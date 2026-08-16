// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! SurrealDB store backend — Ijima's primary backend (D6).
//!
//! Implements [`ijima_core::Store`] over an embedded SurrealDB instance
//! (`kv-mem` engine). The same impl will back a server-mode deployment
//! once the engine type is widened (future).
//!
//! ## Vector search
//!
//! When constructed with [`SurrealStore::open_embedded_with`] (an
//! [`Embedder`]), each stored memory is embedded at write time and
//! indexed via **Cosine similarity** ranking (`vector::similarity::cosine`),
//! brute-force over the namespace (an **HNSW** index is the planned
//! optimization once the SDK can emit typed `<N>f32` vectors; MTREE is
//! deprecated in SurrealDB 2.6). `search_memories`
//!
//! ## Namespace handling (v0)
//!
//! Ijima's [`NamespaceId`](ijima_core::NamespaceId) is stored as a record
//! field and every query filters by it, so isolation is enforced
//! per-query under concurrent access. A future promotion maps each
//! namespace onto a native SurrealDB `NS`/`DB` scope once the
//! concurrency semantics of per-query `USE NS/DB` are validated.

use std::sync::Arc;

use async_trait::async_trait;
use ijima_core::{
    AcceptedExtraction, AuthorityScope, DiaryEntry, Embedding, Entity, EntityId, EntityRecord,
    IjimaError, InstanceId, KgStats, KnowledgeGraph, Memory, MemoryId, NamespaceCount, NamespaceId,
    PalaceGraph, ProjectTaxon, QueuedExtraction, RepoDirectory, Result, Room, SearchHit, Session,
    SessionId, SessionTurn, Store, StoreStats, TokenRevocation, Triple, Tunnel, TunnelTraversal,
    embeddings::Embedder, harness::Harness, memory::MemorySource,
};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem, SurrealKv};

/// The SurrealDB namespace/database the Ijima instance lives in.
const SURREAL_NS: &str = "ijima";
const SURREAL_DB: &str = "core";

const MEMORIES_TABLE: &str = "memories";
const TURNS_TABLE: &str = "session_turns";
const SESSIONS_TABLE: &str = "sessions";
const DIARY_TABLE: &str = "diaries";
const QUEUE_TABLE: &str = "mining_queue";
const REVOCATIONS_TABLE: &str = "token_revocations";
/// Entity nodes (knowledge-graph).
const ENTITIES_TABLE: &str = "entities";
/// Triple edges (knowledge-graph).
const TRIPLES_TABLE: &str = "triples";
/// Repo directory (global Context Mapper registry).
const REPO_TABLE: &str = "repo_directory";

/// A SurrealDB-backed [`Store`].
pub struct SurrealStore {
    db: Surreal<Db>,
    /// When present, memories are embedded at write time and
    /// [`Store::search_memories`] is available. When absent, search
    /// returns [`IjimaError::Store`].
    embedder: Option<Arc<dyn Embedder>>,
}

impl SurrealStore {
    /// Opens an **in-memory** embedded store (the `Mem` engine), with no
    /// embedder. Use for tests; data does not survive restart.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize.
    pub async fn open_embedded() -> Result<Self> {
        Self::open_with_db(new_mem().await?, None).await
    }

    /// Opens an **in-memory** embedded store with an [`Embedder`].
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize.
    pub async fn open_embedded_with(embedder: Arc<dyn Embedder>) -> Result<Self> {
        Self::open_with_db(new_mem().await?, Some(embedder)).await
    }

    /// Opens a **persistent** store (the `SurrealKv` engine) at `path`,
    /// with no embedder. Data survives restart. Creates the directory if
    /// absent.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize or
    /// the path is unwritable.
    pub async fn open_persistent(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::open_with_db(new_surrealkv(&path).await?, None).await
    }

    /// Opens a **persistent** store with an [`Embedder`].
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize or
    /// the path is unwritable.
    pub async fn open_persistent_with(
        path: impl AsRef<std::path::Path>,
        embedder: Arc<dyn Embedder>,
    ) -> Result<Self> {
        Self::open_with_db(new_surrealkv(&path).await?, Some(embedder)).await
    }

    async fn open_with_db(db: Surreal<Db>, embedder: Option<Arc<dyn Embedder>>) -> Result<Self> {
        db.use_ns(SURREAL_NS)
            .use_db(SURREAL_DB)
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("surrealdb use_ns/use_db: {e}"),
            })?;
        // Define indexes on the hot query columns. SurrealDB is schemaless
        // by default and does NOT auto-index fields, so without these every
        // namespace-scoped SELECT and every dedup check is a full table scan
        // (O(N²) for a bulk import). `IF NOT EXISTS` makes this idempotent
        // on existing stores.
        db.query(Self::INDEX_DDL)
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("surrealdb define indexes: {e}"),
            })?;
        Ok(Self { db, embedder })
    }

    /// The `DEFINE INDEX` statements run at store open. Covers the columns
    /// every query filters on: `namespace` (all scoped reads),
    /// `(namespace, content_hash)` (dedup), `(namespace, session_id)`
    /// (session turns), and the knowledge-graph traversal keys.
    const INDEX_DDL: &str = r#"
        DEFINE INDEX IF NOT EXISTS mem_ns        ON TABLE memories       FIELDS namespace;
        DEFINE INDEX IF NOT EXISTS mem_ns_hash   ON TABLE memories       FIELDS namespace, content_hash;
        DEFINE INDEX IF NOT EXISTS turns_ns_sess ON TABLE session_turns   FIELDS namespace, session_id;
        DEFINE INDEX IF NOT EXISTS sess_ns       ON TABLE sessions        FIELDS namespace;
        DEFINE INDEX IF NOT EXISTS diary_ns_ag   ON TABLE diaries         FIELDS namespace, agent;
        DEFINE INDEX IF NOT EXISTS queue_ns      ON TABLE mining_queue    FIELDS namespace;
        DEFINE INDEX IF NOT EXISTS ent_ns        ON TABLE entities        FIELDS namespace;
        DEFINE INDEX IF NOT EXISTS trip_ns       ON TABLE triples         FIELDS namespace;
        DEFINE INDEX IF NOT EXISTS trip_subj     ON TABLE triples         FIELDS subject;
        DEFINE INDEX IF NOT EXISTS trip_obj      ON TABLE triples         FIELDS object;
    "#;

    /// Fetches all `(project, topic)` cells in `ns` with their memory counts.
    /// One query, aggregated in Rust (consistent with `store_stats`).
    async fn project_topic_counts(
        &self,
        ns: &NamespaceId,
    ) -> Result<std::collections::BTreeMap<(String, String), usize>> {
        #[derive(Deserialize)]
        struct Row {
            project: String,
            topic: String,
        }
        let mut result = self
            .db
            .query(format!(
                "SELECT project, topic FROM {MEMORIES_TABLE} WHERE namespace = $ns"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .await
            .map_err(store_err)?;
        let rows: Vec<Row> = result.take(0).map_err(store_err)?;
        let mut counts = std::collections::BTreeMap::new();
        for r in rows {
            *counts.entry((r.project, r.topic)).or_insert(0) += 1;
        }
        Ok(counts)
    }

    /// Returns memories in `ns` matching `project` + `topic`, ranked by
    /// importance desc then recency desc (mirrors `list_memories`).
    async fn project_topic_memories(
        &self,
        ns: &NamespaceId,
        project: &str,
        topic: &str,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT memory_id, content, project, topic, source, harness, session_id, namespace, importance, created_at
                 FROM {MEMORIES_TABLE}
                 WHERE namespace = $ns AND project = $proj AND topic = $topic
                 ORDER BY importance DESC, created_at DESC LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("proj", project.to_string()))
            .bind(("topic", topic.to_string()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let records: Vec<MemoryRecord> = result.take(0).map_err(store_err)?;
        Ok(records.into_iter().map(|r| r.into_memory()).collect())
    }

    /// Exports the entire store as a SurrealDB SQL dump to `path`.
    /// Requires a persistent backend (SurrealKv); in-memory stores do not
    /// support Backup.
    pub async fn export_to(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        self.db
            .export(path.as_ref())
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("export: {e}"),
            })?;
        Ok(())
    }
}

async fn new_mem() -> Result<Surreal<Db>> {
    Surreal::new::<Mem>(())
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("surrealdb mem init: {e}"),
        })
}

async fn new_surrealkv(path: impl AsRef<std::path::Path>) -> Result<Surreal<Db>> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| IjimaError::Store {
            detail: format!("mkdir {}: {e}", parent.display()),
        })?;
    }
    Surreal::new::<SurrealKv>(path.to_string_lossy().to_string())
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("surrealdb surrealkv init at {}: {e}", path.display()),
        })
}

// ---------- wire records ----------

/// The persisted form of a [`Memory`] plus its owning namespace and an
/// optional embedding vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryRecord {
    /// The [`MemoryId`] string (mirrors the SurrealDB record id so search
    /// results can return it without parsing a RecordId).
    memory_id: String,
    content: String,
    /// SHA-256 of `content` (content-hash dedup, Phase 2.2). Stored for
    /// O(1) exact-duplicate lookup within a namespace. `#[serde(default)]`
    /// so SELECTs that omit it (list/search projections) still deserialize.
    #[serde(default)]
    content_hash: String,
    project: String,
    topic: String,
    source: MemorySource,
    harness: Harness,
    session_id: Option<String>,
    namespace: String,
    #[serde(default = "default_record_importance")]
    importance: f32,
    #[serde(default)]
    created_at: String,
    /// Provenance: the authoring instance (ADR provenance-tier). Defaults
    /// to local for documents written before the field existed.
    #[serde(default)]
    origin: InstanceId,
    /// Provenance: the authority scope (source-of-truth) for the record's
    /// domain (ADR provenance-tier). Defaults to local.
    #[serde(default)]
    authority: AuthorityScope,
    /// Which embedding model produced `embedding` (D10 provenance), e.g.
    /// `sentence-transformers/all-MiniLM-L6-v2@main`. Absent when no
    /// embedder is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    embed_model: Option<String>,
    /// Embedding vector (present when the store was opened with an
    /// [`Embedder`]). `#[serde(default)]` so namespace-filtered selects
    /// that omit it still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    embedding: Option<Vec<f32>>,
}

fn default_record_importance() -> f32 {
    0.5
}

impl MemoryRecord {
    fn from_memory(
        memory: &Memory,
        ns: &NamespaceId,
        embedding: Option<Vec<f32>>,
        embed_model: Option<String>,
    ) -> Self {
        use sha2::{Digest, Sha256};
        let content_hash = hex(&Sha256::digest(memory.content.as_bytes()));
        Self {
            memory_id: memory.id.0.clone(),
            content: memory.content.clone(),
            content_hash,
            project: memory.project.clone(),
            topic: memory.topic.clone(),
            source: memory.source,
            harness: memory.harness,
            session_id: memory.session_id.clone(),
            namespace: ns.as_str().to_string(),
            importance: memory.importance,
            created_at: memory.created_at.clone(),
            origin: memory.origin.clone(),
            authority: memory.authority.clone(),
            embed_model,
            embedding,
        }
    }

    fn into_memory(self) -> Memory {
        Memory {
            id: MemoryId(self.memory_id),
            content: self.content,
            project: self.project,
            topic: self.topic,
            source: self.source,
            harness: self.harness,
            session_id: self.session_id,
            origin: self.origin,
            authority: self.authority,
            importance: self.importance,
            created_at: self.created_at,
        }
    }
}

/// The persisted form of a [`SessionTurn`] plus its owning namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionTurnRecord {
    session_id: String,
    turn_index: u64,
    role: ijima_core::TurnRole,
    content: String,
    timestamp: String,
    namespace: String,
}

/// Stored row for a diary entry (Phase 3.3). `ts` is an internal
/// epoch-millis field for correct numeric ORDER BY (string timestamps
/// don't sort lexicographically across formats).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiaryRecord {
    agent: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    timestamp: String,
    ts: i64,
    namespace: String,
}

impl DiaryRecord {
    fn from_entry(entry: &ijima_core::DiaryEntry, ns: &NamespaceId, now_ms: i64) -> Self {
        Self {
            agent: entry.agent.clone(),
            content: entry.content.clone(),
            topic: entry.topic.clone(),
            timestamp: entry.timestamp.clone(),
            ts: now_ms,
            namespace: ns.as_str().to_string(),
        }
    }

    fn into_entry(self) -> ijima_core::DiaryEntry {
        ijima_core::DiaryEntry {
            agent: self.agent,
            content: self.content,
            topic: self.topic,
            timestamp: self.timestamp,
        }
    }
}

/// Stored row for a session's metadata (Phase 2.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    session_id: String,
    harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
    namespace: String,
}

impl SessionRecord {
    fn from_session(session: &Session, ns: &NamespaceId) -> Self {
        Self {
            session_id: session.id.0.clone(),
            harness: session.harness.as_wire_str().to_string(),
            channel: session.channel.clone(),
            started_at: session.started_at.clone(),
            ended_at: session.ended_at.clone(),
            namespace: ns.as_str().to_string(),
        }
    }

    fn into_session(self) -> Session {
        Session {
            id: SessionId(self.session_id),
            harness: Harness::from_wire_str(&self.harness),
            channel: self.channel,
            started_at: self.started_at,
            ended_at: self.ended_at,
        }
    }
}

/// Stored row for a queued mining extraction (ADR M2). `ts` is an internal
/// epoch-millis for correct ORDER BY.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueRecord {
    queue_id: String,
    /// Nested memory fields (flattened for storage).
    memory_id: String,
    content: String,
    project: String,
    topic: String,
    #[serde(default)]
    importance: f32,
    session_id: Option<String>,
    harness: String,
    confidence: f32,
    source_session_id: String,
    ts: i64,
    namespace: String,
}

impl QueueRecord {
    fn from_extraction(
        queue_id: &str,
        memory: &Memory,
        confidence: f32,
        ns: &NamespaceId,
        now_ms: i64,
    ) -> Self {
        Self {
            queue_id: queue_id.to_string(),
            memory_id: memory.id.0.clone(),
            content: memory.content.clone(),
            project: memory.project.clone(),
            topic: memory.topic.clone(),
            importance: memory.importance,
            session_id: memory.session_id.clone(),
            harness: memory.harness.as_wire_str().to_string(),
            confidence,
            source_session_id: memory.session_id.clone().unwrap_or_default(),
            ts: now_ms,
            namespace: ns.as_str().to_string(),
        }
    }

    fn into_queued(self) -> QueuedExtraction {
        let harness = Harness::from_wire_str(&self.harness);
        let memory = Memory {
            id: MemoryId(self.memory_id),
            content: self.content,
            project: self.project,
            topic: self.topic,
            source: MemorySource::Mined,
            harness,
            session_id: self.session_id,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: self.importance,
            created_at: String::new(),
        };
        QueuedExtraction {
            id: self.queue_id,
            memory,
            confidence: self.confidence,
            source_session_id: self.source_session_id,
            queued_at: self.ts.to_string(),
        }
    }

    fn into_memory(self) -> Memory {
        Memory {
            id: MemoryId(self.memory_id),
            content: self.content,
            project: self.project,
            topic: self.topic,
            source: MemorySource::Mined,
            harness: Harness::from_wire_str(&self.harness),
            session_id: self.session_id,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: self.importance,
            created_at: String::new(),
        }
    }
}

fn store_err(e: surrealdb::Error) -> IjimaError {
    IjimaError::Store {
        detail: e.to_string(),
    }
}

/// Current epoch-millis (for internal ORDER BY fields).
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Lowercase hex of a byte slice (for content-hash).
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn embed_for(embedder: &dyn Embedder, text: &str) -> Result<Option<Vec<f32>>> {
    Ok(Some(embedder.embed(text)?.0))
}

#[async_trait]
impl Store for SurrealStore {
    async fn store_memory(&self, ns: &NamespaceId, memory: Memory) -> Result<MemoryId> {
        // Content-hash dedup (Phase 2.2): reject exact duplicates within
        // the namespace, returning the existing id so callers can recover.
        if let Some(existing) = self.check_duplicate(ns, &memory.content).await? {
            return Err(IjimaError::duplicate(format!(
                "content already stored as {}",
                existing.0
            )));
        }
        let (embedding, embed_model) = match &self.embedder {
            Some(e) => {
                // Stamp the model id (D10 provenance) so a future model
                // swap is detectable and a re-embed pass can be triggered.
                (
                    embed_for(e.as_ref(), &memory.content)?,
                    Some(e.model_id().to_string()),
                )
            }
            None => (None, None),
        };
        let id_str = memory.id.0.clone();
        let record = MemoryRecord::from_memory(&memory, ns, embedding, embed_model);
        let _: Option<MemoryRecord> = self
            .db
            .create((MEMORIES_TABLE, id_str.clone()))
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(memory.id)
    }

    async fn check_duplicate(&self, ns: &NamespaceId, content: &str) -> Result<Option<MemoryId>> {
        use sha2::{Digest, Sha256};
        let hash = hex(&Sha256::digest(content.as_bytes()));
        let mut result = self
            .db
            .query(format!(
                "SELECT memory_id FROM {MEMORIES_TABLE}
                 WHERE namespace = $ns AND content_hash = $hash LIMIT 1"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("hash", hash))
            .await
            .map_err(store_err)?;
        #[derive(Deserialize)]
        struct IdRow {
            memory_id: String,
        }
        let rows: Vec<IdRow> = result.take(0).map_err(store_err)?;
        Ok(rows.into_iter().next().map(|r| MemoryId(r.memory_id)))
    }

    async fn recall_memory(&self, ns: &NamespaceId, id: &MemoryId) -> Result<Option<Memory>> {
        let record: Option<MemoryRecord> = self
            .db
            .select((MEMORIES_TABLE, id.0.clone()))
            .await
            .map_err(store_err)?;
        Ok(record
            .filter(|r| r.namespace == ns.as_str())
            .map(|r| r.into_memory()))
    }

    async fn delete_memory(&self, ns: &NamespaceId, id: &MemoryId) -> Result<()> {
        // Verify ownership before deleting (isolation).
        let existing = self.recall_memory(ns, id).await?;
        if existing.is_none() {
            return Ok(()); // absent or wrong namespace — nothing to delete
        }
        let _: Option<MemoryRecord> = self
            .db
            .delete((MEMORIES_TABLE, id.0.clone()))
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn list_memories(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<Memory>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT memory_id, content, project, topic, source, harness, session_id, namespace, importance, created_at
                 FROM {MEMORIES_TABLE}
                 WHERE namespace = $ns
                 ORDER BY importance DESC, created_at DESC
                 LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let records: Vec<MemoryRecord> = result.take(0).map_err(store_err)?;
        Ok(records.into_iter().map(|r| r.into_memory()).collect())
    }

    async fn store_stats(&self) -> Result<StoreStats> {
        // One query for all namespaces; aggregate in Rust (avoids
        // SurrealDB aggregate-function ambiguity, fine at v0 scale).
        #[derive(Deserialize)]
        struct NsRow {
            namespace: String,
        }
        let mut result = self
            .db
            .query(format!("SELECT namespace FROM {MEMORIES_TABLE}"))
            .await
            .map_err(store_err)?;
        let rows: Vec<NsRow> = result.take(0).map_err(store_err)?;
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for r in &rows {
            *counts.entry(r.namespace.clone()).or_insert(0) += 1;
        }
        let namespaces = counts
            .into_iter()
            .map(|(namespace, memories)| NamespaceCount {
                namespace,
                memories,
            })
            .collect();
        Ok(StoreStats {
            total_memories: rows.len(),
            namespaces,
        })
    }

    async fn list_rooms(
        &self,
        ns: &NamespaceId,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Room>> {
        let counts = self.project_topic_counts(ns).await?;
        let mut rooms: Vec<Room> = counts
            .into_iter()
            .filter(|((p, _), _)| project.is_none_or(|proj| p == proj))
            .map(|((project, topic), count)| Room {
                project,
                topic,
                count,
            })
            .collect();
        // count desc, then topic for determinism.
        rooms.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.topic.cmp(&b.topic)));
        rooms.truncate(limit);
        Ok(rooms)
    }

    async fn taxonomy(&self, ns: &NamespaceId) -> Result<Vec<ProjectTaxon>> {
        let counts = self.project_topic_counts(ns).await?;
        // group by project
        let mut by_project: std::collections::BTreeMap<String, Vec<Room>> =
            std::collections::BTreeMap::new();
        for ((project, topic), count) in counts {
            by_project.entry(project).or_default().push(Room {
                project: String::new(),
                topic,
                count,
            });
        }
        let mut taxons: Vec<ProjectTaxon> = by_project
            .into_iter()
            .map(|(project, mut rooms)| {
                rooms.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.topic.cmp(&b.topic)));
                for r in &mut rooms {
                    r.project = project.clone();
                }
                let total = rooms.iter().map(|r| r.count).sum();
                ProjectTaxon {
                    project,
                    rooms,
                    total,
                }
            })
            .collect();
        // projects with the most memories first.
        taxons.sort_by(|a, b| {
            b.total
                .cmp(&a.total)
                .then_with(|| a.project.cmp(&b.project))
        });
        Ok(taxons)
    }

    async fn palace_graph(&self, ns: &NamespaceId) -> Result<PalaceGraph> {
        let counts = self.project_topic_counts(ns).await?;
        // topic -> set of (project, count)
        let mut topics: std::collections::BTreeMap<String, Vec<(String, usize)>> =
            std::collections::BTreeMap::new();
        let mut projects: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for ((project, topic), count) in counts {
            projects.insert(project.clone());
            topics.entry(topic).or_default().push((project, count));
        }
        let mut tunnels = Vec::new();
        for (topic, mut entries) in topics {
            if entries.len() < 2 {
                continue; // a tunnel needs two distinct projects
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            // every unordered pair of distinct projects on this topic
            for i in 0..entries.len() {
                for j in (i + 1)..entries.len() {
                    tunnels.push(Tunnel {
                        topic: topic.clone(),
                        project_a: entries[i].0.clone(),
                        project_b: entries[j].0.clone(),
                        count_a: entries[i].1,
                        count_b: entries[j].1,
                    });
                }
            }
        }
        Ok(PalaceGraph {
            projects: projects.into_iter().collect(),
            tunnels,
        })
    }

    async fn traverse_tunnel(
        &self,
        ns: &NamespaceId,
        topic: &str,
        project_a: &str,
        project_b: &str,
        limit: usize,
    ) -> Result<TunnelTraversal> {
        let memories_a = self
            .project_topic_memories(ns, project_a, topic, limit)
            .await?;
        let memories_b = self
            .project_topic_memories(ns, project_b, topic, limit)
            .await?;
        Ok(TunnelTraversal {
            topic: topic.to_string(),
            project_a: project_a.to_string(),
            project_b: project_b.to_string(),
            memories_a,
            memories_b,
        })
    }

    async fn search_memories(
        &self,
        ns: &NamespaceId,
        embedding: &Embedding,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if self.embedder.is_none() {
            return Err(IjimaError::Store {
                detail: "search requires the store to opened with an Embedder".into(),
            });
        }
        // Cosine similarity ranking, brute-force over the namespace.
        // Correct (no ANN approximation); HNSW is the planned
        // optimization once the SDK emits typed `<N>f32` vectors.
        let mut result = self
            .db
            .query(format!(
                "SELECT memory_id, content, project, topic, source, harness, session_id, namespace,
                        origin, authority, importance, created_at,
                        vector::similarity::cosine(embedding, $query) AS score
                 FROM {MEMORIES_TABLE}
                 WHERE namespace = $ns AND embedding IS NOT NONE
                 ORDER BY score DESC
                 LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("query", embedding.0.clone()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        // Deserialize each row as a MemoryRecord (missing fields default)
        // flattened alongside the computed `score`.
        #[derive(Deserialize)]
        struct ScoredRecord {
            #[serde(flatten)]
            rec: MemoryRecord,
            score: f64,
        }
        let rows: Vec<ScoredRecord> = result.take(0).map_err(store_err)?;
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                memory: r.rec.into_memory(),
                similarity: r.score as f32,
            })
            .collect())
    }

    async fn ingest_turn(&self, ns: &NamespaceId, turn: SessionTurn) -> Result<()> {
        let record = SessionTurnRecord {
            session_id: turn.session_id.0.clone(),
            turn_index: turn.turn_index,
            role: turn.role,
            content: turn.content,
            timestamp: turn.timestamp,
            namespace: ns.as_str().to_string(),
        };
        let _: Option<SessionTurnRecord> = self
            .db
            .create(TURNS_TABLE)
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn session_turns(
        &self,
        ns: &NamespaceId,
        session: &SessionId,
        limit: usize,
    ) -> Result<Vec<SessionTurn>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT session_id, turn_index, role, content, timestamp, namespace
                 FROM {TURNS_TABLE}
                 WHERE namespace = $ns AND session_id = $sid
                 ORDER BY turn_index DESC LIMIT $lim",
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("sid", session.0.clone()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let mut records: Vec<SessionTurnRecord> = result.take(0).map_err(store_err)?;
        records.reverse(); // query was DESC for "last N"; return chronological
        Ok(records
            .into_iter()
            .map(|r| SessionTurn {
                session_id: SessionId(r.session_id),
                turn_index: r.turn_index,
                role: r.role,
                content: r.content,
                timestamp: r.timestamp,
            })
            .collect())
    }

    async fn create_session(&self, ns: &NamespaceId, session: Session) -> Result<SessionId> {
        let id_str = session.id.0.clone();
        let record = SessionRecord::from_session(&session, ns);
        // Upsert: create if absent, replace metadata if present. Session
        // ids are globally unique (same convention as memory ids).
        let _: Option<SessionRecord> = self
            .db
            .upsert((SESSIONS_TABLE, id_str.clone()))
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(session.id)
    }

    async fn list_sessions(
        &self,
        ns: &NamespaceId,
        harness: Option<&Harness>,
        limit: usize,
    ) -> Result<Vec<Session>> {
        let mut query = format!(
            "SELECT session_id, harness, channel, started_at, ended_at, namespace
             FROM {SESSIONS_TABLE}
             WHERE namespace = $ns"
        );
        if harness.is_some() {
            query.push_str(" AND harness = $harness");
        }
        query.push_str(" ORDER BY started_at DESC LIMIT $lim");
        let mut q = self
            .db
            .query(query)
            .bind(("ns", ns.as_str().to_string()))
            .bind(("lim", limit as i64));
        if let Some(h) = harness {
            q = q.bind(("harness", h.as_wire_str().to_string()));
        }
        let res = q.await.map_err(store_err)?;
        let mut res = res;
        let rows: Vec<SessionRecord> = res.take(0).map_err(store_err)?;
        Ok(rows.into_iter().map(SessionRecord::into_session).collect())
    }

    async fn end_session(
        &self,
        ns: &NamespaceId,
        session: &SessionId,
        ended_at: String,
    ) -> Result<()> {
        // Scoped by namespace + session id (safety: a principal can only
        // end sessions in their own namespace).
        let _ = self
            .db
            .query(format!(
                "UPDATE {SESSIONS_TABLE}
                 SET ended_at = $ended
                 WHERE namespace = $ns AND session_id = $sid"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("sid", session.0.clone()))
            .bind(("ended", ended_at))
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn enqueue_extraction(
        &self,
        ns: &NamespaceId,
        memory: Memory,
        confidence: f32,
    ) -> Result<String> {
        let now_ms = now_millis();
        let queue_id = format!("q_{}_{now_ms}", memory.id.0);
        let record = QueueRecord::from_extraction(&queue_id, &memory, confidence, ns, now_ms);
        let _: Option<QueueRecord> = self
            .db
            .create((QUEUE_TABLE, queue_id.clone()))
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(queue_id)
    }

    async fn list_pending(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<QueuedExtraction>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT queue_id, memory_id, content, project, topic, importance, session_id, harness, confidence, source_session_id, ts, namespace
                 FROM {QUEUE_TABLE}
                 WHERE namespace = $ns
                 ORDER BY ts DESC LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let records: Vec<QueueRecord> = result.take(0).map_err(store_err)?;
        Ok(records.into_iter().map(QueueRecord::into_queued).collect())
    }

    async fn accept_extraction(
        &self,
        ns: &NamespaceId,
        queue_id: &str,
    ) -> Result<AcceptedExtraction> {
        // Read the queued record (scoped by namespace), promote to the
        // palace, then delete from the queue.
        let mut result = self
            .db
            .query(format!(
                "SELECT queue_id, memory_id, content, project, topic, importance, session_id, harness, confidence, source_session_id, ts, namespace
                 FROM {QUEUE_TABLE}
                 WHERE namespace = $ns AND queue_id = $qid LIMIT 1"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("qid", queue_id.to_string()))
            .await
            .map_err(store_err)?;
        let records: Vec<QueueRecord> = result.take(0).map_err(store_err)?;
        let Some(record) = records.into_iter().next() else {
            return Err(IjimaError::not_found(format!("queue entry {queue_id}")));
        };
        let memory = record.into_memory();
        let memory_id = memory.id.clone();
        // store_memory does content-hash dedup; a duplicate promote is a no-op.
        self.store_memory(ns, memory).await?;
        let _: Option<QueueRecord> = self
            .db
            .delete((QUEUE_TABLE, queue_id.to_string()))
            .await
            .map_err(store_err)?;
        Ok(AcceptedExtraction { memory_id })
    }

    async fn reject_extraction(&self, ns: &NamespaceId, queue_id: &str) -> Result<()> {
        // Scoped delete: only removes if the namespace matches.
        let _ = self
            .db
            .query(format!(
                "DELETE FROM {QUEUE_TABLE}
                 WHERE namespace = $ns AND queue_id = $qid"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("qid", queue_id.to_string()))
            .await
            .map_err(store_err)?;
        Ok(())
    }
    async fn write_diary(&self, ns: &NamespaceId, entry: DiaryEntry) -> Result<()> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let record = DiaryRecord::from_entry(&entry, ns, now_ms);
        let _: Option<DiaryRecord> = self
            .db
            .create(DIARY_TABLE)
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn read_diary(
        &self,
        ns: &NamespaceId,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<DiaryEntry>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT agent, content, topic, timestamp, ts, namespace
                 FROM {DIARY_TABLE}
                 WHERE namespace = $ns AND agent = $agent
                 ORDER BY ts DESC LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("agent", agent.to_string()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let mut records: Vec<DiaryRecord> = result.take(0).map_err(store_err)?;
        records.reverse(); // chronological (DESC → reverse)
        Ok(records.into_iter().map(DiaryRecord::into_entry).collect())
    }

    // ===== Repo directory (global registry — Context Mapper) =====

    async fn register_repo(&self, repo: RepoDirectory) -> Result<()> {
        let name = repo.name.clone();
        let _: Option<RepoDirectory> = self
            .db
            .upsert((REPO_TABLE, name))
            .content(repo)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn list_repos(&self) -> Result<Vec<RepoDirectory>> {
        let mut result = self
            .db
            .query(format!("SELECT * FROM {REPO_TABLE} ORDER BY name"))
            .await
            .map_err(store_err)?;
        let repos: Vec<RepoDirectory> = result.take(0).map_err(store_err)?;
        Ok(repos)
    }

    // ===== Token revocation (WS1b) =====

    async fn revoke_token(&self, revocation: TokenRevocation) -> Result<()> {
        let hash = revocation.token_hash.clone();
        let _: Option<TokenRevocation> = self
            .db
            .upsert((REVOCATIONS_TABLE, hash))
            .content(revocation)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn list_revocations(&self) -> Result<Vec<TokenRevocation>> {
        let mut result = self
            .db
            .query(format!(
                "SELECT * FROM {REVOCATIONS_TABLE} ORDER BY revoked_at_unix"
            ))
            .await
            .map_err(store_err)?;
        let revs: Vec<TokenRevocation> = result.take(0).map_err(store_err)?;
        Ok(revs)
    }
}

// ---------- knowledge graph ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EntityRecord_ {
    name: String,
    entity_type: String,
    namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TripleRecord {
    /// Deterministic id: `<subject>:<predicate>:<object>`.
    triple_id: String,
    subject: String,
    predicate: String,
    object: String,
    #[serde(default)]
    valid_from: Option<String>,
    #[serde(default)]
    valid_to: Option<String>,
    #[serde(default = "default_record_importance")]
    confidence: f32,
    namespace: String,
    #[serde(default)]
    source_memory_id: Option<String>,
}

impl TripleRecord {
    fn into_triple(self) -> Triple {
        Triple {
            id: self.triple_id,
            subject: EntityId(self.subject),
            predicate: self.predicate,
            object: EntityId(self.object),
            valid_from: self.valid_from,
            valid_to: self.valid_to,
            confidence: self.confidence,
            namespace: self.namespace,
            source_memory_id: self.source_memory_id,
        }
    }
}

#[async_trait::async_trait]
impl KnowledgeGraph for SurrealStore {
    async fn add_triple(
        &self,
        ns: &NamespaceId,
        subject: EntityId,
        predicate: &str,
        object: EntityId,
        valid_from: Option<&str>,
        confidence: f32,
        source_memory_id: Option<&str>,
    ) -> Result<Triple> {
        let ns_str = ns.as_str().to_string();
        let subj = subject.0.clone();
        let obj = object.0.clone();
        // Create both entity nodes (idempotent — ignore already-exists).
        for eid in [&subj, &obj] {
            let record = EntityRecord_ {
                name: eid.clone(),
                entity_type: "unknown".into(),
                namespace: ns_str.clone(),
            };
            let _: std::result::Result<Option<EntityRecord_>, _> = self
                .db
                .create((ENTITIES_TABLE, eid.clone()))
                .content(record)
                .await;
            // Ignore errors (record already exists from a prior triple).
        }
        // Store the triple as a record (keyed by deterministic triple_id).
        // Native graph edges (RELATE) are a future optimization for
        // multi-hop traversal; v0 queries via subject/object fields.
        let triple_id = format!("{subj}:{predicate}:{obj}");
        let record = TripleRecord {
            triple_id: triple_id.clone(),
            subject: subj.clone(),
            predicate: predicate.to_string(),
            object: obj.clone(),
            valid_from: valid_from.map(str::to_string),
            valid_to: None,
            confidence,
            namespace: ns_str.clone(),
            source_memory_id: source_memory_id.map(str::to_string),
        };
        let _: Option<TripleRecord> = self
            .db
            .create((TRIPLES_TABLE, triple_id.clone()))
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(Triple {
            id: triple_id,
            subject: EntityId(subj),
            predicate: predicate.to_string(),
            object: EntityId(obj),
            valid_from: valid_from.map(str::to_string),
            valid_to: None,
            confidence,
            namespace: ns_str,
            source_memory_id: source_memory_id.map(str::to_string),
        })
    }

    async fn query_entity(&self, ns: &NamespaceId, entity: &EntityId) -> Result<EntityRecord> {
        let eid = entity.0.clone();
        let ent: Option<EntityRecord_> = self
            .db
            .select((ENTITIES_TABLE, eid.clone()))
            .await
            .map_err(store_err)?;
        let entity_node = ent.and_then(|r| {
            if r.namespace == ns.as_str() {
                Some(Entity {
                    id: entity.clone(),
                    name: r.name,
                    entity_type: r.entity_type,
                    namespace: r.namespace,
                })
            } else {
                None
            }
        });
        let mut res = self
            .db
            .query(format!(
                "SELECT triple_id, subject, predicate, object, valid_from, valid_to, confidence, namespace, source_memory_id
                 FROM {TRIPLES_TABLE}
                 WHERE namespace = $ns AND (subject = $eid OR object = $eid)"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("eid", eid))
            .await
            .map_err(store_err)?;
        let triples: Vec<TripleRecord> = res.take(0).map_err(store_err)?;
        let requested = entity.0.as_str();
        let mut outgoing = Vec::new();
        let mut incoming = Vec::new();
        for t in triples {
            if t.subject == requested {
                outgoing.push(t.into_triple());
            } else {
                incoming.push(t.into_triple());
            }
        }
        Ok(EntityRecord {
            entity: entity_node,
            outgoing,
            incoming,
        })
    }

    async fn invalidate_triple(&self, ns: &NamespaceId, triple_id: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let _ = self
            .db
            .query(format!(
                "UPDATE {TRIPLES_TABLE} SET valid_to = $now
                 WHERE triple_id = $tid AND namespace = $ns"
            ))
            .bind(("now", now))
            .bind(("tid", triple_id.to_string()))
            .bind(("ns", ns.as_str().to_string()))
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn find_triples(
        &self,
        ns: &NamespaceId,
        subject: Option<&EntityId>,
        predicate: Option<&str>,
        object: Option<&EntityId>,
    ) -> Result<Vec<Triple>> {
        let mut conditions = vec!["namespace = $ns".to_string()];
        if subject.is_some() {
            conditions.push("subject = $subj".into());
        }
        if predicate.is_some() {
            conditions.push("predicate = $pred".into());
        }
        if object.is_some() {
            conditions.push("object = $obj".into());
        }
        let where_ = conditions.join(" AND ");
        let mut q = self
            .db
            .query(format!(
                "SELECT triple_id, subject, predicate, object, valid_from, valid_to, confidence, namespace, source_memory_id
                 FROM {TRIPLES_TABLE} WHERE {where_} LIMIT 100"
            ))
            .bind(("ns", ns.as_str().to_string()));
        if let Some(s) = subject {
            q = q.bind(("subj", s.0.clone()));
        }
        if let Some(p) = predicate {
            q = q.bind(("pred", p.to_string()));
        }
        if let Some(o) = object {
            q = q.bind(("obj", o.0.clone()));
        }
        let mut res = q.await.map_err(store_err)?;
        let triples: Vec<TripleRecord> = res.take(0).map_err(store_err)?;
        Ok(triples.into_iter().map(TripleRecord::into_triple).collect())
    }

    async fn kg_timeline(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<Triple>> {
        let mut res = self
            .db
            .query(format!(
                "SELECT triple_id, subject, predicate, object, valid_from, valid_to, confidence, namespace, source_memory_id
                 FROM {TRIPLES_TABLE}
                 WHERE namespace = $ns
                 ORDER BY valid_from DESC LIMIT $lim"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .bind(("lim", limit as i64))
            .await
            .map_err(store_err)?;
        let triples: Vec<TripleRecord> = res.take(0).map_err(store_err)?;
        Ok(triples.into_iter().map(TripleRecord::into_triple).collect())
    }

    async fn knowledge_stats(&self, ns: &NamespaceId) -> Result<KgStats> {
        let mut res = self
            .db
            .query(format!(
                "SELECT namespace FROM {ENTITIES_TABLE} WHERE namespace = $ns;
                 SELECT namespace FROM {TRIPLES_TABLE} WHERE namespace = $ns;"
            ))
            .bind(("ns", ns.as_str().to_string()))
            .await
            .map_err(store_err)?;
        #[derive(Deserialize)]
        struct Row {
            #[allow(dead_code)]
            namespace: String,
        }
        let entities: Vec<Row> = res.take(0).map_err(store_err)?;
        let triples: Vec<Row> = res.take(1).map_err(store_err)?;
        Ok(KgStats {
            entities: entities.len(),
            triples: triples.len(),
        })
    }

    async fn kg_global_stats(&self) -> Result<KgStats> {
        let mut res = self
            .db
            .query(format!(
                "SELECT namespace FROM {ENTITIES_TABLE};
                 SELECT namespace FROM {TRIPLES_TABLE};"
            ))
            .await
            .map_err(store_err)?;
        #[derive(Deserialize)]
        struct Row {
            #[allow(dead_code)]
            namespace: String,
        }
        let entities: Vec<Row> = res.take(0).map_err(store_err)?;
        let triples: Vec<Row> = res.take(1).map_err(store_err)?;
        Ok(KgStats {
            entities: entities.len(),
            triples: triples.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::{NamespaceId, TurnRole};

    async fn fresh() -> SurrealStore {
        SurrealStore::open_embedded()
            .await
            .expect("embedded store must open")
    }

    fn sample_memory(id: &str, content: &str) -> Memory {
        Memory {
            id: MemoryId(id.into()),
            content: content.into(),
            project: "ijima".into(),
            topic: "test".into(),
            source: MemorySource::Explicit,
            harness: Harness::Pi,
            session_id: Some("sess_1".into()),
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.5,
            created_at: "0".into(),
        }
    }

    #[tokio::test]
    async fn store_then_recall_round_trips() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_elliott_private");
        let id = store
            .store_memory(&ns, sample_memory("mem_1", "decided to use surrealdb"))
            .await
            .expect("store");
        assert_eq!(id.0.as_str(), "mem_1");

        let got = store
            .recall_memory(&ns, &MemoryId("mem_1".into()))
            .await
            .expect("recall");
        let got = got.expect("must be present");
        assert_eq!(got.content, "decided to use surrealdb");
        assert_eq!(got.harness, Harness::Pi);
        assert_eq!(got.source, MemorySource::Explicit);
        // Provenance fields persist through a store→recall round-trip
        // (ADR provenance-tier): origin/authority survive SurrealDB.
        assert_eq!(got.origin, InstanceId::local());
        assert_eq!(got.authority, AuthorityScope::local());
    }

    #[tokio::test]
    async fn store_rejects_exact_duplicate_content() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_dedup");
        store
            .store_memory(&ns, sample_memory("m1", "identical content"))
            .await
            .expect("first store");
        // Same content, different id — must be rejected.
        let result = store
            .store_memory(&ns, sample_memory("m2", "identical content"))
            .await;
        assert!(matches!(result, Err(IjimaError::Duplicate { .. })));

        // check_duplicate finds the existing id.
        let dup = store
            .check_duplicate(&ns, "identical content")
            .await
            .expect("check");
        assert_eq!(dup.as_ref().unwrap().0, "m1");

        // Different content stores fine.
        store
            .store_memory(&ns, sample_memory("m3", "different content"))
            .await
            .expect("distinct content stores");

        // Same content in a DIFFERENT namespace is not a duplicate.
        let other = NamespaceId::new("ns_other");
        store
            .store_memory(&other, sample_memory("mx", "identical content"))
            .await
            .expect("same content, different namespace");
    }

    #[tokio::test]
    async fn index_definitions_are_idempotent_and_dedup_still_works() {
        // SurrealDB is schemaless with no auto-indexes; open_with_db defines
        // them. Re-running the DDL (as on every store open against an
        // existing database) must not error — IF NOT EXISTS guards it. This
        // is the fix for the O(N²) full-scan migration hang.
        let store = fresh().await;
        store
            .db
            .query(SurrealStore::INDEX_DDL)
            .await
            .expect("re-defining indexes on an already-indexed store");
        // Dedup is still correct after indexing (namespace + content_hash).
        let ns = NamespaceId::new("ns_idx");
        store
            .store_memory(&ns, sample_memory("m1", "indexed-content"))
            .await
            .expect("store");
        let dup = store
            .check_duplicate(&ns, "indexed-content")
            .await
            .expect("check");
        assert_eq!(dup.as_ref().unwrap().0, "m1");
        // Different namespace, same content: not a duplicate (composite index).
        let other = NamespaceId::new("ns_idx_other");
        store
            .store_memory(&other, sample_memory("m2", "indexed-content"))
            .await
            .expect("same content, different ns");
    }

    #[tokio::test]
    async fn namespace_isolation_hides_other_namespace_memories() {
        let store = fresh().await;
        let alice = NamespaceId::new("ns_alice");
        let bob = NamespaceId::new("ns_bob");

        store
            .store_memory(&alice, sample_memory("mem_a", "alice's secret"))
            .await
            .unwrap();

        let got = store
            .recall_memory(&bob, &MemoryId("mem_a".into()))
            .await
            .expect("recall");
        assert!(got.is_none(), "namespace isolation must hide the memory");
    }

    #[tokio::test]
    async fn delete_removes_owned_memory_only() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_elliott_private");
        store
            .store_memory(&ns, sample_memory("mem_1", "bye"))
            .await
            .unwrap();
        store
            .delete_memory(&ns, &MemoryId("mem_1".into()))
            .await
            .expect("delete");
        let got = store
            .recall_memory(&ns, &MemoryId("mem_1".into()))
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn ingest_then_read_session_turns_chronologically() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_elliott_private");
        let session = SessionId::new("sess_1");
        for (i, content) in ["first", "second", "third"].iter().enumerate() {
            store
                .ingest_turn(
                    &ns,
                    SessionTurn {
                        session_id: session.clone(),
                        turn_index: i as u64,
                        role: if i % 2 == 0 {
                            TurnRole::User
                        } else {
                            TurnRole::Assistant
                        },
                        content: (*content).into(),
                        timestamp: format!("2026-07-05T12:00:0{i}Z"),
                    },
                )
                .await
                .expect("ingest");
        }

        let turns = store
            .session_turns(&ns, &session, 10)
            .await
            .expect("read turns");
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].content, "first");
        assert_eq!(turns[2].content, "third");
    }

    #[tokio::test]
    async fn session_turns_respect_limit_and_namespace() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_elliott_private");
        let other = NamespaceId::new("ns_other");
        let session = SessionId::new("sess_1");

        for i in 0..5 {
            store
                .ingest_turn(
                    &ns,
                    SessionTurn {
                        session_id: session.clone(),
                        turn_index: i,
                        role: TurnRole::User,
                        content: format!("turn {i}"),
                        timestamp: format!("2026-07-05T12:00:0{i}Z"),
                    },
                )
                .await
                .unwrap();
        }
        store
            .ingest_turn(
                &other,
                SessionTurn {
                    session_id: session.clone(),
                    turn_index: 0,
                    role: TurnRole::User,
                    content: "intruder".into(),
                    timestamp: "2026-07-05T12:00:00Z".into(),
                },
            )
            .await
            .unwrap();

        let last_two = store.session_turns(&ns, &session, 2).await.expect("read");
        assert_eq!(last_two.len(), 2);
        assert_eq!(last_two[1].content, "turn 4");
        assert!(last_two.iter().all(|t| t.content != "intruder"));
    }

    #[tokio::test]
    async fn session_metadata_create_list_end_round_trip() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_sessions");

        // Create two sessions (different harnesses).
        let s1 = Session {
            id: SessionId::new("sess_a"),
            harness: Harness::Pi,
            channel: Some("thread-1".into()),
            started_at: "2026-07-05T10:00:00Z".into(),
            ended_at: None,
        };
        let s2 = Session {
            id: SessionId::new("sess_b"),
            harness: Harness::Sakamoto,
            channel: None,
            started_at: "2026-07-05T12:00:00Z".into(),
            ended_at: None,
        };
        store
            .create_session(&ns, s1.clone())
            .await
            .expect("create s1");
        store
            .create_session(&ns, s2.clone())
            .await
            .expect("create s2");

        // List all — newest first (s2 has later started_at).
        let all = store.list_sessions(&ns, None, 10).await.expect("list");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id.as_str(), "sess_b");
        assert_eq!(all[1].id.as_str(), "sess_a");
        assert!(all[0].ended_at.is_none());

        // Filter by harness.
        let pi_only = store
            .list_sessions(&ns, Some(&Harness::Pi), 10)
            .await
            .expect("list pi");
        assert_eq!(pi_only.len(), 1);
        assert_eq!(pi_only[0].id.as_str(), "sess_a");

        // End s1.
        store
            .end_session(
                &ns,
                &SessionId::new("sess_a"),
                "2026-07-05T11:30:00Z".into(),
            )
            .await
            .expect("end");
        let after_end = store.list_sessions(&ns, None, 10).await.expect("list");
        let s1_row = after_end
            .iter()
            .find(|s| s.id.as_str() == "sess_a")
            .unwrap();
        assert_eq!(s1_row.ended_at.as_deref(), Some("2026-07-05T11:30:00Z"));

        // Upsert (re-create s1) preserves metadata shape.
        store.create_session(&ns, s1.clone()).await.expect("upsert");
        let upserted = store
            .list_sessions(&ns, Some(&Harness::Pi), 10)
            .await
            .expect("list");
        assert_eq!(upserted.len(), 1);

        // Namespace isolation: sessions in another namespace are invisible.
        let other = NamespaceId::new("ns_other");
        let cross = store
            .list_sessions(&other, None, 10)
            .await
            .expect("list other");
        assert!(cross.is_empty());
    }

    // ===== Diaries =====

    #[tokio::test]
    async fn diaries_write_appends_and_reads_chronologically() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_diaries");

        let e1 = DiaryEntry {
            agent: "claude".into(),
            content: "first entry".into(),
            topic: None,
            timestamp: String::new(),
        };
        let e2 = DiaryEntry {
            agent: "claude".into(),
            content: "second entry".into(),
            topic: Some("reflection".into()),
            timestamp: "2026-07-05T12:00:00Z".into(),
        };
        store.write_diary(&ns, e1).await.expect("write1");
        store.write_diary(&ns, e2).await.expect("write2");

        let entries = store.read_diary(&ns, "claude", 10).await.expect("read");
        assert_eq!(entries.len(), 2);
        // chronological: e1 before e2
        assert_eq!(entries[0].content, "first entry");
        assert_eq!(entries[1].content, "second entry");
        assert_eq!(entries[1].topic.as_deref(), Some("reflection"));

        // Agent isolation: different agent sees nothing.
        let other = store.read_diary(&ns, "pi", 10).await.expect("read");
        assert!(other.is_empty());

        // Namespace isolation: different ns sees nothing.
        let other_ns = NamespaceId::new("ns_other");
        let cross = store
            .read_diary(&other_ns, "claude", 10)
            .await
            .expect("read");
        assert!(cross.is_empty());
    }

    // ===== Vector search =====

    /// A deterministic test embedder: maps text to a small fixed-dim
    /// vector so nearest-neighbour behaviour is predictable. Production
    /// uses candle + all-MiniLM-L6-v2 (384-dim).
    struct TestEmbedder;
    impl Embedder for TestEmbedder {
        fn dim(&self) -> usize {
            4
        }
        fn embed(&self, text: &str) -> Result<Embedding> {
            // Cosine-relevant signal: word-count + keyword flags.
            let words = text.split_whitespace().count() as f32;
            let has_surreal = text.contains("surreal") as u32 as f32;
            let has_rust = text.contains("rust") as u32 as f32;
            let has_memory = text.contains("memory") as u32 as f32;
            Ok(Embedding(vec![words, has_surreal, has_rust, has_memory]))
        }
    }

    #[tokio::test]
    async fn search_without_embedder_errors() {
        let store = fresh().await; // no embedder
        let ns = NamespaceId::new("ns_x");
        let result = store
            .search_memories(&ns, &Embedding(vec![0.0; 4]), 5)
            .await;
        assert!(matches!(result, Err(IjimaError::Store { .. })));
    }

    #[tokio::test]
    async fn search_finds_nearest_with_embedder() {
        let store = SurrealStore::open_embedded_with(Arc::new(TestEmbedder))
            .await
            .expect("open with embedder");
        let ns = NamespaceId::new("ns_elliott_private");

        // Three memories with distinct keyword signals.
        store
            .store_memory(&ns, sample_memory("m1", "rust memory store"))
            .await
            .unwrap();
        store
            .store_memory(&ns, sample_memory("m2", "surreal db graph"))
            .await
            .unwrap();
        store
            .store_memory(&ns, sample_memory("m3", "completely unrelated text"))
            .await
            .unwrap();

        // Query close to the "rust memory" vector.
        let query = Embedding(vec![3.0, 0.0, 1.0, 1.0]);
        let hits = store.search_memories(&ns, &query, 2).await.expect("search");
        assert!(!hits.is_empty(), "must find at least one memory");
        // The nearest hit must be the rust/memory one (m1), not the
        // unrelated text.
        assert_eq!(hits[0].memory.content, "rust memory store");
        // Scored search: each hit carries its cosine similarity.
        assert!(hits[0].similarity > 0.0);
    }

    #[tokio::test]
    async fn search_respects_namespace_isolation() {
        let store = SurrealStore::open_embedded_with(Arc::new(TestEmbedder))
            .await
            .expect("open with embedder");
        let alice = NamespaceId::new("ns_alice");
        let bob = NamespaceId::new("ns_bob");

        store
            .store_memory(&alice, sample_memory("a1", "rust memory store"))
            .await
            .unwrap();

        // Bob searches with an identical query embedding — must see
        // nothing from alice's namespace.
        let query = Embedding(vec![3.0, 0.0, 1.0, 1.0]);
        let hits = store
            .search_memories(&bob, &query, 5)
            .await
            .expect("search");
        assert!(
            hits.is_empty(),
            "namespace isolation must hide alice's memory"
        );
    }

    #[tokio::test]
    async fn list_memories_ranks_by_importance_then_recency() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_test");
        // Three memories with varying importance + recency.
        let mut hi = sample_memory("hi", "important");
        hi.importance = 0.9;
        hi.created_at = "100".into();
        let mut mid = sample_memory("mid", "medium");
        mid.importance = 0.5;
        mid.created_at = "200".into(); // newer but lower importance
        let mut lo = sample_memory("lo", "low");
        lo.importance = 0.9;
        lo.created_at = "300".into(); // same importance as hi, newer
        store.store_memory(&ns, hi).await.unwrap();
        store.store_memory(&ns, mid).await.unwrap();
        store.store_memory(&ns, lo).await.unwrap();

        let list = store.list_memories(&ns, 10).await.expect("list");
        // importance DESC first: the two 0.9s before the 0.5.
        // Among the 0.9s, created_at DESC: lo (300) before hi (100).
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].id.0, "lo"); // 0.9, 300
        assert_eq!(list[1].id.0, "hi"); // 0.9, 100
        assert_eq!(list[2].id.0, "mid"); // 0.5
    }

    // ===== knowledge graph =====

    #[tokio::test]
    async fn add_triple_then_query_entity() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_kg");
        let t = store
            .add_triple(
                &ns,
                EntityId::new("Ijima"),
                "depends_on",
                EntityId::new("SurrealDB"),
                Some("100"),
                1.0,
                None,
            )
            .await
            .expect("add_triple");
        assert_eq!(t.subject.as_str(), "Ijima");
        assert_eq!(t.predicate, "depends_on");
        assert_eq!(t.object.as_str(), "SurrealDB");
        assert!(t.valid_to.is_none()); // current

        let rec = store
            .query_entity(&ns, &EntityId::new("Ijima"))
            .await
            .expect("query");
        assert!(rec.entity.is_some());
        assert_eq!(rec.outgoing.len(), 1);
        assert_eq!(rec.outgoing[0].object.as_str(), "SurrealDB");
        assert!(rec.incoming.is_empty());

        // From the object's side, it's incoming.
        let rec = store
            .query_entity(&ns, &EntityId::new("SurrealDB"))
            .await
            .expect("query");
        assert_eq!(rec.incoming.len(), 1);
        assert!(rec.outgoing.is_empty());
    }

    #[tokio::test]
    async fn invalidate_triple_sets_valid_to() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_kg");
        store
            .add_triple(
                &ns,
                EntityId::new("a"),
                "uses",
                EntityId::new("b"),
                Some("100"),
                1.0,
                None,
            )
            .await
            .unwrap();
        store
            .invalidate_triple(&ns, "a:uses:b")
            .await
            .expect("invalidate");
        let found = store
            .find_triples(&ns, None, Some("uses"), None)
            .await
            .expect("find");
        assert_eq!(found.len(), 1);
        assert!(found[0].valid_to.is_some(), "valid_to must be set");
    }

    #[tokio::test]
    async fn knowledge_stats_counts_entities_and_triples() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_kg");
        store
            .add_triple(
                &ns,
                EntityId::new("a"),
                "x",
                EntityId::new("b"),
                None,
                1.0,
                None,
            )
            .await
            .unwrap();
        store
            .add_triple(
                &ns,
                EntityId::new("a"),
                "y",
                EntityId::new("c"),
                None,
                1.0,
                None,
            )
            .await
            .unwrap();
        let stats = store.knowledge_stats(&ns).await.expect("stats");
        assert_eq!(stats.entities, 3); // a, b, c
        assert_eq!(stats.triples, 2);
    }

    #[tokio::test]
    async fn kg_namespace_isolation() {
        let store = fresh().await;
        let a = NamespaceId::new("ns_a");
        let b = NamespaceId::new("ns_b");
        store
            .add_triple(
                &a,
                EntityId::new("x"),
                "uses",
                EntityId::new("y"),
                None,
                1.0,
                None,
            )
            .await
            .unwrap();
        // b sees nothing.
        let stats = store.knowledge_stats(&b).await.expect("stats");
        assert_eq!(stats.triples, 0);
        let rec = store
            .query_entity(&b, &EntityId::new("x"))
            .await
            .expect("query");
        assert!(rec.outgoing.is_empty());
    }

    #[tokio::test]
    async fn store_stats_counts_across_namespaces() {
        let store = fresh().await;
        store
            .store_memory(&NamespaceId::new("ns_a"), sample_memory("m1", "x"))
            .await
            .unwrap();
        store
            .store_memory(&NamespaceId::new("ns_a"), sample_memory("m2", "y"))
            .await
            .unwrap();
        store
            .store_memory(&NamespaceId::new("ns_b"), sample_memory("m3", "z"))
            .await
            .unwrap();
        let stats = store.store_stats().await.expect("stats");
        assert_eq!(stats.total_memories, 3);
        assert_eq!(stats.namespaces.len(), 2);
        let ns_a = stats
            .namespaces
            .iter()
            .find(|n| n.namespace == "ns_a")
            .expect("ns_a");
        assert_eq!(ns_a.memories, 2);
    }

    /// Builds a memory with explicit project/topic (sample_memory hardcodes them).
    fn mem_in(id: &str, project: &str, topic: &str, content: &str) -> Memory {
        Memory {
            id: MemoryId(id.into()),
            content: content.into(),
            project: project.into(),
            topic: topic.into(),
            source: MemorySource::Explicit,
            harness: Harness::Pi,
            session_id: None,
            origin: InstanceId::local(),
            authority: AuthorityScope::local(),
            importance: 0.5,
            created_at: "0".into(),
        }
    }

    #[tokio::test]
    async fn palace_organization_rooms_taxonomy_graph_tunnel() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_palace");
        // Two projects sharing the topic "auth" (a tunnel), plus a
        // project-only topic.
        store
            .store_memory(&ns, mem_in("m1", "ijima", "auth", "use schubert"))
            .await
            .unwrap();
        store
            .store_memory(&ns, mem_in("m2", "ijima", "auth", "proof tokens"))
            .await
            .unwrap();
        store
            .store_memory(&ns, mem_in("m3", "ijima", "store", "surrealdb"))
            .await
            .unwrap();
        store
            .store_memory(&ns, mem_in("m4", "karpal", "auth", "workspace manifest"))
            .await
            .unwrap();
        store
            .store_memory(&ns, mem_in("m5", "karpal", "build", "ci"))
            .await
            .unwrap();

        // list_rooms — all rooms, count desc.
        let rooms = store.list_rooms(&ns, None, 100).await.expect("rooms");
        assert_eq!(rooms.len(), 4);
        assert_eq!(rooms[0].count, 2); // ijima/auth
        assert_eq!(rooms[0].project, "ijima");
        assert_eq!(rooms[0].topic, "auth");

        // list_rooms filtered to karpal.
        let karpal_rooms = store
            .list_rooms(&ns, Some("karpal"), 100)
            .await
            .expect("rooms karpal");
        assert_eq!(karpal_rooms.len(), 2);
        assert!(karpal_rooms.iter().all(|r| r.project == "karpal"));

        // taxonomy — two projects.
        let taxons = store.taxonomy(&ns).await.expect("taxonomy");
        assert_eq!(taxons.len(), 2);
        let ijima_t = taxons
            .iter()
            .find(|t| t.project == "ijima")
            .expect("ijima taxon");
        assert_eq!(ijima_t.total, 3);
        assert_eq!(ijima_t.rooms.len(), 2); // auth, store

        // palace_graph — projects + the auth tunnel between ijima & karpal.
        let graph = store.palace_graph(&ns).await.expect("graph");
        assert_eq!(graph.projects.len(), 2);
        assert!(graph.projects.contains(&"ijima".to_string()));
        assert!(graph.projects.contains(&"karpal".to_string()));
        let auth_tunnel = graph
            .tunnels
            .iter()
            .find(|t| t.topic == "auth")
            .expect("auth tunnel");
        assert_eq!(auth_tunnel.count_a, 2); // ijima
        assert_eq!(auth_tunnel.count_b, 1); // karpal

        // traverse_tunnel — memories from both sides.
        let trav = store
            .traverse_tunnel(&ns, "auth", "ijima", "karpal", 10)
            .await
            .expect("traverse");
        assert_eq!(trav.memories_a.len(), 2);
        assert_eq!(trav.memories_b.len(), 1);
        assert!(trav.memories_a.iter().all(|m| m.project == "ijima"));
        assert!(trav.memories_b.iter().all(|m| m.project == "karpal"));
    }

    #[tokio::test]
    async fn mining_queue_enqueue_list_accept_reject() {
        let store = fresh().await;
        let ns = NamespaceId::new("ns_mining");
        let other = NamespaceId::new("ns_other");

        // Enqueue two PendingReview extractions.
        let q1 = store
            .enqueue_extraction(&ns, sample_memory("m1", "decided to use surrealdb"), 0.6)
            .await
            .expect("enqueue1");
        let q2 = store
            .enqueue_extraction(&ns, sample_memory("m2", "see https://example.com"), 0.55)
            .await
            .expect("enqueue2");

        // list_pending sees both (newest first).
        let pending = store.list_pending(&ns, 10).await.expect("list");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, q2); // newest first
        assert_eq!(pending[1].confidence, 0.6);

        // Namespace isolation: other ns sees nothing.
        let cross = store.list_pending(&other, 10).await.expect("list other");
        assert!(cross.is_empty());

        // Accept q1 → promotes to palace + removes from queue.
        let accepted = store.accept_extraction(&ns, &q1).await.expect("accept");
        assert_eq!(accepted.memory_id.0, "m1");
        let after_accept = store.list_pending(&ns, 10).await.expect("list");
        assert_eq!(after_accept.len(), 1);
        // The promoted memory is now in the palace.
        let promoted = store
            .recall_memory(&ns, &MemoryId("m1".into()))
            .await
            .expect("recall");
        assert!(promoted.is_some());

        // Reject q2 → drops without promoting.
        store.reject_extraction(&ns, &q2).await.expect("reject");
        let after_reject = store.list_pending(&ns, 10).await.expect("list");
        assert!(after_reject.is_empty());
        let not_promoted = store
            .recall_memory(&ns, &MemoryId("m2".into()))
            .await
            .expect("recall");
        assert!(not_promoted.is_none());

        // Cross-namespace accept fails (queue entry is in ns, not other).
        let q3 = store
            .enqueue_extraction(&ns, sample_memory("m3", "cross test"), 0.5)
            .await
            .expect("enqueue3");
        let cross_accept = store.accept_extraction(&other, &q3).await;
        assert!(cross_accept.is_err());
    }

    #[tokio::test]
    async fn persistent_store_survives_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "ijima-persist-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ns = NamespaceId::new("ns_persist");

        // Write with one instance.
        {
            let store = SurrealStore::open_persistent(&dir).await.expect("open");
            store
                .store_memory(&ns, sample_memory("mem_p", "survives restart"))
                .await
                .expect("store");
        }

        // A fresh instance pointing at the same path must see the memory.
        let store = SurrealStore::open_persistent(&dir).await.expect("reopen");
        let got = store
            .recall_memory(&ns, &MemoryId("mem_p".into()))
            .await
            .expect("recall")
            .expect("memory must survive reopen");
        assert_eq!(got.content, "survives restart");
    }

    #[tokio::test]
    async fn export_to_writes_sql_dump_to_file() {
        let dir = std::env::temp_dir().join(format!(
            "ijima-export-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("create dir");
        let db_path = dir.join("ijima.db");
        let store = crate::SurrealStore::open_persistent(&db_path)
            .await
            .expect("open");
        let ns = NamespaceId::new("ns_export");
        store
            .store_memory(&ns, sample_memory("mem_x", "export this memory"))
            .await
            .expect("store");

        let out = dir.join("dump.surql");
        store.export_to(&out).await.expect("export");
        let contents = std::fs::read_to_string(&out).expect("read");
        assert!(contents.contains("export this memory"));
        // Clean up.
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// The revocation round-trip is covered full-stack in api::tests
// (token_revocation_kills_the_bearer_immediately); here we pin the store
// contract directly: upsert idempotence + oldest-first ordering.
#[tokio::test]
async fn revocation_upsert_and_ordering() {
    use ijima_core::TokenRevocation;
    let store = SurrealStore::open_embedded().await.expect("open");
    let mk = |hash: &str, at: u64| TokenRevocation {
        token_hash: hash.to_string(),
        revoked_at_unix: at,
        reason: None,
    };
    store.revoke_token(mk("bbb", 20)).await.expect("revoke b");
    store.revoke_token(mk("aaa", 10)).await.expect("revoke a");
    // Idempotent re-upsert of the same hash.
    store
        .revoke_token(mk("bbb", 20))
        .await
        .expect("re-revoke b");
    let revs = store.list_revocations().await.expect("list");
    assert_eq!(revs.len(), 2, "upsert keeps one row per hash");
    assert_eq!(revs[0].token_hash, "aaa", "oldest first");
    assert_eq!(revs[1].token_hash, "bbb");
}
