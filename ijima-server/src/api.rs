// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! HTTP/JSON API surface — the REST endpoints harnesses speak.
//!
//! Maps the core [`Store`] trait methods onto axum routes, each guarded
//! by a Schubert capability check via [`AuthPrincipal`]. Every request
//! is scoped to the authenticated principal's personal namespace
//! (`ns_<principal>_private`); shared/global namespaces land with the
//! `memory_promote` endpoint.
//!
//! ## Routes
//!
//! | Method | Path | Capability | Store method |
//! |---|---|---|---|
//! | GET | `/health` | (none) | — |
//! | POST | `/memories` | `memory:write` | `store_memory` |
//! | GET | `/memories/:id` | `memory:read` | `recall_memory` |
//! | DELETE | `/memories/:id` | `memory:write` | `delete_memory` |
//! | POST | `/memories/search` | `memory:read` | `search_memories` |
//! | POST | `/sessions/:session_id/turns` | `session:ingest` | `ingest_turn` |
//! | GET | `/sessions/:session_id/turns` | `memory:read` | `session_turns` |
//! | POST | `/sessions` | `session:ingest` | `create_session` |
//! | GET | `/sessions` | `memory:read` | `list_sessions` |
//! | POST | `/sessions/:session_id/end` | `session:ingest` | `end_session` |
//! | GET | `/mining/queue` | `mining:review` | `list_pending` |
//! | POST | `/mining/queue/:id/accept` | `mining:review` | `accept_extraction` |
//! | POST | `/mining/queue/:id/reject` | `mining:review` | `reject_extraction` |
//! | POST | `/sessions/:session_id/mine` | `mining:trigger` | `trigger_mine` (feature `mining`) |

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "mining")]
use ijima_core::capabilities::MINING_TRIGGER;
use ijima_core::{
    AcceptedExtraction, DiaryEntry, Embedder, EntityId, KnowledgeGraph, Memory, MemoryId,
    NamespaceCount, NamespaceId, PalaceGraph, ProjectTaxon, QueuedExtraction, RepoDirectory, Room,
    SearchHit, Session, SessionId, SessionTurn, Store, TunnelTraversal,
    capabilities::{
        ADMIN, KNOWLEDGE_READ, MEMORY_READ, MEMORY_WRITE, MINING_REVIEW, SESSION_INGEST,
        TRUST_PROMOTE,
    },
    harness::Harness,
    memory::MemorySource,
};

use crate::extractor::AuthPrincipal;
use crate::redaction::Redactor;

#[cfg(feature = "federation")]
use ijima_core::federation::{
    AuthoritativeScope, ConflictSignal, FederationState, InstanceFederationConfig, RoutedWrite,
    RoutedWriteReceipt,
};

/// Builds the Ijima HTTP application router.
///
/// `auth` and `store` are shared via axum's [`Extension`] layer; the
/// [`AuthPrincipal`] extractor reads `auth` to verify bearer tokens.
pub fn app(
    auth: Arc<crate::IjimaAuth>,
    store: Arc<dyn Store>,
    kg: Arc<dyn KnowledgeGraph>,
    embedder: Option<Arc<dyn Embedder>>,
    redactor: Arc<Redactor>,
    #[cfg(feature = "rate-limit")] rate_limiter: Option<crate::rate_limit::RateLimitState>,
    #[cfg(feature = "federation")] federation_config: Arc<InstanceFederationConfig>,
) -> Router {
    let router = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/memories", get(browse_memories).post(store_memory))
        .route("/memories/check", post(check_duplicate))
        .route("/memories/search", post(search_memories))
        .route("/memories/stats", get(memory_stats))
        .route("/memories/{id}", get(recall_memory).delete(delete_memory))
        .route("/memories/{id}/promote", post(promote_memory))
        .route("/rooms", get(list_rooms))
        .route("/taxonomy", get(taxonomy))
        .route("/palace/graph", get(palace_graph))
        .route("/palace/tunnel", get(traverse_tunnel))
        .route("/diaries", post(write_diary))
        .route("/diaries/{agent}", get(read_diary))
        .route("/repos", get(list_repos).post(register_repo))
        .route("/repos/resolve", get(resolve_repo))
        .route("/doctrine", post(ingest_doctrine))
        .route("/wakeup", get(wakeup))
        .route("/kg/triples", post(add_triple).get(find_triples))
        .route("/kg/entities/{id}", get(query_entity))
        .route("/kg/triples/{id}/invalidate", post(invalidate_triple))
        .route("/kg/timeline", get(kg_timeline))
        .route("/kg/stats", get(kg_stats))
        .route(
            "/sessions/{session_id}/turns",
            post(ingest_turn).get(session_turns),
        )
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/{session_id}/end", post(end_session))
        .route("/mining/queue", get(list_pending))
        .route("/mining/queue/{id}/accept", post(accept_extraction))
        .route("/mining/queue/{id}/reject", post(reject_extraction));
    #[cfg(feature = "mining")]
    let router = router.route("/sessions/{session_id}/mine", post(trigger_mine));

    #[cfg(feature = "federation")]
    let router = router
        .route("/federation/state", get(federation_state))
        .route("/federation/routed-write", post(routed_write))
        .route("/federation/conflict-signal", post(conflict_signal));
    let router = router
        .layer(Extension(auth))
        .layer(Extension(store))
        .layer(Extension(kg))
        .layer(Extension(embedder))
        .layer(Extension(redactor));

    #[cfg(feature = "federation")]
    let router = router.layer(Extension(federation_config));

    #[cfg(feature = "rate-limit")]
    let router = match rate_limiter {
        Some(rl) => router.layer(Extension(rl)),
        None => router,
    };
    #[cfg(not(feature = "rate-limit"))]
    let router = router;

    router
}

// ---------- errors ----------

/// API-level error mapping to HTTP status codes.
#[derive(Debug)]
pub enum ApiError {
    /// Capability check failed (principal's token lacks the required cap).
    Forbidden,
    /// Resource absent (or in a different namespace).
    NotFound,
    /// Malformed request body or parameters.
    BadRequest(String),
    /// Duplicate content (content-hash dedup) — 409.
    Conflict(String),
    /// Store / internal failure.
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg): (StatusCode, String) = match self {
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, msg).into_response()
    }
}

fn internal(e: ijima_core::IjimaError) -> ApiError {
    match e {
        ijima_core::IjimaError::Duplicate { detail } => ApiError::Conflict(detail),
        other => ApiError::Internal(other.to_string()),
    }
}

/// Query params carrying an optional namespace override + limit.
#[derive(Deserialize, Default)]
struct NsQuery {
    /// Override the default personal namespace. Personal namespaces
    /// (`ns_<name>_private`) belonging to *other* principals are
    /// rejected with 403; shared/global namespaces are allowed.
    namespace: Option<String>,
    limit: Option<usize>,
}

/// Resolves the effective namespace for a request: the caller's
/// personal namespace by default, or the requested one if authorized.
///
/// Authorization (v0, naming-convention based):
/// - `ns_<this_principal>_private` → allowed (own personal).
/// - any other `ns_*_private` → **403** (someone else's personal).
/// - anything else → allowed (shared / global).
fn resolve_ns(
    principal: &AuthPrincipal,
    requested: Option<&str>,
) -> Result<ijima_core::NamespaceId, ApiError> {
    let own = format!("ns_{}_private", principal.0.principal.as_str());
    match requested {
        None => Ok(ijima_core::NamespaceId::new(own)),
        Some(ns) if ns == own => Ok(ijima_core::NamespaceId::new(ns)),
        Some(ns) if ns.ends_with("_private") => Err(ApiError::Forbidden),
        Some(ns) => Ok(ijima_core::NamespaceId::new(ns)),
    }
}

// ---------- handlers ----------

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// Process start marker — captured once, when `/status` is first hit
/// (equivalently: daemon boot, since the router is built at boot).
static STARTED_AT: std::sync::OnceLock<std::time::SystemTime> = std::sync::OnceLock::new();

#[derive(Serialize)]
struct StatusResponse {
    memories: usize,
    namespaces: Vec<NamespaceCount>,
    entities: usize,
    triples: usize,
    /// Server version (crate version at compile time).
    version: &'static str,
    /// Wall-clock process start (unix seconds).
    started_at_unix: u64,
    /// Seconds since process start.
    uptime_secs: u64,
}

/// Global store statistics across all namespaces. Admin-gated (it spans
/// every principal's data). Per-namespace KG counts are available via
/// `GET /kg/stats?namespace=...`.
async fn status(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
) -> Result<Json<StatusResponse>, ApiError> {
    if !principal.0.may(ijima_core::capabilities::ADMIN) {
        return Err(ApiError::Forbidden);
    }
    let store_stats = store.store_stats().await.map_err(internal)?;
    let kg_stats = kg.kg_global_stats().await.map_err(internal)?;
    let started = *STARTED_AT.get_or_init(std::time::SystemTime::now);
    let uptime_secs = started.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    let started_at_unix = started
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(Json(StatusResponse {
        memories: store_stats.total_memories,
        namespaces: store_stats.namespaces,
        entities: kg_stats.entities,
        triples: kg_stats.triples,
        version: env!("CARGO_PKG_VERSION"),
        started_at_unix,
        uptime_secs,
    }))
}

#[derive(Serialize)]
struct IdResponse {
    id: String,
}

async fn store_memory(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(memory): Json<Memory>,
) -> Result<Json<IdResponse>, ApiError> {
    if !principal.0.may(MEMORY_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    let mut memory = memory;
    if memory.created_at.is_empty() {
        memory.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
    }
    let id = store.store_memory(&ns, memory).await.map_err(internal)?;
    Ok(Json(IdResponse { id: id.0 }))
}

#[derive(Deserialize)]
struct CheckDuplicateRequest {
    content: String,
}

#[derive(Serialize)]
struct CheckDuplicateResponse {
    /// The id of an existing memory with identical content, if any.
    duplicate: Option<String>,
}

/// Pre-check for content-hash dedup (`POST /memories/check`). Returns
/// the existing memory id if identical content is already stored in the
/// caller's (effective) namespace.
async fn check_duplicate(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<NsQuery>,
    Json(req): Json<CheckDuplicateRequest>,
) -> Result<Json<CheckDuplicateResponse>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let dup = store
        .check_duplicate(&ns, &req.content)
        .await
        .map_err(internal)?;
    Ok(Json(CheckDuplicateResponse {
        duplicate: dup.map(|id| id.0),
    }))
}

async fn recall_memory(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<Json<Memory>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    match store
        .recall_memory(&ns, &MemoryId(id))
        .await
        .map_err(internal)?
    {
        Some(memory) => Ok(Json(memory)),
        None => Err(ApiError::NotFound),
    }
}

async fn delete_memory(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(MEMORY_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    store
        .delete_memory(&ns, &MemoryId(id))
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchRequest {
    /// The query text. The daemon embeds this centrally with its own
    /// embedder (D9 §5: "the service owns the model"), guaranteeing
    /// vector compatibility with stored memories.
    text: String,
    limit: Option<usize>,
    /// Search scope: `personal` (default — the resolved namespace only) or
    /// `visible` (the principal's private namespace + the `global` commons,
    /// merged by similarity). The pi integration uses `visible` for parity
    /// with pi-mempalace's global search.
    scope: Option<String>,
}

#[derive(Serialize)]
struct SearchResponse {
    memories: Vec<SearchHit>,
}

async fn search_memories(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Extension(embedder): Extension<Option<Arc<dyn Embedder>>>,
    Query(q): Query<NsQuery>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let embedder = embedder
        .ok_or_else(|| ApiError::Internal("search unavailable: daemon has no embedder".into()))?;
    let query = embedder.embed(&req.text).map_err(internal)?;
    let limit = req.limit.unwrap_or(10);

    // `visible` scope: merge the principal's private namespace + the global
    // commons, ranked by similarity across both (pi-mempalace parity). The
    // `personal` default searches only the resolved namespace.
    let hits = if req.scope.as_deref() == Some("visible") {
        let own_ns = principal.0.personal_namespace();
        let global_ns = NamespaceId::new("global");
        let own_hits = store
            .search_memories(&own_ns, &query, limit)
            .await
            .map_err(internal)?;
        let global_hits = if own_ns == global_ns {
            Vec::new()
        } else {
            store
                .search_memories(&global_ns, &query, limit)
                .await
                .map_err(internal)?
        };
        merge_search_hits(own_hits, global_hits, limit)
    } else {
        let ns = resolve_ns(&principal, q.namespace.as_deref())?;
        store
            .search_memories(&ns, &query, limit)
            .await
            .map_err(internal)?
    };
    Ok(Json(SearchResponse { memories: hits }))
}

/// Merges two scored hit lists by similarity (desc), deduplicating by memory
/// id (the highest-similarity instance wins — NOT `dedup_by`, which only
/// drops adjacent dups) and truncating to `limit`. Pure — the `scope=visible`
/// path uses this to combine private + global results.
fn merge_search_hits(a: Vec<SearchHit>, b: Vec<SearchHit>, limit: usize) -> Vec<SearchHit> {
    use std::collections::HashSet;
    let mut all: Vec<SearchHit> = a.into_iter().chain(b).collect();
    all.sort_by(|x, y| {
        y.similarity
            .partial_cmp(&x.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Keep the first (highest-similarity, post-sort) instance of each id.
    let mut seen: HashSet<String> = HashSet::new();
    all.retain(|h| seen.insert(h.memory.id.0.clone()));
    all.truncate(limit);
    all
}

// ---------- promotion (personal → shared, D9 §2) ----------

#[derive(Deserialize)]
struct PromoteRequest {
    /// The shared/team namespace to promote into
    /// (e.g. `ns_team_default`).
    target_namespace: String,
    /// Optional id for the promoted copy. Defaults to
    /// `<original_id>__shared`.
    new_id: Option<String>,
}

#[derive(Serialize)]
struct PromoteResponse {
    id: String,
    original_id: String,
    target_namespace: String,
    redactions: Vec<crate::redaction::Redaction>,
}

/// Promotes a memory from the caller's personal namespace to a shared
/// namespace, running the [redaction filter](crate::redaction) at the
/// boundary. The original stays verbatim in personal scope; a scrubbed
/// copy lands in the target namespace.
async fn promote_memory(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Extension(redactor): Extension<Arc<Redactor>>,
    Path(id): Path<String>,
    Json(req): Json<PromoteRequest>,
) -> Result<Json<PromoteResponse>, ApiError> {
    if !principal.0.may(TRUST_PROMOTE) {
        return Err(ApiError::Forbidden);
    }
    let personal_ns = principal.0.personal_namespace();

    // Read from the caller's personal namespace.
    let memory = store
        .recall_memory(&personal_ns, &MemoryId(id.clone()))
        .await
        .map_err(internal)?
        .ok_or(ApiError::NotFound)?;

    // Scrub at the boundary (D9 §2 — the one place filtering happens).
    let scrubbed = redactor.redact(&memory.content);

    // Write the redacted copy to the shared namespace.
    let new_id = req
        .new_id
        .clone()
        .unwrap_or_else(|| format!("{id}__shared"));
    let promoted = Memory {
        id: MemoryId(new_id.clone()),
        content: scrubbed.text,
        project: memory.project,
        topic: memory.topic,
        source: ijima_core::memory::MemorySource::Explicit,
        harness: memory.harness,
        // Provenance back-reference to the original personal memory.
        session_id: Some(id.clone()),
        // Promotion preserves the origin/authority provenance of the source.
        origin: memory.origin.clone(),
        authority: memory.authority.clone(),
        importance: memory.importance,
        created_at: memory.created_at.clone(),
    };
    let target_ns = ijima_core::NamespaceId::new(&req.target_namespace);
    store
        .store_memory(&target_ns, promoted)
        .await
        .map_err(internal)?;

    Ok(Json(PromoteResponse {
        id: new_id,
        original_id: id,
        target_namespace: req.target_namespace,
        redactions: scrubbed.redactions,
    }))
}

// ---------- doctrine ingest (D9) ----------

#[derive(Deserialize)]
struct DoctrineRequest {
    id: String,
    content: String,
    project: String,
    topic: String,
}

/// Ingests a curated doctrine entry into the global `ns_doctrine`
/// namespace. Admin-gated — doctrine is PR-reviewed in Git and never
/// written by agents. Idempotent (delete-then-store) so re-ingests
/// upsert cleanly. No redaction (doctrine is pre-reviewed).
async fn ingest_doctrine(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(req): Json<DoctrineRequest>,
) -> Result<Json<IdResponse>, ApiError> {
    if !principal.0.may(ijima_core::capabilities::ADMIN) {
        return Err(ApiError::Forbidden);
    }
    let ns = ijima_core::NamespaceId::new(ijima_core::namespace::DOCTRINE_NAMESPACE);
    // Idempotent upsert: remove any existing entry, then store.
    store
        .delete_memory(&ns, &MemoryId(req.id.clone()))
        .await
        .map_err(internal)?;
    let memory = Memory {
        id: MemoryId(req.id.clone()),
        content: req.content,
        project: req.project,
        topic: req.topic,
        source: ijima_core::memory::MemorySource::Doctrine,
        harness: ijima_core::harness::Harness::Other,
        session_id: None,
        // Doctrine is the curated local tier — authoritative on this instance.
        origin: ijima_core::InstanceId::local(),
        authority: ijima_core::AuthorityScope::local(),
        importance: 1.0,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default(),
    };
    store.store_memory(&ns, memory).await.map_err(internal)?;
    Ok(Json(IdResponse { id: req.id }))
}

// ---------- wake-up composition (D9 §4) ----------

/// How many personal essentials to include in a wake-up response.
const WAKEUP_PERSONAL_LIMIT: usize = 20;
/// How many doctrine entries to include.
const WAKEUP_DOCTRINE_LIMIT: usize = 50;

#[derive(Serialize)]
struct WakeupResponse {
    /// L0: the authenticated principal's identity.
    identity: serde_json::Value,
    /// L1a: the caller's personal essentials (top-N by importance + recency).
    personal_essentials: Vec<Memory>,
    /// L1b: the shared team doctrine baseline (identical across the team).
    doctrine: Vec<Memory>,
}

/// Composes the session-start context: L0 identity + L1a personal
/// essentials + L1b team doctrine. This is the "shared brain" — L1b is
/// identical across the team, L1a is the individual's personal brain.
async fn wakeup(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
) -> Result<Json<WakeupResponse>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let personal_ns = principal.0.personal_namespace();
    let doctrine_ns = ijima_core::NamespaceId::new(ijima_core::namespace::DOCTRINE_NAMESPACE);

    let (personal_essentials, doctrine) = tokio::join!(
        store.list_memories(&personal_ns, WAKEUP_PERSONAL_LIMIT),
        store.list_memories(&doctrine_ns, WAKEUP_DOCTRINE_LIMIT),
    );

    Ok(Json(WakeupResponse {
        identity: serde_json::json!({ "principal": principal.0.principal.as_str() }),
        personal_essentials: personal_essentials.map_err(internal)?,
        doctrine: doctrine.map_err(internal)?,
    }))
}

// ---------- knowledge graph ----------

#[derive(Deserialize)]
struct AddTripleRequest {
    subject: String,
    predicate: String,
    object: String,
    valid_from: Option<String>,
    confidence: Option<f32>,
    source_memory_id: Option<String>,
}

async fn add_triple(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(req): Json<AddTripleRequest>,
) -> Result<Json<ijima_core::Triple>, ApiError> {
    if !principal.0.may(ijima_core::capabilities::KNOWLEDGE_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, None)?;
    let triple = kg
        .add_triple(
            &ns,
            EntityId::new(req.subject),
            &req.predicate,
            EntityId::new(req.object),
            req.valid_from.as_deref(),
            req.confidence.unwrap_or(1.0),
            req.source_memory_id.as_deref(),
        )
        .await
        .map_err(internal)?;
    // Touch `store` so the Extension is consumed.
    let _ = store;
    Ok(Json(triple))
}

async fn query_entity(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Path(id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<Json<ijima_core::EntityRecord>, ApiError> {
    if !principal.0.may(KNOWLEDGE_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let rec = kg
        .query_entity(&ns, &EntityId::new(id))
        .await
        .map_err(internal)?;
    Ok(Json(rec))
}

async fn invalidate_triple(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Path(id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(ijima_core::capabilities::KNOWLEDGE_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    kg.invalidate_triple(&ns, &id).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
struct FindTriplesQuery {
    namespace: Option<String>,
    subject: Option<String>,
    predicate: Option<String>,
    object: Option<String>,
}

async fn find_triples(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Query(q): Query<FindTriplesQuery>,
) -> Result<Json<Vec<ijima_core::Triple>>, ApiError> {
    if !principal.0.may(KNOWLEDGE_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let triples = kg
        .find_triples(
            &ns,
            q.subject.as_deref().map(EntityId::new).as_ref(),
            q.predicate.as_deref(),
            q.object.as_deref().map(EntityId::new).as_ref(),
        )
        .await
        .map_err(internal)?;
    Ok(Json(triples))
}

async fn kg_timeline(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Query(q): Query<NsQuery>,
) -> Result<Json<Vec<ijima_core::Triple>>, ApiError> {
    if !principal.0.may(KNOWLEDGE_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let triples = kg
        .kg_timeline(&ns, q.limit.unwrap_or(50))
        .await
        .map_err(internal)?;
    Ok(Json(triples))
}

async fn kg_stats(
    principal: AuthPrincipal,
    Extension(kg): Extension<Arc<dyn KnowledgeGraph>>,
    Query(q): Query<NsQuery>,
) -> Result<Json<ijima_core::KgStats>, ApiError> {
    if !principal.0.may(KNOWLEDGE_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let stats = kg.knowledge_stats(&ns).await.map_err(internal)?;
    Ok(Json(stats))
}

async fn ingest_turn(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(session_id): Path<String>,
    Json(mut turn): Json<SessionTurn>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(SESSION_INGEST) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    turn.session_id = SessionId::new(session_id);
    store.ingest_turn(&ns, turn).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// TurnsQuery is unified into NsQuery above.

#[derive(Serialize)]
struct TurnsResponse {
    turns: Vec<SessionTurn>,
}

async fn session_turns(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(session_id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<Json<TurnsResponse>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let turns = store
        .session_turns(&ns, &SessionId::new(session_id), q.limit.unwrap_or(50))
        .await
        .map_err(internal)?;
    Ok(Json(TurnsResponse { turns }))
}

/// Creates (or upserts) a session's metadata. `ended_at` is forced to
/// `None` on create — use `POST /sessions/:id/end` to close a session.
/// Auth: `session:ingest`. The session is stored in the caller's
/// personal namespace (matching turn ingest).
async fn create_session(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(mut session): Json<Session>,
) -> Result<Json<IdResponse>, ApiError> {
    if !principal.0.may(SESSION_INGEST) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    if session.started_at.is_empty() {
        session.started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
    }
    session.ended_at = None;
    let id = store.create_session(&ns, session).await.map_err(internal)?;
    Ok(Json(IdResponse { id: id.0 }))
}

#[derive(Deserialize)]
struct SessionListQuery {
    namespace: Option<String>,
    /// Optional harness filter (wire string, e.g. `pi`).
    harness: Option<String>,
    limit: Option<usize>,
}

/// Lists sessions in the effective namespace, newest first, optionally
/// filtered by harness. Auth: `memory:read` (session metadata is
/// read via the same capability as memory palace reads).
async fn list_sessions(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<SessionListQuery>,
) -> Result<Json<Vec<Session>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let harness = q.harness.as_deref().map(Harness::from_wire_str);
    let limit = q.limit.unwrap_or(50).min(500);
    let sessions = store
        .list_sessions(&ns, harness.as_ref(), limit)
        .await
        .map_err(internal)?;
    Ok(Json(sessions))
}

#[derive(Deserialize)]
struct EndSessionRequest {
    ended_at: String,
}

/// Marks a session as ended. Scoped to the caller's personal namespace.
/// Auth: `session:ingest`.
async fn end_session(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(session_id): Path<String>,
    Json(req): Json<EndSessionRequest>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(SESSION_INGEST) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    store
        .end_session(&ns, &SessionId::new(session_id), req.ended_at)
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- mining review queue (ADR M2, M3) ----------

/// Lists pending mining extractions in the effective namespace, newest
/// first. Auth: `mining:review`.
async fn list_pending(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<NsQuery>,
) -> Result<Json<Vec<QueuedExtraction>>, ApiError> {
    if !principal.0.may(MINING_REVIEW) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let limit = q.limit.unwrap_or(50).min(500);
    let pending = store.list_pending(&ns, limit).await.map_err(internal)?;
    Ok(Json(pending))
}

/// Accepts a queued extraction: promotes it to the palace and removes it
/// from the queue. Auth: `mining:review`.
async fn accept_extraction(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(id): Path<String>,
) -> Result<Json<AcceptedExtraction>, ApiError> {
    if !principal.0.may(MINING_REVIEW) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    let accepted = store.accept_extraction(&ns, &id).await.map_err(internal)?;
    Ok(Json(accepted))
}

/// Rejects a queued extraction: drops it without promoting. Auth:
/// `mining:review`. Returns 204.
async fn reject_extraction(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(MINING_REVIEW) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    store.reject_extraction(&ns, &id).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- mining trigger (ADR M1, M3, M7) ----------

/// Triggers an extraction pass over a session's turns: runs the rules tier
/// (always) plus the llm tier when `IJIMA_LLM_*` is configured, merges +
/// content-dedups, then ingests — `Auto` extractions archive to the palace,
/// `PendingReview` stage in the review queue. Auth: `mining:trigger`.
///
/// The llm agent's `HttpAgent::respond` blocks on its own tokio runtime, so
/// the synchronous `mine_all` pass runs on a blocking thread (via
/// [`tokio::task::spawn_blocking`]) to avoid a runtime-in-runtime panic
/// inside this async handler. The concrete [`HttpAgent`] is `Send`; the
/// `&mut dyn Agent` coercion happens *inside* the closure, so it never
/// crosses the spawn boundary as an unsized non-`Send` trait object.
#[cfg(feature = "mining")]
async fn trigger_mine(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(session_id): Path<String>,
    Query(q): Query<NsQuery>,
) -> Result<Json<crate::mining_pipeline::MiningReport>, ApiError> {
    use proserpina::backend::http::HttpAgent;

    if !principal.0.may(MINING_TRIGGER) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;

    // Fetch the session's turns (a generous limit — v0 mines the whole session).
    let turns = store
        .session_turns(&ns, &SessionId::new(session_id.clone()), 10_000)
        .await
        .map_err(internal)?;
    let turn_texts: Vec<String> = turns.into_iter().map(|t| t.content).collect();
    let ctx = crate::mining_pipeline::mining_context(&session_id, "general", Harness::Other);

    // The extraction pass is synchronous (ADR M1); the llm agent bridges to
    // async HTTP internally via its own runtime + `block_on`. Run it on a
    // blocking thread so that `block_on` is legal (we are outside any async
    // executor here). `build_mining_agent` returns a concrete `Option<HttpAgent>`
    // — kept as the concrete type (not a trait object) so it stays `Send` for
    // the move into the spawned task.
    let extractions = tokio::task::spawn_blocking(move || {
        let mut agent: Option<HttpAgent> = build_mining_agent();
        let agent_dyn: Option<&mut dyn proserpina::Agent> =
            agent.as_mut().map(|a| a as &mut dyn proserpina::Agent);
        ijima_miner::mine_all(&turn_texts, &ctx, agent_dyn)
    })
    .await
    .map_err(|e| {
        internal(ijima_core::IjimaError::Mining {
            detail: format!("extraction task failed: {e}"),
        })
    })?
    .map_err(internal)?;

    let report = crate::mining_pipeline::ingest_extractions(store.as_ref(), &ns, extractions)
        .await
        .map_err(internal)?;
    Ok(Json(report))
}

/// Constructs the llm extraction agent from `IJIMA_LLM_*` env config, or
/// `None` when mining should run rules-only (no `IJIMA_LLM_MODEL` /
/// `IJIMA_LLM_API_KEY` set). `mine_all(None)` then skips the llm tier.
///
/// Defaults `IJIMA_LLM_BASE_URL` to the DeepSeek endpoint. The agent uses a
/// single "Session Mining Extractor" persona covering both fact and pattern
/// extraction; v0 does not vary the agent persona per role (ADR M5,
/// single-shot). Returns a concrete [`HttpAgent`] (not a trait object) so it
/// remains `Send` for the blocking-thread move.
#[cfg(feature = "mining")]
fn build_mining_agent() -> Option<proserpina::backend::http::HttpAgent> {
    use proserpina::{
        AgentId, Persona,
        backend::http::{HttpAgent, HttpConfig},
    };

    let base_url = std::env::var("IJIMA_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string());
    let model = std::env::var("IJIMA_LLM_MODEL").ok()?;
    let api_key = std::env::var("IJIMA_LLM_API_KEY").ok()?;

    let persona = Persona::new("Session Mining Extractor")
        .with_framing(
            "You mine session transcripts for durable facts and recurring \
             patterns. Output one JSON object per line, each \
             {\"content\",\"project\",\"topic\",\"confidence\"}. Omit all \
             preamble. If nothing worth extracting, output nothing.",
        )
        .with_focus(
            "decisions, chosen tools, stated constraints, measurements, recurring workflows",
        );

    Some(HttpAgent::new(
        AgentId::new("ijima-miner"),
        persona,
        HttpConfig {
            base_url,
            model,
            api_key,
        },
    ))
}

// ===== Palace organization (memory:read) =====

#[derive(Deserialize)]
struct NamespaceQuery {
    namespace: Option<String>,
}

#[derive(Deserialize)]
struct RoomsQuery {
    namespace: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct TunnelQuery {
    namespace: Option<String>,
    topic: String,
    project_a: String,
    project_b: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct DiaryQuery {
    namespace: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct MemoryBrowseQuery {
    namespace: Option<String>,
    project: Option<String>,
    topic: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct ResolveRepoQuery {
    cwd: String,
}

/// Lists rooms (topic cells), optionally filtered to a project. Auth: `memory:read`.
async fn list_rooms(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<RoomsQuery>,
) -> Result<Json<Vec<Room>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let limit = q.limit.unwrap_or(50).min(500);
    let rooms = store
        .list_rooms(&ns, q.project.as_deref(), limit)
        .await
        .map_err(internal)?;
    Ok(Json(rooms))
}

/// Full project → topic → count taxonomy. Auth: `memory:read`.
async fn taxonomy(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<NamespaceQuery>,
) -> Result<Json<Vec<ProjectTaxon>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    Ok(Json(store.taxonomy(&ns).await.map_err(internal)?))
}

/// The palace graph: projects as nodes, shared-topic tunnels as edges. Auth: `memory:read`.
async fn palace_graph(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<NamespaceQuery>,
) -> Result<Json<PalaceGraph>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    Ok(Json(store.palace_graph(&ns).await.map_err(internal)?))
}

/// Traverses a tunnel — the memories from both projects on a shared topic. Auth: `memory:read`.
async fn traverse_tunnel(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<TunnelQuery>,
) -> Result<Json<TunnelTraversal>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let limit = q.limit.unwrap_or(50).min(500);
    Ok(Json(
        store
            .traverse_tunnel(&ns, &q.topic, &q.project_a, &q.project_b, limit)
            .await
            .map_err(internal)?,
    ))
}

/// Appends a diary entry to the caller's namespace. Auth: `memory:write`.
async fn write_diary(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(entry): Json<DiaryEntry>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(MEMORY_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();
    store.write_diary(&ns, entry).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reads `agent`'s diary in the caller's namespace. Auth: `memory:read`.
async fn read_diary(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Path(agent): Path<String>,
    Query(q): Query<DiaryQuery>,
) -> Result<Json<Vec<DiaryEntry>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let limit = q.limit.unwrap_or(50).min(500);
    Ok(Json(
        store
            .read_diary(&ns, &agent, limit)
            .await
            .map_err(internal)?,
    ))
}

/// Browses memories (the `memory_recall` path), optionally filtered to
/// project/topic — distinct from the importance-ranked wake-up feed. Auth: `memory:read`.
async fn browse_memories(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<MemoryBrowseQuery>,
) -> Result<Json<Vec<Memory>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let limit = q.limit.unwrap_or(50).min(500);
    Ok(Json(
        store
            .list_memories_filtered(&ns, q.project.as_deref(), q.topic.as_deref(), limit)
            .await
            .map_err(internal)?,
    ))
}

#[derive(Serialize)]
struct NamespaceStats {
    total: usize,
    projects: Vec<ProjectCount>,
}

#[derive(Serialize)]
struct ProjectCount {
    project: String,
    count: usize,
}

/// Read-accessible namespace stats (derived from room counts; unlike
/// `/status` which is admin-gated). Auth: `memory:read`.
async fn memory_stats(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<NamespaceQuery>,
) -> Result<Json<NamespaceStats>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let rooms = store.list_rooms(&ns, None, 1000).await.map_err(internal)?;
    let total: usize = rooms.iter().map(|r| r.count).sum();
    let mut by_project: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for r in &rooms {
        *by_project.entry(r.project.clone()).or_default() += r.count;
    }
    let projects = by_project
        .into_iter()
        .map(|(project, count)| ProjectCount { project, count })
        .collect();
    Ok(Json(NamespaceStats { total, projects }))
}

// ===== Repo directory (global registry — Context Mapper) =====

/// Registers/upserts a repo in the global registry (operator action). Auth: `admin`.
async fn register_repo(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Json(repo): Json<RepoDirectory>,
) -> Result<StatusCode, ApiError> {
    if !principal.0.may(ADMIN) {
        return Err(ApiError::Forbidden);
    }
    store.register_repo(repo).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists every registered repo (the ecosystem roster). Auth: `memory:read`.
async fn list_repos(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
) -> Result<Json<Vec<RepoDirectory>>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(store.list_repos().await.map_err(internal)?))
}

/// Reverse-resolves a working directory to its registered repo. Auth: `memory:read`.
async fn resolve_repo(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Query(q): Query<ResolveRepoQuery>,
) -> Result<Json<RepoDirectory>, ApiError> {
    if !principal.0.may(MEMORY_READ) {
        return Err(ApiError::Forbidden);
    }
    match store.resolve_repo(&q.cwd).await.map_err(internal)? {
        Some(repo) => Ok(Json(repo)),
        None => Err(ApiError::NotFound),
    }
}

// ---------- federation control API (scaffold; feature `federation`) ----------

/// `GET /federation/state` — the instance's federated self-description.
#[cfg(feature = "federation")]
async fn federation_state(
    Extension(cfg): Extension<Arc<InstanceFederationConfig>>,
) -> Json<FederationState> {
    Json(cfg.to_state())
}

/// `POST /federation/routed-write` — apply a write under an authoritative scope.
///
/// Scaffold: applies the write locally with provenance stamping (origin =
/// this instance, authority = the scope) but performs **no** boundary
/// enforcement — no trust-tier egress filtering, scope/airgap deny, or
/// boundary transformation. Ijima's non-bypassable safety floor is the
/// follow-on (ADR `federation-control-api` §Deferred).
#[cfg(feature = "federation")]
async fn routed_write(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    Extension(cfg): Extension<Arc<InstanceFederationConfig>>,
    Json(write): Json<RoutedWrite>,
) -> Result<Json<RoutedWriteReceipt>, ApiError> {
    if !principal.0.may(MEMORY_WRITE) {
        return Err(ApiError::Forbidden);
    }
    let RoutedWrite {
        target: _,
        scope,
        operation: _,
        payload,
    } = write;

    // === Boundary enforcement (non-bypassable; the federation ingress path) ===
    // (1) Airgap: a sovereign instance rejects all federation writes.
    if cfg.role == ijima_core::federation::InstanceRole::Airgapped {
        return Err(ApiError::Forbidden);
    }
    // (2) Scope filter: accept only writes for scopes this instance is
    //     authoritative for (default-deny for sovereignty).
    if !cfg.accepts_scope(&scope) {
        return Err(ApiError::BadRequest(format!(
            "out of authoritative scope: {}/{}",
            scope.namespace, scope.project
        )));
    }

    let mut memory: Memory = serde_json::from_value(payload)
        .map_err(|e| ApiError::BadRequest(format!("payload is not a Memory: {e}")))?;
    // Stamp federation provenance: this instance applied it; the routed scope
    // is the source-of-truth authority for the record.
    memory.origin = ijima_core::provenance::InstanceId::local();
    memory.authority =
        ijima_core::provenance::AuthorityScope(format!("{}/{}", scope.namespace, scope.project));
    if memory.created_at.is_empty() {
        memory.created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
    }
    let ns = principal.0.personal_namespace();

    // (3) Trust-tier ingress: doctrine arriving via federation is never
    //     auto-trusted — stage it as PendingReview (never auto-promoted).
    //     Lower tiers (Explicit/Mined/AutoCapture) cross as-is.
    let (commit, mut warnings) = if memory.source == MemorySource::Doctrine {
        let pending = store
            .enqueue_extraction(&ns, memory, 0.5)
            .await
            .map_err(internal)?;
        (
            pending,
            vec!["doctrine downgraded to PendingReview (trust-tier ingress rule)".into()],
        )
    } else {
        let id = store.store_memory(&ns, memory).await.map_err(internal)?;
        (id.0, Vec::new())
    };
    warnings.push("boundary enforcement: scope + airgap + doctrine-downgrade applied".into());
    Ok(Json(RoutedWriteReceipt {
        accepted: true,
        instance: cfg.instance_id.clone(),
        scope,
        commit: Some(commit),
        warnings,
    }))
}

/// `POST /federation/conflict-signal` — poll for a conflict on a scope.
///
/// Scaffold: no conflict detection yet. Returns `404` (no active conflict);
/// the single-instance deployment has no peer to conflict with.
#[cfg(feature = "federation")]
async fn conflict_signal(
    Json(_scope): Json<AuthoritativeScope>,
) -> Result<Json<ConflictSignal>, ApiError> {
    Err(ApiError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IjimaAuth;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use ijima_core::{harness::Harness, memory::MemorySource};
    use tower::ServiceExt;

    async fn app_with_store() -> (Router, Arc<IjimaAuth>) {
        let auth = Arc::new(IjimaAuth::from_embedded_policy().expect("policy"));
        let store_inner = Arc::new(crate::SurrealStore::open_embedded().await.expect("open"));
        let store: Arc<dyn Store> = store_inner.clone();
        let kg: Arc<dyn KnowledgeGraph> = store_inner;
        (
            app(
                auth.clone(),
                store,
                kg,
                None,
                Arc::new(crate::redaction::Redactor::new()),
                #[cfg(feature = "rate-limit")]
                None,
                #[cfg(feature = "federation")]
                Arc::new(InstanceFederationConfig::default()),
            ),
            auth,
        )
    }

    /// Like [`app_with_store`] but with a custom federation config — for
    /// boundary-enforcement tests (airgap, out-of-scope).
    #[cfg(feature = "federation")]
    async fn app_with_federation_config(
        config: InstanceFederationConfig,
    ) -> (Router, Arc<IjimaAuth>) {
        let auth = Arc::new(IjimaAuth::from_embedded_policy().expect("policy"));
        let store_inner = Arc::new(crate::SurrealStore::open_embedded().await.expect("open"));
        let store: Arc<dyn Store> = store_inner.clone();
        let kg: Arc<dyn KnowledgeGraph> = store_inner;
        (
            app(
                auth.clone(),
                store,
                kg,
                None,
                Arc::new(crate::redaction::Redactor::new()),
                #[cfg(feature = "rate-limit")]
                None,
                Arc::new(config),
            ),
            auth,
        )
    }

    fn bearer(auth: &IjimaAuth, principal: &str, cap: &str) -> String {
        format!(
            "Bearer {}",
            auth.issue_bearer(principal, cap).expect("issue")
        )
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn federation_state_returns_local_config() {
        let (app, _auth) = app_with_store().await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/federation/state")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let state = body_json(res).await;
        assert_eq!(state["instance_id"], "local");
        assert_eq!(state["role"], "Unifying");
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn routed_write_applies_a_memory() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let body = serde_json::json!({
            "target": "local",
            "scope": {"namespace": "local", "project": "Dominic"},
            "operation": "Create",
            "payload": {
                "id": "mem_fed_test",
                "content": "federated hello",
                "project": "Dominic",
                "topic": "federated",
                "source": "Explicit",
                "harness": "Dominic"
            }
        })
        .to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/routed-write")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let receipt = body_json(res).await;
        assert_eq!(receipt["accepted"], true);
        assert!(receipt["commit"].as_str().is_some());
        assert_eq!(
            receipt["warnings"][0],
            "boundary enforcement: scope + airgap + doctrine-downgrade applied"
        );
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn routed_write_requires_memory_write() {
        let (app, auth) = app_with_store().await;
        let read = bearer(&auth, "elliott", MEMORY_READ); // read cap, not write
        let body = serde_json::json!({
            "target": "local",
            "scope": {"namespace": "local", "project": "Dominic"},
            "operation": "Create",
            "payload": {
                "id": "x", "content": "c", "project": "p",
                "topic": "t", "source": "Explicit", "harness": "Dominic"
            }
        })
        .to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/routed-write")
                    .header("authorization", &read)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn routed_write_rejects_out_of_scope() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        // default config is authoritative for {local, *}; {shared, ...} is out of scope
        let body = serde_json::json!({
            "target": "local",
            "scope": {"namespace": "shared", "project": "Dominic"},
            "operation": "Create",
            "payload": {
                "id": "x", "content": "c", "project": "p",
                "topic": "t", "source": "Explicit", "harness": "Dominic"
            }
        })
        .to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/routed-write")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn routed_write_rejects_when_airgapped() {
        let cfg = InstanceFederationConfig {
            role: ijima_core::federation::InstanceRole::Airgapped,
            ..InstanceFederationConfig::default()
        };
        let (app, auth) = app_with_federation_config(cfg).await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let body = serde_json::json!({
            "target": "local",
            "scope": {"namespace": "local", "project": "Dominic"},
            "operation": "Create",
            "payload": {
                "id": "x", "content": "c", "project": "p",
                "topic": "t", "source": "Explicit", "harness": "Dominic"
            }
        })
        .to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/routed-write")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn routed_write_downgrades_doctrine_to_pending() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let body = serde_json::json!({
            "target": "local",
            "scope": {"namespace": "local", "project": "Dominic"},
            "operation": "Create",
            "payload": {
                "id": "mem_doctrine",
                "content": "peer-claimed doctrine",
                "project": "Dominic",
                "topic": "federated",
                "source": "Doctrine",
                "harness": "Dominic"
            }
        })
        .to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/routed-write")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let receipt = body_json(res).await;
        assert_eq!(receipt["accepted"], true);
        assert_eq!(
            receipt["warnings"][0],
            "doctrine downgraded to PendingReview (trust-tier ingress rule)"
        );
    }

    #[cfg(feature = "federation")]
    #[tokio::test]
    async fn conflict_signal_returns_404_when_none() {
        let (app, _auth) = app_with_store().await;
        let body = serde_json::json!({"namespace": "shared", "project": "Dominic"}).to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/federation/conflict-signal")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    fn sample_memory_json(id: &str) -> String {
        serde_json::json!({
            "id": id,
            "content": "decided to wire the daemon",
            "project": "ijima",
            "topic": "api",
            "source": "Explicit",
            "harness": "Pi",
            "session_id": "sess_1",
            "importance": 0.5,
            "created_at": "0",
        })
        .to_string()
    }

    #[tokio::test]
    async fn health_is_public() {
        let (app, _) = app_with_store().await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn recall_without_auth_is_401() {
        let (app, _) = app_with_store().await;
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn store_then_recall_round_trips() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let read = bearer(&auth, "elliott", MEMORY_READ);

        // POST /memories
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(sample_memory_json("mem_1")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // GET /memories/mem_1
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_1")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let mem: Memory = serde_json::from_slice(&body).unwrap();
        assert_eq!(mem.content, "decided to wire the daemon");
        assert_eq!(mem.harness, Harness::Pi);
        assert_eq!(mem.source, MemorySource::Explicit);
    }

    #[tokio::test]
    async fn store_with_read_only_token_is_403() {
        let (app, auth) = app_with_store().await;
        let read = bearer(&auth, "elliott", MEMORY_READ);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &read)
                    .header("content-type", "application/json")
                    .body(Body::from(sample_memory_json("mem_x")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn namespace_isolation_across_principals() {
        let (app, auth) = app_with_store().await;
        // alice stores
        let alice_write = bearer(&auth, "alice", MEMORY_WRITE);
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &alice_write)
                    .header("content-type", "application/json")
                    .body(Body::from(sample_memory_json("mem_a")))
                    .unwrap(),
            )
            .await
            .unwrap();
        // bob cannot recall alice's memory
        let bob_read = bearer(&auth, "bob", MEMORY_READ);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_a")
                    .header("authorization", &bob_read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn promote_redacts_secrets_and_leaves_original_intact() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let read = bearer(&auth, "elliott", MEMORY_READ);
        let promote = bearer(&auth, "elliott", TRUST_PROMOTE);

        // Store a personal memory containing a secret.
        let body = serde_json::json!({
            "id": "mem_secret",
            "content": "deploy key sk-abcdefghijklmnopqrstuvwxyz1234567890 contact ops@test.com",
            "project": "ijima",
            "topic": "ops",
            "source": "Explicit",
            "harness": "Pi",
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Promote to a shared namespace.
        let promote_body = serde_json::json!({
            "target_namespace": "ns_team_shared",
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories/mem_secret/promote")
                    .header("authorization", &promote)
                    .header("content-type", "application/json")
                    .body(Body::from(promote_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let resp: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let new_id = resp["id"].as_str().unwrap();
        assert_eq!(new_id, "mem_secret__shared");
        let cats: Vec<&str> = resp["redactions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["category"].as_str().unwrap())
            .collect();
        assert!(cats.contains(&"api_key"));
        assert!(cats.contains(&"email"));

        // The original personal memory is untouched (verbatim).
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_secret")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let orig: Memory = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(orig.content.contains("sk-abcdef"));
        assert!(orig.content.contains("ops@test.com"));

        // The promoted shared copy is readable via ?namespace= and has
        // secrets scrubbed.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_secret__shared?namespace=ns_team_shared")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let shared: Memory = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(shared.content.contains("[REDACTED:api_key]"));
        assert!(shared.content.contains("[REDACTED:email]"));
        assert!(!shared.content.contains("sk-abcdef"));
        assert!(!shared.content.contains("ops@test.com"));
        // Provenance back-reference.
        assert_eq!(shared.session_id.as_deref(), Some("mem_secret"));
    }

    #[tokio::test]
    async fn promote_requires_trust_promote_not_memory_write() {
        // ADR provenance-tier: raising trust is costlier than writing at a
        // tier, so promote_memory requires trust:promote (codim 4), not
        // memory:write (codim 2). A memory:write-only token gets 403.
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let body = serde_json::json!({
            "id": "mem_p",
            "content": "provenance tier test",
            "project": "ijima",
            "topic": "t",
            "source": "Explicit",
            "harness": "Pi",
        })
        .to_string();
        // Store succeeds with memory:write.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Promote is forbidden with only memory:write.
        let promote_body = serde_json::json!({ "target_namespace": "ns_team_shared" }).to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories/mem_p/promote")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(promote_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // A trust:promote holder succeeds.
        let promote = bearer(&auth, "elliott", TRUST_PROMOTE);
        let promote_body = serde_json::json!({ "target_namespace": "ns_team_shared" }).to_string();
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories/mem_p/promote")
                    .header("authorization", &promote)
                    .header("content-type", "application/json")
                    .body(Body::from(promote_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cross_principal_personal_namespace_is_forbidden() {
        let (app, auth) = app_with_store().await;
        // Alice stores a memory.
        let alice_write = bearer(&auth, "alice", MEMORY_WRITE);
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &alice_write)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "mem_a",
                            "content": "alice only",
                            "project": "x",
                            "topic": "x",
                            "source": "Explicit",
                            "harness": "Pi",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Bob tries to read alice's personal namespace explicitly.
        let bob_read = bearer(&auth, "bob", MEMORY_READ);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/mem_a?namespace=ns_alice_private")
                    .header("authorization", &bob_read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn doctrine_ingest_requires_admin_and_is_readable_shared() {
        let (app, auth) = app_with_store().await;
        let admin = bearer(&auth, "ci", "admin");
        let read = bearer(&auth, "anyone", MEMORY_READ);

        // Non-admin cannot ingest doctrine.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/doctrine")
                    .header("authorization", &read)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "d1",
                            "content": "doctrine body",
                            "project": "ijima",
                            "topic": "arch",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // Admin ingests.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/doctrine")
                    .header("authorization", &admin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "d1",
                            "content": "doctrine body",
                            "project": "ijima",
                            "topic": "arch",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Any read-capable principal can recall doctrine from ns_doctrine.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/memories/d1?namespace=ns_doctrine")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let mem: Memory = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(mem.content, "doctrine body");
        assert_eq!(mem.source, ijima_core::memory::MemorySource::Doctrine);
    }

    #[tokio::test]
    async fn wakeup_composes_personal_and_doctrine() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let admin = bearer(&auth, "ci", "admin");
        let read = bearer(&auth, "elliott", MEMORY_READ);

        // Store a personal memory.
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "mem_p",
                            "content": "personal essential",
                            "project": "ijima",
                            "topic": "x",
                            "source": "Explicit",
                            "harness": "Pi",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Ingest doctrine.
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/doctrine")
                    .header("authorization", &admin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "doc_1",
                            "content": "doctrine baseline",
                            "project": "ijima",
                            "topic": "arch",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Wake-up composes both.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/wakeup")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["identity"]["principal"], "elliott");
        assert_eq!(body["personal_essentials"].as_array().unwrap().len(), 1);
        assert_eq!(
            body["personal_essentials"][0]["content"],
            "personal essential"
        );
        assert_eq!(body["doctrine"].as_array().unwrap().len(), 1);
        assert_eq!(body["doctrine"][0]["content"], "doctrine baseline");
        assert_eq!(body["doctrine"][0]["source"], "Doctrine");
    }

    #[tokio::test]
    async fn knowledge_graph_add_query_invalidate() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", "knowledge:write");
        let read = bearer(&auth, "elliott", "knowledge:read");

        // Add a triple.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/kg/triples")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "subject": "Ijima",
                            "predicate": "depends_on",
                            "object": "SurrealDB",
                            "confidence": 1.0,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Query the entity — outgoing edge present.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/kg/entities/Ijima")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["outgoing"].as_array().unwrap().len(), 1);
        assert_eq!(body["outgoing"][0]["object"], "SurrealDB");
        assert!(body["incoming"].as_array().unwrap().is_empty());

        // Stats.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/kg/stats")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["entities"], 2);
        assert_eq!(body["triples"], 1);

        // Invalidate.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/kg/triples/Ijima:depends_on:SurrealDB/invalidate")
                    .header("authorization", &write)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn status_requires_admin_and_reports_counts() {
        let (app, auth) = app_with_store().await;
        let admin = bearer(&auth, "op", "admin");
        let read = bearer(&auth, "user", MEMORY_READ);

        // Store a memory + a triple so counts are non-zero.
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", &admin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "id": "m1",
                            "content": "stat test",
                            "project": "x",
                            "topic": "x",
                            "source": "Explicit",
                            "harness": "Pi",
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Non-admin is forbidden.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // Admin sees global counts.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .header("authorization", &admin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["memories"], 1);
        // Deploy-kit fields: version pinned to the crate version, sane
        // uptime, real start time.
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        let uptime = body["uptime_secs"].as_u64().expect("uptime is u64");
        assert!(uptime < 60, "fresh test app should have tiny uptime");
        assert!(
            body["started_at_unix"].as_u64().expect("started_at is u64") > 1_000_000_000,
            "started_at looks like a unix timestamp"
        );
        assert!(!body["namespaces"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sessions_create_list_end_via_http() {
        let (app, auth) = app_with_store().await;
        let ingest = bearer(&auth, "op", SESSION_INGEST);
        let read = bearer(&auth, "op", MEMORY_READ);

        // Create two sessions.
        for (id, harness) in [("sess_a", "Pi"), ("sess_b", "Sakamoto")] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/sessions")
                        .header("authorization", &ingest)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "id": id,
                                "harness": harness,
                                "channel": "thread-1",
                                "started_at": "2026-07-05T10:00:00Z",
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }

        // List — both present.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        // Filter by harness=pi.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/sessions?harness=pi")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert_eq!(body[0]["harness"], "Pi");

        // End sess_a.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/sess_a/end")
                    .header("authorization", &ingest)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "ended_at": "2026-07-05T11:00:00Z" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Verify ended_at is persisted.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/sessions?harness=pi")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body[0]["ended_at"], "2026-07-05T11:00:00Z");
    }

    #[tokio::test]
    async fn mining_queue_requires_review_capability() {
        let (app, auth) = app_with_store().await;
        let reviewer = bearer(&auth, "op", MINING_REVIEW);
        let reader = bearer(&auth, "op", MEMORY_READ);

        // A memory:read holder cannot list the queue.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/mining/queue")
                    .header("authorization", &reader)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        // A mining:review holder can list (empty queue).
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/mining/queue")
                    .header("authorization", &reviewer)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(body.as_array().unwrap().is_empty());
    }

    fn hit_mem(id: &str, sim: f32) -> SearchHit {
        SearchHit {
            memory: Memory {
                id: MemoryId(id.into()),
                content: id.into(),
                project: "p".into(),
                topic: "t".into(),
                source: ijima_core::MemorySource::Explicit,
                harness: ijima_core::harness::Harness::Pi,
                session_id: None,
                origin: ijima_core::InstanceId::local(),
                authority: ijima_core::AuthorityScope::local(),
                importance: 0.5,
                created_at: "0".into(),
            },
            similarity: sim,
        }
    }

    #[test]
    fn merge_search_hits_ranks_desc_dedups_and_truncates() {
        // scope=visible merge: two ranked lists combine by similarity, dedup
        // by memory id (first wins), truncate to limit.
        let a = vec![hit_mem("a", 0.9), hit_mem("b", 0.5)];
        let b = vec![hit_mem("c", 0.8), hit_mem("a", 0.7)]; // 'a' dup, lower sim
        let merged = merge_search_hits(a, b, 3);
        // Sorted by similarity desc: a(0.9), c(0.8), b(0.5) — the dup a(0.7)
        // is dropped (first wins).
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].memory.id.0, "a");
        assert_eq!((merged[0].similarity * 10.0).round() as i32, 9);
        assert_eq!(merged[1].memory.id.0, "c");
        assert_eq!(merged[2].memory.id.0, "b");
    }

    #[test]
    fn merge_search_hits_respects_limit() {
        let a = vec![hit_mem("a", 0.9), hit_mem("b", 0.8)];
        let b = vec![hit_mem("c", 0.7), hit_mem("d", 0.6)];
        let merged = merge_search_hits(a, b, 2);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].memory.id.0, "a");
        assert_eq!(merged[1].memory.id.0, "b");
    }

    #[cfg(feature = "mining")]
    #[tokio::test]
    async fn trigger_requires_mining_trigger_capability() {
        let (app, auth) = app_with_store().await;
        // A memory:write holder cannot trigger mining.
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/sess_x/mine")
                    .header("authorization", &write)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[cfg(feature = "mining")]
    #[tokio::test]
    async fn trigger_mines_decision_and_archives() {
        // Rules-only: assumes no IJIMA_LLM_* env is set (CI is clean). When
        // env is unset, `build_mining_agent` returns None and `mine_all` runs
        // the deterministic rules tier.
        let (app, auth) = app_with_store().await;
        let ingest = bearer(&auth, "elliott", SESSION_INGEST);
        let trigger = bearer(&auth, "elliott", MINING_TRIGGER);

        // Ingest a decision-bearing turn into elliott's personal namespace.
        let turn = serde_json::json!({
            "session_id": "sess_mine",
            "turn_index": 0,
            "role": "User",
            "content": "We decided to use SurrealDB for storage.",
            "timestamp": "0",
        });
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/sess_mine/turns")
                    .header("authorization", &ingest)
                    .header("content-type", "application/json")
                    .body(Body::from(turn.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Trigger mining (rules-only: no IJIMA_LLM_* env in tests).
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/sess_mine/mine")
                    .header("authorization", &trigger)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let report: crate::mining_pipeline::MiningReport = serde_json::from_slice(&body).unwrap();
        assert!(
            report.archived >= 1,
            "rules tier should archive the decision: {report:?}"
        );
    }

    // ===== Palace / diary / repo route tests (Phase B) =====

    async fn body_json(res: axum::response::Response) -> serde_json::Value {
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn seed_memory(app: &Router, auth: &IjimaAuth, id: &str, project: &str, topic: &str) {
        let body = serde_json::json!({
            "id": id,
            "content": format!("{project}/{topic} note"),
            "project": project,
            "topic": topic,
            "source": "Explicit",
            "harness": "Pi",
            "session_id": "sess_1",
            "importance": 0.5,
            "created_at": "0",
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/memories")
                    .header("authorization", bearer(auth, "elliott", MEMORY_WRITE))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "seed {id} failed");
    }

    #[tokio::test]
    async fn rooms_taxonomy_stats_reflect_seeded_memories() {
        let (app, auth) = app_with_store().await;
        seed_memory(&app, &auth, "mem_a", "ijima", "api").await;
        seed_memory(&app, &auth, "mem_b", "ijima", "auth").await;
        let read = bearer(&auth, "elliott", MEMORY_READ);

        // /rooms
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/rooms")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let rooms = body_json(res).await;
        let topics: std::collections::HashSet<&str> = rooms
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["topic"].as_str().unwrap())
            .collect();
        assert!(
            topics.contains("api") && topics.contains("auth"),
            "rooms: {rooms}"
        );

        // /memories/stats
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/memories/stats")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let stats = body_json(res).await;
        assert_eq!(stats["total"], 2, "stats: {stats}");
        assert_eq!(stats["projects"][0]["project"], "ijima");
        assert_eq!(stats["projects"][0]["count"], 2);
    }

    #[tokio::test]
    async fn browse_memories_filters_by_project() {
        let (app, auth) = app_with_store().await;
        seed_memory(&app, &auth, "mem_a", "ijima", "api").await;
        seed_memory(&app, &auth, "mem_b", "possum", "efficiency").await;
        let read = bearer(&auth, "elliott", MEMORY_READ);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/memories?project=possum")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let mems = body_json(res).await;
        let arr = mems.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["project"], "possum");
    }

    #[tokio::test]
    async fn palace_graph_and_tunnel_link_shared_topic() {
        let (app, auth) = app_with_store().await;
        seed_memory(&app, &auth, "mem_a", "ijima", "efficiency").await;
        seed_memory(&app, &auth, "mem_b", "possum", "efficiency").await;
        let read = bearer(&auth, "elliott", MEMORY_READ);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/palace/graph")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let graph = body_json(res).await;
        let projects: std::collections::HashSet<&str> = graph["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        assert!(
            projects.contains("ijima") && projects.contains("possum"),
            "graph: {graph}"
        );

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/palace/tunnel?topic=efficiency&project_a=ijima&project_b=possum")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let trav = body_json(res).await;
        assert_eq!(trav["memories_a"].as_array().unwrap().len(), 1);
        assert_eq!(trav["memories_b"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn diary_write_then_read_round_trips() {
        let (app, auth) = app_with_store().await;
        let write = bearer(&auth, "elliott", MEMORY_WRITE);
        let read = bearer(&auth, "elliott", MEMORY_READ);

        let body = serde_json::json!({
            "agent": "pi",
            "content": "shipped the routes",
            "topic": "ijima",
            "timestamp": "2026-08-09T12:00:00Z"
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/diaries")
                    .header("authorization", &write)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/diaries/pi")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let entries = body_json(res).await;
        let arr = entries.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["content"], "shipped the routes");
    }

    #[tokio::test]
    async fn diary_write_requires_memory_write_not_read() {
        let (app, auth) = app_with_store().await;
        let read = bearer(&auth, "elliott", MEMORY_READ);
        let body = serde_json::json!({"agent": "pi", "content": "x", "timestamp": "t"}).to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/diaries")
                    .header("authorization", &read)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn repo_register_list_resolve_round_trips() {
        let (app, auth) = app_with_store().await;
        let admin = bearer(&auth, "elliott", ADMIN);
        let read = bearer(&auth, "elliott", MEMORY_READ);

        // register a repo (admin)
        let body = serde_json::json!({
            "name": "Ijima",
            "path": "/home/x/Ijima",
            "remote_url": "git@github.com:Industrial-Algebra/Ijima.git",
            "role": "memory-service"
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/repos")
                    .header("authorization", &admin)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // list (memory:read)
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/repos")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let repos = body_json(res).await;
        assert_eq!(repos[0]["name"], "Ijima");
        assert_eq!(repos[0]["path"], "/home/x/Ijima");

        // resolve a cwd inside the repo (memory:read)
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/repos/resolve?cwd=/home/x/Ijima/src")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let repo = body_json(res).await;
        assert_eq!(repo["name"], "Ijima");

        // resolve a cwd in no registered repo → 404
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/repos/resolve?cwd=/nowhere/here")
                    .header("authorization", &read)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn repo_register_requires_admin() {
        let (app, auth) = app_with_store().await;
        let read = bearer(&auth, "elliott", MEMORY_READ);
        let body = serde_json::json!({
            "name": "X", "path": "/x", "remote_url": "u", "role": "r"
        })
        .to_string();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/repos")
                    .header("authorization", &read)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
}
