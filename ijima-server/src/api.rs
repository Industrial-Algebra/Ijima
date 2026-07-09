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

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use ijima_core::{
    Embedder, EntityId, KnowledgeGraph, Memory, MemoryId, NamespaceCount, Session, SessionId,
    SessionTurn, Store,
    capabilities::{KNOWLEDGE_READ, MEMORY_READ, MEMORY_WRITE, SESSION_INGEST},
    harness::Harness,
};

use crate::extractor::AuthPrincipal;
use crate::redaction::Redactor;

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
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/memories", post(store_memory))
        .route("/memories/check", post(check_duplicate))
        .route("/memories/search", post(search_memories))
        .route("/memories/:id", get(recall_memory).delete(delete_memory))
        .route("/memories/:id/promote", post(promote_memory))
        .route("/doctrine", post(ingest_doctrine))
        .route("/wakeup", get(wakeup))
        .route("/kg/triples", post(add_triple).get(find_triples))
        .route("/kg/entities/:id", get(query_entity))
        .route("/kg/triples/:id/invalidate", post(invalidate_triple))
        .route("/kg/timeline", get(kg_timeline))
        .route("/kg/stats", get(kg_stats))
        .route(
            "/sessions/:session_id/turns",
            post(ingest_turn).get(session_turns),
        )
        .route("/sessions", post(create_session).get(list_sessions))
        .route("/sessions/:session_id/end", post(end_session))
        .layer(Extension(auth))
        .layer(Extension(store))
        .layer(Extension(kg))
        .layer(Extension(embedder))
        .layer(Extension(redactor))
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

#[derive(Serialize)]
struct StatusResponse {
    memories: usize,
    namespaces: Vec<NamespaceCount>,
    entities: usize,
    triples: usize,
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
    Ok(Json(StatusResponse {
        memories: store_stats.total_memories,
        namespaces: store_stats.namespaces,
        entities: kg_stats.entities,
        triples: kg_stats.triples,
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
}

#[derive(Serialize)]
struct SearchResponse {
    memories: Vec<Memory>,
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
    let ns = resolve_ns(&principal, q.namespace.as_deref())?;
    let query = embedder.embed(&req.text).map_err(internal)?;
    let hits = store
        .search_memories(&ns, &query, req.limit.unwrap_or(10))
        .await
        .map_err(internal)?;
    Ok(Json(SearchResponse { memories: hits }))
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
    if !principal.0.may(MEMORY_WRITE) {
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
                    .header("authorization", &write)
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
}
