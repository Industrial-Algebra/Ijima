// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! The storage contract — the abstraction boundary between Ijima's domain
//! logic and the SurrealDB / SQLite / mock backends.
//!
//! Every backend implements [`Store`] (memory palace + session context) and
//! (in future) [`KnowledgeGraph`](crate::knowledge::KnowledgeGraph).

use async_trait::async_trait;

use crate::{
    AcceptedExtraction, DiaryEntry, Embedding, Memory, MemoryId, NamespaceId, QueuedExtraction,
    RepoDirectory, Result, Session, SessionId, SessionTurn, TokenRevocation,
    harness::Harness,
    namespace::NamespaceMembership,
    palace::{PalaceGraph, ProjectTaxon, Room, TunnelTraversal},
};

/// A scored semantic-search hit: the matched [`Memory`] plus its cosine
/// similarity to the query (0.0–1.0, higher = more relevant).
///
/// Required both for cross-namespace result merging (the pi integration's
/// `scope=visible` search) and for downstream "% match" display.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchHit {
    /// The matched memory.
    pub memory: Memory,
    /// Cosine similarity to the query (0.0–1.0).
    pub similarity: f32,
}
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

    /// Lists memories in `ns`, optionally filtered to `project`/`topic`.
    /// Powers `GET /memories` (the `memory_recall` browse path — distinct
    /// from [`Self::list_memories`], which is the importance-ranked
    /// wake-up feed). Default: fetch a cap, filter in Rust, truncate.
    /// Backends MAY override with a native filtered query.
    async fn list_memories_filtered(
        &self,
        ns: &NamespaceId,
        project: Option<&str>,
        topic: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let cap = if project.is_some() || topic.is_some() {
            500
        } else {
            limit
        };
        let mut mems = self.list_memories(ns, cap).await?;
        if let Some(p) = project {
            mems.retain(|m| m.project == p);
        }
        if let Some(t) = topic {
            mems.retain(|m| m.topic == t);
        }
        mems.truncate(limit);
        Ok(mems)
    }

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
    ) -> Result<Vec<SearchHit>>;

    // ===== Palace organization (Phase 3.1 + 3.2) =====

    /// Lists rooms (topic cells) in `ns`, optionally filtered to a single
    /// project. Each room carries its memory count. Ordered by count desc.
    async fn list_rooms(
        &self,
        ns: &NamespaceId,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Room>>;

    /// Full project → topic → count taxonomy of `ns`. Powers
    /// `getTaxonomy` navigation.
    async fn taxonomy(&self, ns: &NamespaceId) -> Result<Vec<ProjectTaxon>>;

    /// The palace graph: projects as nodes, shared-topic tunnels as edges.
    /// Powers `getPalaceGraph` — *"what connects these projects?"*
    async fn palace_graph(&self, ns: &NamespaceId) -> Result<PalaceGraph>;

    /// Traverses a tunnel: returns the actual memories from both projects on
    /// the shared `topic`, so the caller can see what connects them.
    async fn traverse_tunnel(
        &self,
        ns: &NamespaceId,
        topic: &str,
        project_a: &str,
        project_b: &str,
        limit: usize,
    ) -> Result<TunnelTraversal>;

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

    // ===== Diaries (Phase 3.3) =====

    /// Appends a diary entry under `ns`.
    async fn write_diary(&self, ns: &NamespaceId, entry: DiaryEntry) -> Result<()>;

    /// Returns the last `limit` entries of `agent`'s diary under `ns`, in
    /// chronological order.
    async fn read_diary(
        &self,
        ns: &NamespaceId,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<DiaryEntry>>;

    // ===== Repo directory (global — Context Mapper) =====

    /// Registers or upserts a repository in the global registry (keyed by
    /// `name`). Powers `POST /repos` — the canonical Anima roster.
    async fn register_repo(&self, repo: RepoDirectory) -> Result<()>;

    /// Lists every registered repository (the ecosystem roster).
    async fn list_repos(&self) -> Result<Vec<RepoDirectory>>;
    /// Reverse-resolves a working directory to its registered repo: the
    /// most specific repo whose `path` is a prefix of `cwd` (after
    /// normalizing). Powers `GET /repos/resolve` (CWD → project).
    async fn resolve_repo(&self, cwd: &str) -> Result<Option<RepoDirectory>> {
        let target = crate::repo::normalize_path(cwd);
        let repos = self.list_repos().await?;
        Ok(repos
            .into_iter()
            .filter(|r| target == r.path || target.starts_with(&format!("{}/", r.path)))
            .max_by_key(|r| r.path.len()))
    }

    // ===== Token revocation (WS1b — the grant kill-switch) =====

    /// Records a token revocation (idempotent upsert keyed by hash).
    /// Powers `POST /tokens/revoke` (admin).
    async fn revoke_token(&self, revocation: TokenRevocation) -> Result<()>;

    /// Lists every recorded revocation, oldest first. Powers
    /// `GET /tokens/revocations` (admin) and daemon-boot hydration of the
    /// in-memory rejection set.
    async fn list_revocations(&self) -> Result<Vec<TokenRevocation>>;

    // ===== Shared-namespace membership (WS3 org walls) =====

    /// Grants (upserts) a principal's membership in a shared namespace.
    /// Admin operation; idempotent — re-granting refreshes `granted_at`/
    /// `granted_by` but never duplicates.
    async fn grant_namespace_membership(&self, membership: NamespaceMembership) -> Result<()>;

    /// Revokes a membership. Idempotent: revoking an absent membership is
    /// not an error.
    async fn revoke_namespace_membership(&self, ns: &NamespaceId, principal: &str) -> Result<()>;

    /// Lists the members of a namespace, oldest grant first. Powers
    /// `GET /namespaces/members` (admin).
    async fn list_namespace_members(&self, ns: &NamespaceId) -> Result<Vec<NamespaceMembership>>;

    /// Hot-path membership check behind `resolve_ns` (shared-namespace
    /// reads/writes).
    async fn is_namespace_member(&self, ns: &NamespaceId, principal: &str) -> Result<bool>;

    /// Every namespace `principal` holds membership in (their org walls).
    /// Backs `scope=visible` search: the union of private + global + these
    /// + open `ns_import_*` staging is the principal's readable world.
    async fn list_namespaces_for_principal(&self, principal: &str) -> Result<Vec<NamespaceId>>;
    // ===== Mining review queue (ADR M2, M3) =====

    /// Stages a PendingReview extraction in the per-namespace queue.
    async fn enqueue_extraction(
        &self,
        ns: &NamespaceId,
        memory: Memory,
        confidence: f32,
    ) -> Result<String>;

    /// Lists pending extractions in `ns`, newest first.
    async fn list_pending(&self, ns: &NamespaceId, limit: usize) -> Result<Vec<QueuedExtraction>>;

    /// Accepts a queued extraction: promotes it to the palace and removes
    /// it from the queue.
    async fn accept_extraction(
        &self,
        ns: &NamespaceId,
        queue_id: &str,
    ) -> Result<AcceptedExtraction>;

    /// Rejects a queued extraction: drops it from the queue without promoting.
    async fn reject_extraction(&self, ns: &NamespaceId, queue_id: &str) -> Result<()>;
}
