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
    Embedding, IjimaError, Memory, MemoryId, NamespaceId, Result, SessionId, SessionTurn, Store,
    embeddings::Embedder, harness::Harness, memory::MemorySource,
};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};

/// The SurrealDB namespace/database the Ijima instance lives in.
const SURREAL_NS: &str = "ijima";
const SURREAL_DB: &str = "core";

const MEMORIES_TABLE: &str = "memories";
const TURNS_TABLE: &str = "session_turns";

/// A SurrealDB-backed [`Store`].
pub struct SurrealStore {
    db: Surreal<Db>,
    /// When present, memories are embedded at write time and
    /// [`Store::search_memories`] is available. When absent, search
    /// returns [`IjimaError::Store`].
    embedder: Option<Arc<dyn Embedder>>,
}

impl SurrealStore {
    /// Opens an in-memory embedded store, scoped to the Ijima NS/DB,
    /// with **no embedder**. Memories store without embeddings and
    /// [`Store::search_memories`] errors.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize.
    pub async fn open_embedded() -> Result<Self> {
        Self::open(None).await
    }

    /// Opens an in-memory embedded store with an [`Embedder`]. Memories
    /// are embedded at write time, enabling
    /// [`Store::search_memories`] via Cosine ranking.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Store`] if SurrealDB cannot initialize.
    pub async fn open_embedded_with(embedder: Arc<dyn Embedder>) -> Result<Self> {
        Self::open(Some(embedder)).await
    }

    async fn open(embedder: Option<Arc<dyn Embedder>>) -> Result<Self> {
        let db = Surreal::new::<Mem>(())
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("surrealdb init: {e}"),
            })?;
        db.use_ns(SURREAL_NS)
            .use_db(SURREAL_DB)
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("surrealdb use_ns/use_db: {e}"),
            })?;

        Ok(Self { db, embedder })
    }
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
    fn from_memory(memory: &Memory, ns: &NamespaceId, embedding: Option<Vec<f32>>) -> Self {
        Self {
            memory_id: memory.id.0.clone(),
            content: memory.content.clone(),
            project: memory.project.clone(),
            topic: memory.topic.clone(),
            source: memory.source,
            harness: memory.harness,
            session_id: memory.session_id.clone(),
            namespace: ns.as_str().to_string(),
            importance: memory.importance,
            created_at: memory.created_at.clone(),
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

fn store_err(e: surrealdb::Error) -> IjimaError {
    IjimaError::Store {
        detail: e.to_string(),
    }
}

fn embed_for(embedder: &dyn Embedder, text: &str) -> Result<Option<Vec<f32>>> {
    Ok(Some(embedder.embed(text)?.0))
}

#[async_trait]
impl Store for SurrealStore {
    async fn store_memory(&self, ns: &NamespaceId, memory: Memory) -> Result<MemoryId> {
        let embedding = match &self.embedder {
            Some(e) => embed_for(e.as_ref(), &memory.content)?,
            None => None,
        };
        let id_str = memory.id.0.clone();
        let record = MemoryRecord::from_memory(&memory, ns, embedding);
        let _: Option<MemoryRecord> = self
            .db
            .create((MEMORIES_TABLE, id_str.clone()))
            .content(record)
            .await
            .map_err(store_err)?;
        Ok(memory.id)
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

    async fn search_memories(
        &self,
        ns: &NamespaceId,
        embedding: &Embedding,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if self.embedder.is_none() {
            return Err(IjimaError::Store {
                detail: "search requires the store to be opened with an Embedder".into(),
            });
        }
        // Cosine similarity ranking, brute-force over the namespace.
        // Correct (no ANN approximation); HNSW is the planned
        // optimization once the SDK emits typed `<N>f32` vectors.
        let mut result = self
            .db
            .query(format!(
                "SELECT memory_id, content, project, topic, source, harness, session_id, namespace,
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
        let records: Vec<MemoryRecord> = result.take(0).map_err(store_err)?;
        Ok(records.into_iter().map(|r| r.into_memory()).collect())
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
        assert_eq!(hits[0].content, "rust memory store");
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
}
