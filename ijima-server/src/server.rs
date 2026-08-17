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
        // Env > config file > built-in defaults (see `config`).
        let file = crate::config::load().unwrap_or_default();
        Self {
            host: crate::config::resolve_str("IJIMA_HOST", file.host, "127.0.0.1"),
            port: std::env::var("IJIMA_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .or(file.port)
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
/// Build the federation instance config from environment variables (the
/// `IJIMA_INSTANCE_*` family), falling back to the single-instance default.
/// Core stays env-free; this is the server-boundary binding (ADR
/// `federation-control-api`).
///
/// - `IJIMA_INSTANCE_ID` — stable instance id (default `local`).
/// - `IJIMA_INSTANCE_ROLE` — `unifying` | `archive` | `domain-authority` |
///   `edge` | `airgapped` (default `unifying`).
/// - `IJIMA_INSTANCE_SCOPES` — comma-separated `namespace:project` pairs,
///   e.g. `local:*,shared:Dominic` (default `local:*`).
/// - `IJIMA_CAPABILITY_POLICY_REF` — capability-policy hash/ref (default none).
///
/// Outbound links are not yet configurable (no peer-topology config format).
#[cfg(feature = "federation")]
fn federation_config_from_env() -> ijima_core::federation::InstanceFederationConfig {
    use ijima_core::federation::{
        AuthoritativeScope, InstanceFederationConfig, InstanceId, InstanceRole,
    };
    use std::str::FromStr;
    let instance_id = std::env::var("IJIMA_INSTANCE_ID")
        .map(InstanceId::new)
        .unwrap_or_default();
    let role = std::env::var("IJIMA_INSTANCE_ROLE")
        .ok()
        .and_then(|r| InstanceRole::from_str(&r).ok())
        .unwrap_or(InstanceRole::Unifying);
    let authoritative_scopes = std::env::var("IJIMA_INSTANCE_SCOPES")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|p| AuthoritativeScope::from_str(p.trim()).ok())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![AuthoritativeScope::new("local", "*")]);
    let capability_policy_ref = std::env::var("IJIMA_CAPABILITY_POLICY_REF")
        .ok()
        .filter(|s| !s.is_empty());
    InstanceFederationConfig {
        instance_id,
        role,
        authoritative_scopes,
        outbound_links: Vec::new(),
        capability_policy_ref,
    }
}

pub async fn serve(config: &DaemonConfig) -> Result<()> {
    init_tracing();
    // Config file layer — loaded once; a malformed file fails the daemon
    // before anything opens (explicit `$IJIMA_CONFIG` must be honored).
    let file_config = crate::config::load()?;
    let key_path = key_store::default_key_path()?;
    let seed = key_store::load_or_create(&key_path)?;
    let auth = Arc::new(IjimaAuth::from_embedded_policy_with_seed(seed)?);

    #[cfg(feature = "embeddings-candle")]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = {
        // Model resolution: env IJIMA_EMBED_MODEL > config file > default.
        let model = crate::config::resolve_str(
            "IJIMA_EMBED_MODEL",
            file_config.embedding_model.clone(),
            crate::embeddings_candle::DEFAULT_MODEL,
        );
        let revision = std::env::var("IJIMA_EMBED_REVISION").unwrap_or_else(|_| "main".into());
        let e: Arc<dyn ijima_core::Embedder> = Arc::new(
            crate::embeddings_candle::CandleEmbedder::from_hub_model(&model, &revision)?,
        );
        tracing::info!(model = %e.model_id(), "embedder loaded");
        Some(e)
    };
    #[cfg(not(feature = "embeddings-candle"))]
    let embedder: Option<Arc<dyn ijima_core::Embedder>> = None;

    // Persistent on disk by default (SurrealKv). Data dir resolution:
    // env $IJIMA_DIR > config file `data_dir` > ~/.ijima (see `config`).
    let data_dir = crate::config::resolve_data_dir()?;
    let db_path = data_dir.join("ijima.db");

    #[cfg(feature = "embeddings-candle")]
    let store_inner = Arc::new(
        crate::SurrealStore::open_persistent_with(&db_path, embedder.clone().unwrap()).await?,
    );
    #[cfg(not(feature = "embeddings-candle"))]
    let store_inner = Arc::new(crate::SurrealStore::open_persistent(&db_path).await?);
    let store: Arc<dyn Store> = store_inner.clone();
    let kg: Arc<dyn ijima_core::KnowledgeGraph> = store_inner;

    // Hydrate the grant-revocation set from the store (WS1b): any bearer
    // revoked on a previous boot stays dead across restarts.
    let revocations = store.list_revocations().await?;
    if !revocations.is_empty() {
        tracing::info!(count = revocations.len(), "hydrated token revocations");
    }
    auth.hydrate_revocations(&revocations);

    // Schubert geometric rate limiting (Phase 3.4). Configurable via env
    // or config file; disabled when IJIMA_RATE_DISABLE is set (tests, CI).
    // Capacity scales with the capability's Schubert intersection number.
    #[cfg(feature = "rate-limit")]
    let rate_limiter: Option<crate::rate_limit::RateLimitState> =
        if std::env::var_os("IJIMA_RATE_DISABLE").is_some() {
            None
        } else {
            let base = crate::config::resolve_f64("IJIMA_RATE_BASE", file_config.rate_base, 10.0);
            let mult = crate::config::resolve_f64(
                "IJIMA_RATE_MULTIPLIER",
                file_config.rate_multiplier,
                1.0,
            );
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
        #[cfg(feature = "federation")]
        std::sync::Arc::new(federation_config_from_env()),
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
