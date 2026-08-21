// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! One-time corpus migration: read the legacy SQLite stores
//! (pi-mempalace `memories.db` + ZeroClaw `brain.db`) and import them into
//! the SurrealDB palace + knowledge graph. Behind `backend-sqlite`.
//!
//! ## Mapping (see `docs/HANDOFF.md` §3 for the source schema)
//!
//! - pi-mempalace `memories`: `auto-capture`→`AutoCapture`,
//!   `manual-save`/`diary`→`Explicit`; harness `Pi`.
//! - ZeroClaw `brain.db`: `conversation`→`AutoCapture`, else `Explicit`;
//!   harness `Other` (ZeroClaw is a deprecated predecessor).
//! - Embeddings are NOT ported (the source uses an unreadable `vec0`
//!   virtual table); Ijima re-embeds centrally with candle at write time.
//! - All migrated records stamp `origin`/`authority` = local (ADR
//!   provenance-tier).

use ijima_core::{
    AuthorityScope, InstanceId, Memory, MemoryId, MemorySource, NamespaceId, harness::Harness,
};

/// A raw pi-mempalace `memories` row (the subset we migrate).
#[derive(Debug, Clone)]
pub struct PiPalaceRow {
    pub id: String,
    pub content: String,
    pub project: String,
    pub topic: String,
    pub source: String,
    pub timestamp: String,
    pub session_id: String,
    pub importance: f64,
}

/// A raw ZeroClaw `brain.db` memories row (the subset we migrate).
#[derive(Debug, Clone)]
pub struct ZeroClawRow {
    pub id: String,
    pub content: String,
    pub category: String,
    pub created_at: String,
    pub session_id: Option<String>,
}

/// A raw pi-mempalace `entities` row (the subset we migrate).
#[derive(Debug, Clone)]
pub struct PiPalaceEntity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
}

/// A raw pi-mempalace `triples` row (the subset we migrate). Subject and
/// object reference `entities.id` (opaque `ent_*` hashes), not names.
#[derive(Debug, Clone)]
pub struct PiPalaceTriple {
    pub id: i64,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: Option<f32>,
    pub source_memory_id: Option<String>,
}

/// A knowledge-graph batch prepared for import: name-addressed triples
/// plus a count of unmappable source rows (orphans referencing unknown
/// entities, or empty entity names).
#[derive(Debug, Clone, Default)]
pub struct KgImport {
    pub triples: Vec<ijima_core::ImportTriple>,
    pub unmapped: usize,
}

/// Maps a pi-mempalace source string to an Ijima [`MemorySource`].
/// `auto-capture`→`AutoCapture`; `manual-save` and `diary`→`Explicit`;
/// unknown→`AutoCapture` (conservative: unverified).
pub fn map_pipalace_source(source: &str) -> MemorySource {
    match source {
        "auto-capture" => MemorySource::AutoCapture,
        "manual-save" | "diary" => MemorySource::Explicit,
        _ => MemorySource::AutoCapture,
    }
}

/// Maps a ZeroClaw category to an Ijima [`MemorySource`].
/// `conversation`→`AutoCapture` (raw chatter); everything else→`Explicit`.
pub fn map_zeroclaw_source(category: &str) -> MemorySource {
    match category {
        "conversation" => MemorySource::AutoCapture,
        _ => MemorySource::Explicit,
    }
}

/// Builds an Ijima [`Memory`] from a pi-mempalace row. Harness = `Pi`;
/// empty `session_id` becomes `None`.
pub fn map_pipalace_memory(row: &PiPalaceRow) -> Memory {
    Memory {
        id: MemoryId(row.id.clone()),
        content: row.content.clone(),
        project: row.project.clone(),
        topic: row.topic.clone(),
        source: map_pipalace_source(&row.source),
        harness: Harness::Pi,
        session_id: if row.session_id.is_empty() {
            None
        } else {
            Some(row.session_id.clone())
        },
        importance: row.importance.clamp(0.0, 1.0) as f32,
        created_at: row.timestamp.clone(),
        origin: InstanceId::local(),
        authority: AuthorityScope::local(),
    }
}

/// Builds an Ijima [`Memory`] from a ZeroClaw row. Harness = `Other`;
/// project = `"zeroclaw"`; topic = the row's category.
pub fn map_zeroclaw_memory(row: &ZeroClawRow) -> Memory {
    Memory {
        id: MemoryId(row.id.clone()),
        content: row.content.clone(),
        project: "zeroclaw".to_string(),
        topic: row.category.clone(),
        source: map_zeroclaw_source(&row.category),
        harness: Harness::Other,
        session_id: row.session_id.clone(),
        importance: 0.5,
        created_at: row.created_at.clone(),
        origin: InstanceId::local(),
        authority: AuthorityScope::local(),
    }
}

/// The default namespace migrated records land in: the legacy pi-mempalace
/// "everyone sees everything" commons (DESIGN D2 migration baseline).
pub const MIGRATION_NAMESPACE: &str = "global";

/// The migration namespace as a [`NamespaceId`].
pub fn migration_namespace() -> NamespaceId {
    NamespaceId::new(MIGRATION_NAMESPACE)
}

/// Retags a mapped memory for remote multi-source import (WS2): stamps
/// the origin as the source workstation and drops the trust tier to
/// [`MemorySource::AutoCapture`] regardless of the row's original
/// classification — imported content is unverified until promoted via
/// `trust:promote` (provenance-tier ADR). Harness provenance is
/// preserved (`Pi` for mempalace rows, `Other` for ZeroClaw).
pub fn retag_imported(mut memory: Memory, source: &str) -> Memory {
    memory.origin = InstanceId(source.to_string());
    memory.source = MemorySource::AutoCapture;
    memory
}

/// Default namespace for a remote import from `source` (WS2): one
/// namespace per source, `ns_import_<sanitized>` — never the global
/// commons. Sanitization: lowercase, ASCII alphanumerics kept, every
/// other run of characters collapsed to a single `_`.
pub fn default_import_ns(source: &str) -> NamespaceId {
    let mut sanitized = String::with_capacity(source.len());
    let mut pending_underscore = false;
    for ch in source.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
            pending_underscore = false;
        } else if !pending_underscore {
            sanitized.push('_');
            pending_underscore = true;
        }
    }
    NamespaceId::new(format!("ns_import_{sanitized}"))
}

// ===== SQLite read path (rusqlite, bundled) =====

/// Reads the `memories` table from a pi-mempalace `memories.db`.
///
/// # Errors
/// Returns [`ijima_core::IjimaError::Store`] if the file can't be opened
/// or the query fails.
pub fn read_pipalace_memories(path: &str) -> ijima_core::Result<Vec<PiPalaceRow>> {
    use rusqlite::Connection;
    let conn = Connection::open(path).map_err(store_err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, content, project, topic, source, timestamp, session_id, importance \
             FROM memories ORDER BY rowid",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PiPalaceRow {
                id: row.get(0)?,
                content: row.get(1)?,
                project: row.get(2)?,
                topic: row.get(3)?,
                source: row.get(4)?,
                timestamp: row.get(5)?,
                session_id: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                importance: row.get::<_, Option<f64>>(7)?.unwrap_or(0.5),
            })
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(store_err)?);
    }
    Ok(out)
}

/// Reads the `memories` table from a ZeroClaw `brain.db`.
pub fn read_zeroclaw_memories(path: &str) -> ijima_core::Result<Vec<ZeroClawRow>> {
    use rusqlite::Connection;
    let conn = Connection::open(path).map_err(store_err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, content, category, created_at, session_id FROM memories ORDER BY rowid",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ZeroClawRow {
                id: row.get(0)?,
                content: row.get(1)?,
                category: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "core".into()),
                created_at: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                session_id: row.get(4)?,
            })
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(store_err)?);
    }
    Ok(out)
}

fn store_err(e: rusqlite::Error) -> ijima_core::IjimaError {
    ijima_core::IjimaError::Store {
        detail: format!("migration sqlite: {e}"),
    }
}

/// Reads the `entities` + `triples` tables from a pi-mempalace
/// `memories.db`. Older corpora may lack the knowledge-graph tables —
/// missing tables read as empty, not an error.
pub fn read_pipalace_kg(
    path: &str,
) -> ijima_core::Result<(Vec<PiPalaceEntity>, Vec<PiPalaceTriple>)> {
    use rusqlite::Connection;
    let conn = Connection::open(path).map_err(store_err)?;
    let has_table = |name: &str| -> ijima_core::Result<bool> {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .map_err(store_err)?;
        Ok(n > 0)
    };
    let mut entities = Vec::new();
    if has_table("entities")? {
        let mut stmt = conn
            .prepare("SELECT id, name, entity_type FROM entities ORDER BY id")
            .map_err(store_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PiPalaceEntity {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    entity_type: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "unknown".into()),
                })
            })
            .map_err(store_err)?;
        for row in rows {
            entities.push(row.map_err(store_err)?);
        }
    }
    let mut triples = Vec::new();
    if has_table("triples")? {
        let mut stmt = conn
            .prepare(
                "SELECT id, subject, predicate, object, valid_from, valid_to, \
                 confidence, source_memory_id FROM triples ORDER BY id",
            )
            .map_err(store_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PiPalaceTriple {
                    id: row.get(0)?,
                    subject: row.get(1)?,
                    predicate: row.get(2)?,
                    object: row.get(3)?,
                    valid_from: row.get(4)?,
                    valid_to: row.get(5)?,
                    confidence: row.get::<_, Option<f32>>(6)?,
                    source_memory_id: row.get(7)?,
                })
            })
            .map_err(store_err)?;
        for row in rows {
            triples.push(row.map_err(store_err)?);
        }
    }
    Ok((entities, triples))
}

/// Maps pi-mempalace KG rows onto Ijima's id-is-name convention: entity
/// **names** become `EntityId`s (first entity wins on duplicate names —
/// same-name source entities merge, which is the desired dedup), and
/// triples are re-addressed through the id→name table. Triples that
/// reference unknown entities (orphans) or nameless entities are dropped
/// and counted in `unmapped`.
pub fn map_pipalace_kg(entities: &[PiPalaceEntity], triples: &[PiPalaceTriple]) -> KgImport {
    use std::collections::HashMap;
    let mut by_id: HashMap<&str, &str> = HashMap::new();
    for e in entities {
        let name = e.name.trim();
        if name.is_empty() {
            continue; // unmappable; triples referencing it drop below
        }
        by_id.entry(e.id.as_str()).or_insert(name);
    }
    let mut out = KgImport::default();
    for t in triples {
        let (Some(subj), Some(obj)) = (by_id.get(t.subject.as_str()), by_id.get(t.object.as_str()))
        else {
            out.unmapped += 1;
            continue;
        };
        out.triples.push(ijima_core::ImportTriple {
            subject: (*subj).to_string(),
            predicate: t.predicate.clone(),
            object: (*obj).to_string(),
            valid_from: t.valid_from.clone(),
            valid_to: t.valid_to.clone(),
            confidence: t.confidence.unwrap_or(1.0),
            source_memory_id: t.source_memory_id.clone(),
        });
    }
    out
}

// ===== Import orchestration =====

/// The outcome of a memory-import pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Rows read from the source.
    pub attempted: usize,
    /// Successfully stored in the palace.
    pub imported: usize,
    /// Not stored — content-hash duplicate (the common case) or a store error.
    pub skipped: usize,
}

/// Imports already-mapped `memories` into `store` under `ns`. Content-hash
/// dedup in `store_memory` means exact duplicates (across sources or
/// re-runs) are counted as `skipped`, not errors. Embedding happens at
/// write time if the store was opened with an embedder.
///
/// # Errors
/// Propagates only a fatal store error from the *first* failure path;
/// per-row failures are counted as `skipped` so one bad row doesn't abort
/// the whole migration.
pub async fn import_memories(
    store: &dyn ijima_core::Store,
    ns: &NamespaceId,
    memories: Vec<Memory>,
) -> ijima_core::Result<ImportReport> {
    let mut report = ImportReport {
        attempted: memories.len(),
        ..Default::default()
    };
    for memory in memories {
        match store.store_memory(ns, memory).await {
            Ok(_) => report.imported += 1,
            Err(_) => report.skipped += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipalace_source_mapping() {
        assert_eq!(
            map_pipalace_source("auto-capture"),
            MemorySource::AutoCapture
        );
        assert_eq!(map_pipalace_source("manual-save"), MemorySource::Explicit);
        assert_eq!(map_pipalace_source("diary"), MemorySource::Explicit);
        // Unknown → conservative AutoCapture.
        assert_eq!(map_pipalace_source("???"), MemorySource::AutoCapture);
    }

    #[test]
    fn zeroclaw_source_mapping() {
        assert_eq!(
            map_zeroclaw_source("conversation"),
            MemorySource::AutoCapture
        );
        assert_eq!(map_zeroclaw_source("core"), MemorySource::Explicit);
        assert_eq!(map_zeroclaw_source("daily"), MemorySource::Explicit);
        assert_eq!(map_zeroclaw_source("identity"), MemorySource::Explicit);
    }

    #[test]
    fn retag_imported_stamps_origin_and_drops_to_autocapture() {
        // manual-save maps to Explicit, but WS2 import policy says
        // imported content lands unverified (AutoCapture) until promoted.
        let row = PiPalaceRow {
            id: "mem_imp1".into(),
            content: "saved on the laptop".into(),
            project: "ijima".into(),
            topic: "import".into(),
            source: "manual-save".into(),
            timestamp: "1755400000".into(),
            session_id: String::new(),
            importance: 0.5,
        };
        let tagged = retag_imported(map_pipalace_memory(&row), "elliotthall-laptop");
        assert_eq!(tagged.origin.0, "elliotthall-laptop");
        assert_eq!(tagged.source, MemorySource::AutoCapture);
        assert_eq!(tagged.harness, Harness::Pi);
    }

    #[test]
    fn retag_imported_preserves_zeroclaw_harness() {
        let row = ZeroClawRow {
            id: "zc1".into(),
            content: "discord memory".into(),
            category: "core".into(),
            created_at: "1755400000".into(),
            session_id: None,
        };
        let tagged = retag_imported(map_zeroclaw_memory(&row), "zeroclaw-archive");
        assert_eq!(tagged.origin.0, "zeroclaw-archive");
        assert_eq!(tagged.harness, Harness::Other);
    }

    #[test]
    fn default_import_ns_sanitizes_source() {
        assert_eq!(
            default_import_ns("elliotthall-laptop").as_str(),
            "ns_import_elliotthall_laptop"
        );
        assert_eq!(
            default_import_ns("Laptop 01").as_str(),
            "ns_import_laptop_01"
        );
        assert_eq!(
            default_import_ns("weird!!name").as_str(),
            "ns_import_weird_name"
        );
    }

    #[test]
    fn pipalace_row_maps_with_provenance() {
        let row = PiPalaceRow {
            id: "mem_abc".into(),
            content: "Amari is the flagship math library".into(),
            project: "amari".into(),
            topic: "project-context".into(),
            source: "manual-save".into(),
            timestamp: "2026-04-22T00:23:59.352Z".into(),
            session_id: "sess_7".into(),
            importance: 0.97,
        };
        let m = map_pipalace_memory(&row);
        assert_eq!(m.id.0, "mem_abc");
        assert_eq!(m.project, "amari");
        assert_eq!(m.source, MemorySource::Explicit);
        assert_eq!(m.harness, Harness::Pi);
        assert_eq!(m.session_id.as_deref(), Some("sess_7"));
        assert_eq!(m.importance, 0.97);
        // Provenance-tier stamps.
        assert_eq!(m.origin, InstanceId::local());
        assert_eq!(m.authority, AuthorityScope::local());
    }

    #[test]
    fn pipalace_empty_session_becomes_none() {
        let row = PiPalaceRow {
            id: "x".into(),
            content: "c".into(),
            project: "p".into(),
            topic: "t".into(),
            source: "auto-capture".into(),
            timestamp: "0".into(),
            session_id: String::new(),
            importance: 0.5,
        };
        assert!(map_pipalace_memory(&row).session_id.is_none());
    }

    #[test]
    fn pipalace_importance_clamps_to_unit_range() {
        let mut row = PiPalaceRow {
            id: "x".into(),
            content: "c".into(),
            project: "p".into(),
            topic: "t".into(),
            source: "auto-capture".into(),
            timestamp: "0".into(),
            session_id: String::new(),
            importance: 5.0,
        };
        assert_eq!(map_pipalace_memory(&row).importance, 1.0);
        row.importance = -1.0;
        assert_eq!(map_pipalace_memory(&row).importance, 0.0);
    }

    #[test]
    fn zeroclaw_row_maps_with_category_as_topic() {
        let row = ZeroClawRow {
            id: "uuid-1".into(),
            content: "Lucien brought me online".into(),
            category: "core".into(),
            created_at: "2026-06-16T09:22:38Z".into(),
            session_id: None,
        };
        let m = map_zeroclaw_memory(&row);
        assert_eq!(m.project, "zeroclaw");
        assert_eq!(m.topic, "core");
        assert_eq!(m.source, MemorySource::Explicit);
        assert_eq!(m.harness, Harness::Other);
        assert!(m.session_id.is_none());
        assert_eq!(m.origin, InstanceId::local());
    }

    #[test]
    fn zeroclaw_conversation_is_autocapture() {
        let row = ZeroClawRow {
            id: "u".into(),
            content: "Say hello".into(),
            category: "conversation".into(),
            created_at: "0".into(),
            session_id: Some("s".into()),
        };
        assert_eq!(map_zeroclaw_memory(&row).source, MemorySource::AutoCapture);
        assert_eq!(map_zeroclaw_memory(&row).session_id.as_deref(), Some("s"));
    }

    #[test]
    fn migration_namespace_is_global_commons() {
        assert_eq!(MIGRATION_NAMESPACE, "global");
        assert_eq!(migration_namespace().as_str(), "global");
    }

    #[test]
    fn read_pipalace_memories_round_trips_a_fixture_db() {
        use rusqlite::Connection;
        let mut path = std::env::temp_dir();
        path.push(format!("ijima_mig_test_{}.db", std::process::id()));
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE memories (id TEXT, content TEXT, project TEXT, topic TEXT, \
             source TEXT, timestamp TEXT, session_id TEXT, importance REAL)",
            [],
        )
        .expect("create");
        conn.execute(
            "INSERT INTO memories VALUES ('m1','c','p','t','manual-save','ts','s1',0.9)",
            [],
        )
        .expect("insert");
        drop(conn);
        let rows = read_pipalace_memories(path.to_str().expect("path")).expect("read");
        let _ = std::fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "m1");
        assert_eq!(rows[0].source, "manual-save");
        assert_eq!(rows[0].importance, 0.9);
        // Reader feeds the mapper cleanly.
        let m = map_pipalace_memory(&rows[0]);
        assert_eq!(m.source, MemorySource::Explicit);
    }

    #[test]
    fn read_pipalace_kg_round_trips_and_tolerates_missing_tables() {
        use rusqlite::Connection;
        let mut path = std::env::temp_dir();
        path.push(format!("ijima_mig_kg_{}.db", std::process::id()));
        let conn = Connection::open(&path).expect("open");
        conn.execute(
            "CREATE TABLE entities (id TEXT PRIMARY KEY, name TEXT, entity_type TEXT)",
            [],
        )
        .expect("create entities");
        conn.execute(
            "CREATE TABLE triples (id INTEGER PRIMARY KEY AUTOINCREMENT, subject TEXT, \
             predicate TEXT, object TEXT, valid_from TEXT, valid_to TEXT, confidence REAL, \
             source_memory_id TEXT)",
            [],
        )
        .expect("create triples");
        conn.execute(
            "INSERT INTO entities VALUES ('ent_a','Ijima','project'), ('ent_b','Schubert','project')",
            [],
        )
        .expect("insert entities");
        conn.execute(
            "INSERT INTO triples (subject, predicate, object, valid_from, valid_to, confidence, \
             source_memory_id) VALUES ('ent_a','depends_on','ent_b','2026-08-01',NULL,0.9,'mem_1')",
            [],
        )
        .expect("insert triple");
        drop(conn);
        let (entities, triples) = read_pipalace_kg(path.to_str().expect("path")).expect("read kg");
        let _ = std::fs::remove_file(&path);
        assert_eq!(entities.len(), 2);
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].subject, "ent_a");
        assert_eq!(triples[0].confidence, Some(0.9));

        // A corpus without KG tables reads as empty, not an error.
        let mut bare = std::env::temp_dir();
        bare.push(format!("ijima_mig_kg_bare_{}.db", std::process::id()));
        let conn = Connection::open(&bare).expect("open bare");
        conn.execute("CREATE TABLE memories (id TEXT)", [])
            .expect("create");
        drop(conn);
        let (e, t) = read_pipalace_kg(bare.to_str().expect("path")).expect("read bare");
        let _ = std::fs::remove_file(&bare);
        assert!(e.is_empty());
        assert!(t.is_empty());
    }

    #[test]
    fn map_pipalace_kg_translates_ids_to_names() {
        let entities = vec![
            PiPalaceEntity {
                id: "ent_a".into(),
                name: "Ijima".into(),
                entity_type: "project".into(),
            },
            PiPalaceEntity {
                id: "ent_b".into(),
                name: "Schubert".into(),
                entity_type: "project".into(),
            },
            PiPalaceEntity {
                id: "ent_dup".into(),
                name: "Ijima".into(), // duplicate name: merges with ent_a
                entity_type: "project".into(),
            },
            PiPalaceEntity {
                id: "ent_empty".into(),
                name: "   ".into(), // nameless: unmappable
                entity_type: "unknown".into(),
            },
        ];
        let triples = vec![
            PiPalaceTriple {
                id: 1,
                subject: "ent_a".into(),
                predicate: "depends_on".into(),
                object: "ent_b".into(),
                valid_from: Some("2026-08-01".into()),
                valid_to: None,
                confidence: Some(0.9),
                source_memory_id: Some("mem_1".into()),
            },
            PiPalaceTriple {
                id: 2,
                subject: "ent_dup".into(), // same name → dedups onto Ijima
                predicate: "ships_with".into(),
                object: "ent_b".into(),
                valid_from: Some("2026-08-02".into()),
                valid_to: Some("2026-08-10".into()),
                confidence: None,
                source_memory_id: None,
            },
            PiPalaceTriple {
                id: 3,
                subject: "ent_missing".into(), // orphan → dropped
                predicate: "broken".into(),
                object: "ent_a".into(),
                valid_from: None,
                valid_to: None,
                confidence: None,
                source_memory_id: None,
            },
            PiPalaceTriple {
                id: 4,
                subject: "ent_empty".into(), // nameless → dropped
                predicate: "broken".into(),
                object: "ent_a".into(),
                valid_from: None,
                valid_to: None,
                confidence: None,
                source_memory_id: None,
            },
        ];
        let kg = map_pipalace_kg(&entities, &triples);
        assert_eq!(kg.unmapped, 2);
        assert_eq!(kg.triples.len(), 2);
        assert_eq!(kg.triples[0].subject, "Ijima");
        assert_eq!(kg.triples[0].object, "Schubert");
        assert_eq!(kg.triples[0].confidence, 0.9);
        assert_eq!(kg.triples[1].subject, "Ijima"); // ent_dup merged
        assert_eq!(kg.triples[1].valid_to.as_deref(), Some("2026-08-10"));
        assert_eq!(kg.triples[1].confidence, 1.0);
    }

    #[tokio::test]
    async fn import_memories_counts_dups_as_skipped() {
        // Content-hash dedup: importing the same memory twice → second is
        // rejected by store_memory and counted as skipped, not a hard error.
        let store = crate::SurrealStore::open_embedded().await.expect("open");
        let ns = migration_namespace();
        let mem = map_pipalace_memory(&PiPalaceRow {
            id: "mem_dup".into(),
            content: "deterministic dup-content for migration test".into(),
            project: "ijima".into(),
            topic: "test".into(),
            source: "manual-save".into(),
            timestamp: "0".into(),
            session_id: String::new(),
            importance: 0.5,
        });
        let first = import_memories(&store, &ns, vec![mem.clone()])
            .await
            .expect("import");
        assert_eq!(
            first,
            ImportReport {
                attempted: 1,
                imported: 1,
                skipped: 0
            }
        );
        let second = import_memories(&store, &ns, vec![mem])
            .await
            .expect("import");
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 1);
    }
}
