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

use crate::{
    Embedding, Memory, MemoryId, NamespaceId, RepoDirectory, Result, Session, SessionId,
    SessionTurn, harness::Harness,
};

/// Global memory-palace statistics (across all namespaces).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StoreStats {
    /// Total memories across every namespace.
    pub total_memories: usize,
    /// Per-namespace breakdown.
    pub namespaces: Vec<NamespaceCount>,
}

/// One namespace's memory count.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NamespaceCount {
    /// The namespace id.
    pub namespace: String,
    /// Memories in that namespace.
    pub memories: usize,
}

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

    /// Lists up to `limit` memories in `ns`, ranked by importance DESC
    /// then recency DESC. Powers wake-up composition (L1a personal
    /// essentials, L1b doctrine baseline).
    async fn list_memories(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<Memory>>;

    /// Global store statistics across all namespaces (operator/admin
    /// view). Powers `GET /status`.
    async fn store_stats(&self) -> Result<StoreStats>;

    /// Checks whether a memory with identical content already exists in
    /// `ns` (content-hash dedup). Returns the existing [`MemoryId`] if so.
    async fn check_duplicate(&self, ns: &NamespaceId, content: &str) -> Result<Option<MemoryId>>;

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

    /// Creates or updates a session's metadata under `ns` (upsert by
    /// id). Call when a session starts; turns reference the session id.
    /// `ended_at` is set via [`Self::end_session`].
    async fn create_session(&self, ns: &NamespaceId, session: Session) -> Result<SessionId>;

    /// Lists up to `limit` sessions in `ns`, newest first, optionally
    /// filtered by `harness`.
    async fn list_sessions(
        &self,
        ns: &NamespaceId,
        harness: Option<&Harness>,
        limit: usize,
    ) -> Result<Vec<Session>>;

    /// Marks a session as ended (sets `ended_at`). Scoped by `ns` so a
    /// principal can only end sessions in their own namespace.
    async fn end_session(
        &self,
        ns: &NamespaceId,
        session: &SessionId,
        ended_at: String,
    ) -> Result<()>;

    // ===== Context Mapper (global repo directory) =====

    /// Registers or updates a repo in the global registry (upsert by name).
    async fn register_repo(&self, repo: RepoDirectory) -> Result<()>;

    /// Forward lookup by canonical name.
    async fn lookup_repo(&self, name: &str) -> Result<Option<RepoDirectory>>;

    /// Lists all registered repos (the canonical ecosystem roster).
    async fn list_repos(&self) -> Result<Vec<RepoDirectory>>;

    /// Reverse lookup: resolves a filesystem path to its repo via
    /// longest-prefix match (so a CWD *inside* a repo resolves to it).
    async fn resolve_path(&self, path: &str) -> Result<Option<RepoDirectory>>;
}
