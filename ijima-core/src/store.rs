// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The storage contract every backend implements.
//!
//! A pure, backend-free trait (mirrors the [`crate::Embedder`] pattern):
//! the memory palace, the session-context repository, and (in future)
//! the knowledge graph all flow through [`Store`]. Concrete backends
//! live in `ijima-server` behind additive features — `backend-surreal`
//! (primary), `backend-sqlite` (migration-only), `backend-postgres`
//! (future).
//!
//! Every method is scoped to a [`crate::NamespaceId`] so that isolation
//! is enforced at the type level, not bolted on by callers.

use async_trait::async_trait;

use crate::{Embedding, Memory, MemoryId, NamespaceId, Result, SessionId, SessionTurn};

/// The storage contract.
///
/// Implementations must be `Send + Sync` (shared across an axum handler
/// pool). Async via [`async_trait`], matching the DB backends' async I/O.
#[async_trait]
pub trait Store: Send + Sync {
    // ===== Memory palace =====

    /// Stores a curated memory under `ns`, performing content-hash +
    /// semantic dedup. Returns the stored id.
    async fn store_memory(&self, ns: &NamespaceId, memory: Memory) -> Result<MemoryId>;

    /// Recalls a single memory by id within `ns`. Returns `None` if the
    /// id is absent or belongs to a different namespace (isolation).
    async fn recall_memory(&self, ns: &NamespaceId, id: &MemoryId) -> Result<Option<Memory>>;

    /// Deletes a memory by id within `ns`.
    async fn delete_memory(&self, ns: &NamespaceId, id: &MemoryId) -> Result<()>;

    /// Semantic search over memories in `ns` by nearest embedding.
    ///
    /// Implementations back this with a vector index (SurrealDB MTREE,
    /// pgvector). v0 backends MAY return [`crate::IjimaError::Store`] until
    /// the vector index is wired.
    async fn search_memories(
        &self,
        ns: &NamespaceId,
        embedding: &Embedding,
        limit: usize,
    ) -> Result<Vec<Memory>>;

    // ===== Session-context repository =====

    /// Appends a raw turn to the session transcript under `ns`.
    async fn ingest_turn(&self, ns: &NamespaceId, turn: SessionTurn) -> Result<()>;

    /// Returns the last `limit` turns of `session` under `ns`, in order.
    async fn session_turns(
        &self,
        ns: &NamespaceId,
        session: &SessionId,
        limit: usize,
    ) -> Result<Vec<SessionTurn>>;
}
