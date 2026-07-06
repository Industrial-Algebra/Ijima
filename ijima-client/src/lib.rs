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
//! ## Status
//!
//! Scaffolded — the typed request/response methods land test-first.

#![forbid(unsafe_code)]

use ijima_core::{Result, harness::Harness};

/// Configuration for connecting to an Ijima server.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Base URL of the Ijima daemon, e.g. `http://ijima.tailnet:7373`.
    pub base_url: String,
    /// Optional bearer token (matches the pi-mempalace `PI_MEMPALACE_TOKEN`
    /// convention).
    pub token: Option<String>,
    /// The harness this client identifies as in provenance fields.
    pub harness: Harness,
}

impl ClientConfig {
    /// Builds a config pointing at `base_url` for the given `harness`,
    /// with no bearer token.
    pub fn new(base_url: impl Into<String>, harness: Harness) -> Self {
        Self {
            base_url: base_url.into(),
            token: None,
            harness,
        }
    }
}

/// A typed Ijima API client.
///
/// Construct with [`Client::connect`], then call the memory/knowledge/
/// session methods (added test-first). Each method maps one-to-one onto
/// an Ijima HTTP route.
#[derive(Debug)]
pub struct Client {
    config: ClientConfig,
}

impl Client {
    /// Creates a new client bound to the given configuration.
    ///
    /// This does not perform any I/O; the underlying HTTP connection is
    /// established lazily on the first request.
    pub fn new(config: ClientConfig) -> Self {
        Self { config }
    }

    /// Returns the configuration this client was built from.
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Connects to the server and confirms it is reachable.
    ///
    /// # Errors
    ///
    /// Returns [`ijima_core::IjimaError::Transport`] if the health check
    /// fails.
    pub async fn connect(&self) -> Result<()> {
        // TDD: first failing test stands up a mock server and asserts
        // connect() succeeds against `/health`.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_records_harness_and_url() {
        let cfg = ClientConfig::new("http://ijima.tailnet:7373", Harness::Pi);
        assert_eq!(cfg.base_url, "http://ijima.tailnet:7373");
        assert_eq!(cfg.harness, Harness::Pi);
        assert!(cfg.token.is_none());
    }
}
