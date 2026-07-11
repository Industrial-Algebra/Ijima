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

/// Initializes structured logging (`tracing`) if not already installed.
/// Idempotent — safe to call from both the daemon and CLI paths.
pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("IJIMA_LOG").unwrap_or_else(|_| EnvFilter::new("ijima=info")),
        )
        .try_init();
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
    init_tracing();
    let key_path = key_store::default_key_path()?;
    let seed = key_store::load_or_create(&key_path)?;
    let auth = Arc::new(IjimaAuth::from_embedded_policy_with_seed(seed)?);

    #[cfg(feature = "embeddings-candle")]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = {
        let e: Arc<dyn ijima_core::Embedder> =
            Arc::new(crate::embeddings_candle::CandleEmbedder::from_env()?);
        tracing::info!(model = %e.model_id(), "embedder loaded");
        Some(e)
    };
    #[cfg(not(feature = "embeddings-candle"))]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = None;

    // Persistent on disk by default (SurrealKv). The data dir is
    // $IJIMA_DIR (default ~/.ijima); the store lives at ijima.db.
    let data_dir = std::env::var("IJIMA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            std::env::var_os("HOME")
                .map(|h| std::path::PathBuf::from(h).join(".ijima"))
                .ok_or_else(|| {
                    ijima_core::IjimaError::invalid_input(
                        "cannot resolve data dir: set IJIMA_DIR or HOME",
                    )
                })
        })?;
    let db_path = data_dir.join("ijima.db");

    #[cfg(feature = "embeddings-candle")]
    let store_inner = Arc::new(
        crate::SurrealStore::open_persistent_with(&db_path, embedder.clone().unwrap()).await?,
    );
    #[cfg(not(feature = "embeddings-candle"))]
    let store_inner = Arc::new(crate::SurrealStore::open_persistent(&db_path).await?);
    let store: Arc<dyn Store> = store_inner.clone();
    let kg: Arc<dyn ijima_core::KnowledgeGraph> = store_inner;

    // Schubert geometric rate limiting (Phase 3.4). Configurable via env;
    // disabled when IJIMA_RATE_DISABLE is set (tests, CI). Capacity scales
    // with the capability's Schubert intersection number (codimension).
    #[cfg(feature = "rate-limit")]
    let rate_limiter: Option<crate::rate_limit::RateLimitState> =
        if std::env::var_os("IJIMA_RATE_DISABLE").is_some() {
            None
        } else {
            let base = std::env::var("IJIMA_RATE_BASE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10.0);
            let mult = std::env::var("IJIMA_RATE_MULTIPLIER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0);
            tracing::info!(
                base_tokens_per_second = base,
                multiplier = mult,
                "rate limiting enabled (Schubert intersection-number capacity)"
            );
            Some(crate::rate_limit::make_rate_limiter(base, mult))
        };

    let app = api::app(
        auth,
        store,
        kg,
        embedder,
        std::sync::Arc::new(crate::redaction::Redactor::new()),
        #[cfg(feature = "rate-limit")]
        rate_limiter,
    );

    let addr = format!("{}:{}", config.host, config.port);

    // TLS: if both cert and key env vars are set, serve over HTTPS.
    // Plain HTTP is the default — no config = no TLS.
    #[cfg(feature = "tls")]
    if let (Some(cert_path), Some(key_path)) = (
        std::env::var_os("IJIMA_TLS_CERT"),
        std::env::var_os("IJIMA_TLS_KEY"),
    ) {
        let tls_config =
            axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                .await
                .map_err(|e| IjimaError::Store {
                    detail: format!("tls config: {e}"),
                })?;
        let listener = axum_server::bind_rustls(
            addr.parse().map_err(|e| IjimaError::Store {
                detail: format!("parse {addr}: {e}"),
            })?,
            tls_config,
        );
        eprintln!("ijima: listening on https://{addr}");
        listener
            .serve(app.into_make_service())
            .await
            .map_err(|e| IjimaError::Store {
                detail: format!("serve: {e}"),
            })?;
        return Ok(());
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("bind {addr}: {e}"),
        })?;
    tracing::info!(addr = %addr, "ijima listening");
    axum::serve(listener, app)
        .await
        .map_err(|e| IjimaError::Store {
            detail: format!("serve: {e}"),
        })?;
    Ok(())
}
