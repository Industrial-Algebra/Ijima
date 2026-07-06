// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! # ijima-client
//!
//! Thin HTTP client and harness-adapter crate for the
//! [Ijima](https://github.com/Industrial-Algebra/Ijima) centralized
//! agentic memory backend.
//!
//! Each harness (pi, Tsume, Sakamoto, Wallace, opencode, ...) depends
//! on this crate instead of re-implementing its own bridge. The client
//! translates the harness's native memory calls into Ijima API calls
//! over HTTP/JSON. It replaces the fragile per-harness bridge-script
//! anti-pattern documented in `docs/HANDOFF.md` §2.2.
//!
//! ## Features
//!
//! - `std` (default): Standard library support.
//! - `remote` (default): reqwest-based HTTP client speaking the Ijima
//!   API. Disable to embed a no-op stub (useful in tests or offline
//!   builds).
//!
//! ## Quick start
//!
//! ```no_run
//! # #[tokio::main] async fn main() -> ijima_core::Result<()> {
//! use ijima_client::{Client, ClientConfig};
//! use ijima_core::{harness::Harness, Memory, MemoryId, MemorySource};
//!
//! let mut cfg = ClientConfig::new("http://127.0.0.1:7373", Harness::Pi);
//! cfg.token = Some("Bearer ...".into());
//! let client = Client::new(cfg);
//!
//! client.store_memory(Memory {
//!     id: MemoryId("m1".into()),
//!     content: "decided to use SurrealDB".into(),
//!     project: "ijima".into(),
//!     topic: "storage".into(),
//!     source: MemorySource::Explicit,
//!     harness: Harness::Pi,
//!     session_id: None,
//!     importance: 0.5,
//!     created_at: String::new(),
//! }).await?;
//!
//! let hits = client.search_memories("database choice", Some(5), None).await?;
//! println!("{} matches", hits.len());
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

use ijima_core::harness::Harness;
use ijima_core::{IjimaError, Memory, Result, SessionTurn};
use serde::Deserialize;

/// Configuration for connecting to an Ijima server.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Base URL of the Ijima daemon, e.g. `http://ijima.tailnet:7373`.
    pub base_url: String,
    /// Optional bearer token (`"Bearer <capability-token>"`).
    pub token: Option<String>,
    /// The harness this client identifies as in provenance fields.
    pub harness: Harness,
}

impl ClientConfig {
    /// Builds a config pointing at `base_url` for the given `harness`,
    /// with no bearer token.
    #[must_use]
    pub fn new(base_url: impl Into<String>, harness: Harness) -> Self {
        Self {
            base_url: base_url.into(),
            token: None,
            harness,
        }
    }

    /// Sets the bearer token (returns `self` for chaining).
    #[must_use]
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }
}

/// A typed Ijima API client. Each method maps one-to-one onto an Ijima
/// HTTP route.
#[derive(Debug)]
pub struct Client {
    config: ClientConfig,
    #[cfg(feature = "remote")]
    http: reqwest::Client,
}

// ---------- response wrappers ----------

#[derive(Deserialize)]
struct IdResponse {
    id: String,
}

#[derive(Deserialize)]
struct MemoriesResponse {
    memories: Vec<Memory>,
}

#[derive(Deserialize)]
struct TurnsResponse {
    turns: Vec<SessionTurn>,
}

/// The result of a promotion (personal → shared, redacted).
#[derive(Debug, Clone, Deserialize)]
pub struct Promotion {
    /// The id of the promoted copy in the shared namespace.
    pub id: String,
    /// The original personal memory's id.
    pub original_id: String,
    /// The shared namespace the copy was written to.
    pub target_namespace: String,
    /// Which redaction categories fired.
    pub redactions: Vec<RedactionSummary>,
}

/// One redaction category that fired during promotion.
#[derive(Debug, Clone, Deserialize)]
pub struct RedactionSummary {
    pub category: String,
    pub count: usize,
}

/// The wake-up composition: L0 identity + L1a personal + L1b doctrine.
#[derive(Debug, Clone, Deserialize)]
pub struct Wakeup {
    pub identity: serde_json::Value,
    pub personal_essentials: Vec<Memory>,
    pub doctrine: Vec<Memory>,
}

impl Client {
    /// Creates a new client bound to the given configuration. The HTTP
    /// connection is established lazily on the first request.
    #[must_use]
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "remote")]
            http: reqwest::Client::new(),
        }
    }

    /// Returns the configuration this client was built from.
    #[must_use]
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Confirms the server is reachable (`GET /health`).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] if the health check fails.
    #[cfg(feature = "remote")]
    pub async fn health(&self) -> Result<()> {
        let resp = self.get("/health").await?;
        ok_status(resp).await?;
        Ok(())
    }

    /// Stores a memory (`POST /memories`). Returns the stored id.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn store_memory(&self, memory: Memory) -> Result<String> {
        let resp = self.post("/memories", &memory).await?;
        let r: IdResponse = decode(ok_status(resp).await?).await?;
        Ok(r.id)
    }

    /// Recalls a memory by id (`GET /memories/:id`). Pass `namespace`
    /// to read from a shared namespace; omit for the caller's personal
    /// namespace. Returns `None` if the memory is absent (404).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on non-404 HTTP failures.
    #[cfg(feature = "remote")]
    pub async fn recall_memory(&self, id: &str, namespace: Option<&str>) -> Result<Option<Memory>> {
        let path = build_path(&format!("/memories/{id}"), namespace, None);
        let resp = self.get(&path).await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let m: Memory = decode(ok_status(resp).await?).await?;
        Ok(Some(m))
    }

    /// Deletes a memory by id (`DELETE /memories/:id`).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn delete_memory(&self, id: &str, namespace: Option<&str>) -> Result<()> {
        let path = build_path(&format!("/memories/{id}"), namespace, None);
        let resp = self.delete(&path).await?;
        ok_status(resp).await?;
        Ok(())
    }

    /// Semantic search (`POST /memories/search`). The daemon embeds the
    /// query text centrally. Pass `namespace` to search a shared scope.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn search_memories(
        &self,
        text: &str,
        limit: Option<usize>,
        namespace: Option<&str>,
    ) -> Result<Vec<Memory>> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            text: &'a str,
            limit: Option<usize>,
        }
        let path = build_path("/memories/search", namespace, None);
        let resp = self.post(&path, &Req { text, limit }).await?;
        let r: MemoriesResponse = decode(ok_status(resp).await?).await?;
        Ok(r.memories)
    }

    /// Promotes a personal memory to a shared namespace
    /// (`POST /memories/:id/promote`), running the redaction filter at
    /// the boundary.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn promote_memory(
        &self,
        id: &str,
        target_namespace: &str,
        new_id: Option<&str>,
    ) -> Result<Promotion> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            target_namespace: &'a str,
            new_id: Option<&'a str>,
        }
        let resp = self
            .post(
                &format!("/memories/{id}/promote"),
                &Req {
                    target_namespace,
                    new_id,
                },
            )
            .await?;
        let p: Promotion = decode(ok_status(resp).await?).await?;
        Ok(p)
    }

    /// Appends a turn to a session transcript
    /// (`POST /sessions/:session_id/turns`).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn ingest_turn(&self, session_id: &str, turn: SessionTurn) -> Result<()> {
        let resp = self
            .post(&format!("/sessions/{session_id}/turns"), &turn)
            .await?;
        ok_status(resp).await?;
        Ok(())
    }

    /// Reads the last turns of a session
    /// (`GET /sessions/:session_id/turns`).
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn session_turns(
        &self,
        session_id: &str,
        namespace: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SessionTurn>> {
        let path = build_path(&format!("/sessions/{session_id}/turns"), namespace, limit);
        let resp = self.get(&path).await?;
        let r: TurnsResponse = decode(ok_status(resp).await?).await?;
        Ok(r.turns)
    }

    /// Composes the session-start context (`GET /wakeup`): L0 identity +
    /// L1a personal essentials + L1b team doctrine.
    ///
    /// # Errors
    ///
    /// Returns [`IjimaError::Transport`] on any HTTP failure.
    #[cfg(feature = "remote")]
    pub async fn wakeup(&self) -> Result<Wakeup> {
        let resp = self.get("/wakeup").await?;
        let w: Wakeup = decode(ok_status(resp).await?).await?;
        Ok(w)
    }

    // ---------- HTTP plumbing ----------

    #[cfg(feature = "remote")]
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url.trim_end_matches('/'), path)
    }

    #[cfg(feature = "remote")]
    fn add_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.config.token {
            Some(t) => {
                // Accept either a bare token (from `ijima token issue`)
                // or a pre-formatted "Bearer ..." value.
                let val = if t.starts_with("Bearer ") {
                    t.clone()
                } else {
                    format!("Bearer {t}")
                };
                req.header(reqwest::header::AUTHORIZATION, val)
            }
            None => req,
        }
    }

    #[cfg(feature = "remote")]
    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        self.add_auth(self.http.get(self.url(path)))
            .send()
            .await
            .map_err(transport)
    }

    #[cfg(feature = "remote")]
    async fn delete(&self, path: &str) -> Result<reqwest::Response> {
        self.add_auth(self.http.delete(self.url(path)))
            .send()
            .await
            .map_err(transport)
    }

    #[cfg(feature = "remote")]
    async fn post<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<reqwest::Response> {
        self.add_auth(self.http.post(self.url(path)).json(body))
            .send()
            .await
            .map_err(transport)
    }
}

#[cfg(feature = "remote")]
async fn ok_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        Ok(resp)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(IjimaError::Transport {
            detail: format!("HTTP {status}: {body}"),
        })
    }
}

#[cfg(feature = "remote")]
async fn decode<T: serde::de::DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    resp.json::<T>().await.map_err(|e| IjimaError::Transport {
        detail: format!("decode: {e}"),
    })
}

#[cfg(feature = "remote")]
fn transport(e: reqwest::Error) -> IjimaError {
    IjimaError::Transport {
        detail: e.to_string(),
    }
}

/// Builds a path with optional `namespace` + `limit` query params.
fn build_path(base: &str, namespace: Option<&str>, limit: Option<usize>) -> String {
    let mut params: Vec<String> = Vec::new();
    if let Some(ns) = namespace {
        params.push(format!("namespace={ns}"));
    }
    if let Some(l) = limit {
        params.push(format!("limit={l}"));
    }
    if params.is_empty() {
        base.to_string()
    } else {
        format!("{}?{}", base, params.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ijima_core::MemoryId;

    #[test]
    fn config_records_harness_and_url() {
        let cfg = ClientConfig::new("http://ijima.tailnet:7373", Harness::Pi);
        assert_eq!(cfg.base_url, "http://ijima.tailnet:7373");
        assert_eq!(cfg.harness, Harness::Pi);
        assert!(cfg.token.is_none());
    }

    #[test]
    fn with_token_sets_token() {
        let cfg = ClientConfig::new("http://x", Harness::Pi).with_token("Bearer abc");
        assert_eq!(cfg.token.as_deref(), Some("Bearer abc"));
    }

    #[test]
    fn build_path_no_params() {
        assert_eq!(build_path("/memories/m1", None, None), "/memories/m1");
    }

    #[test]
    fn build_path_namespace_only() {
        assert_eq!(
            build_path("/memories/m1", Some("ns_team"), None),
            "/memories/m1?namespace=ns_team"
        );
    }

    #[test]
    fn build_path_namespace_and_limit() {
        assert_eq!(
            build_path("/sessions/s1/turns", Some("ns_team"), Some(5)),
            "/sessions/s1/turns?namespace=ns_team&limit=5"
        );
    }

    #[test]
    fn build_path_limit_only() {
        assert_eq!(
            build_path("/sessions/s1/turns", None, Some(10)),
            "/sessions/s1/turns?limit=10"
        );
    }

    /// End-to-end client test against a running daemon. `#[ignore]` —
    /// run with:
    ///   `IJIMA_TEST_URL=http://127.0.0.1:7373 cargo test --features remote -- --ignored daemon`
    #[cfg(feature = "remote")]
    #[tokio::test]
    #[ignore]
    async fn daemon_round_trip() {
        let url = std::env::var("IJIMA_TEST_URL").expect("set IJIMA_TEST_URL");
        let token = std::env::var("IJIMA_TEST_TOKEN").expect("set IJIMA_TEST_TOKEN");
        let client = Client::new(ClientConfig::new(url, Harness::Pi).with_token(token));

        client.health().await.expect("health");

        let id = client
            .store_memory(Memory {
                id: MemoryId("client_test_1".into()),
                content: "client round-trip test".into(),
                project: "ijima".into(),
                topic: "client".into(),
                source: ijima_core::memory::MemorySource::Explicit,
                harness: Harness::Pi,
                session_id: None,
                importance: 0.5,
                created_at: String::new(),
            })
            .await
            .expect("store");
        assert_eq!(id, "client_test_1");

        let got = client
            .recall_memory("client_test_1", None)
            .await
            .expect("recall");
        assert_eq!(got.expect("present").content, "client round-trip test");

        client
            .delete_memory("client_test_1", None)
            .await
            .expect("delete");
    }
}
