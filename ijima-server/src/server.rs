// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! Daemon wiring: construct the auth core + store, build the router,
//! and bind/listen.

use std::sync::Arc;

use ijima_core::{IjimaError, Result, Store};

use crate::IjimaAuth;
use crate::api;
use crate::key_store;

/// Daemon configuration, resolved from env vars or CLI args.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Bind host. Default `127.0.0.1` (`IJIMA_HOST`).
    pub host: String,
    /// Bind port. Default `7373` (`IJIMA_PORT`).
    pub port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: std::env::var("IJIMA_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: std::env::var("IJIMA_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(7373),
        }
    }
}

/// Runs the Ijima HTTP daemon: loads the issuer key (creating it on first
/// run), opens the embedded SurrealDB store, builds the router, and
/// serves until interrupted.
///
/// # Errors
///
/// Returns [`IjimaError`] if the key, store, or socket cannot be opened.
pub async fn serve(config: &DaemonConfig) -> Result<()> {
    let key_path = key_store::default_key_path()?;
    let seed = key_store::load_or_create(&key_path)?;
    let auth = Arc::new(IjimaAuth::from_embedded_policy_with_seed(seed)?);

    #[cfg(feature = "embeddings-candle")]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = {
        let e: Arc<dyn ijima_core::Embedder> =
            Arc::new(crate::embeddings_candle::CandleEmbedder::from_hub()?);
        Some(e)
    };
    #[cfg(not(feature = "embeddings-candle"))]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = None;

    #[cfg(feature = "embeddings-candle")]
    let store: Arc<dyn Store> =
        { Arc::new(crate::SurrealStore::open_embedded_with(embedder.clone().unwrap()).await?) };
    #[cfg(not(feature = "embeddings-candle"))]
    let store: Arc<dyn Store> = Arc::new(crate::SurrealStore::open_embedded().await?);

    let app = api::app(
        auth,
        store,
        embedder,
        std::sync::Arc::new(crate::redaction::Redactor::new()),
    );

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("bind {addr}: {e}"),
        })?;
    eprintln!("ijima: listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("serve: {e}"),
        })?;
    Ok(())
}
