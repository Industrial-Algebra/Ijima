// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! `ijima` — the Ijima daemon and admin CLI.
//!
//! Today: `ijima token issue` mints a Schubert grant token from the
//! persistent issuer key (see [`ijima_server::key_store`]). The HTTP
//! daemon (`ijima serve`) lands once the store + auth HTTP routes are
//! wired.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[cfg(feature = "backend-sqlite")]
use ijima_core::NamespaceId;
use ijima_core::capabilities::ALL_CAPABILITIES;
use ijima_server::{IjimaAuth, key_store};

#[derive(Parser)]
#[command(
    name = "ijima",
    version,
    about = "Ijima — centralized agentic memory backend (admin CLI)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Mint and inspect Schubert capability tokens.
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Ingest doctrine from a seed-pack directory into a running daemon.
    Doctrine {
        #[command(subcommand)]
        action: DoctrineAction,
    },
    /// Run the HTTP daemon.
    Serve(ServeArgs),
    /// Export the SurrealDB store as a SQL dump.
    Export(ExportArgs),
    /// Migrate the legacy pi-mempalace / ZeroClaw SQLite corpora into the
    /// SurrealDB store (one-time import).
    #[cfg(feature = "backend-sqlite")]
    Migrate(MigrateArgs),
}

/// Arguments to `ijima export`.
#[derive(Args, Debug)]
struct ExportArgs {
    /// Output path for the SurrealDB SQL dump.
    #[arg(long, short)]
    out: std::path::PathBuf,
}

/// Arguments to `ijima migrate`.
#[derive(Args, Debug)]
struct MigrateArgs {
    /// pi-mempalace `memories.db` path (imports memories + KG).
    #[arg(long, value_name = "PATH")]
    palace: Option<std::path::PathBuf>,
    /// ZeroClaw `brain.db` path (imports Sara's Discord memories).
    #[arg(long, value_name = "PATH")]
    brain: Option<std::path::PathBuf>,
    /// Re-embed every imported memory with the candle embedder at write
    /// time (slow for large corpora; without it, search is unavailable
    /// until a later re-embed pass).
    #[arg(long)]
    embed: bool,
    /// Target namespace for imported memories (default: `global`, the
    /// pi-mempalace commons). Use a principal's private namespace
    /// (e.g. `ns_elliott_private`) so migrated history lives where new
    /// pi writes land.
    #[arg(long, value_name = "NS", default_value = "global")]
    namespace: String,
}

#[derive(Subcommand)]
enum DoctrineAction {
    /// Read `*.md` files from a directory and POST them as doctrine
    /// entries to a daemon's `/doctrine` endpoint.
    Ingest(IngestArgs),
}

#[derive(Args)]
struct IngestArgs {
    /// Directory containing `*.md` doctrine files (frontmatter + body).
    #[arg(long, value_name = "DIR")]
    dir: PathBuf,
    /// Daemon base URL (e.g. `http://127.0.0.1:7373`).
    #[arg(long, value_name = "URL")]
    url: String,
    /// Admin bearer token (`ijima token issue --capability admin`).
    #[arg(long, value_name = "TOKEN")]
    token: String,
}

#[derive(Args)]
struct ServeArgs {
    /// Bind host (default: $IJIMA_HOST or 127.0.0.1).
    #[arg(long)]
    host: Option<String>,
    /// Bind port (default: $IJIMA_PORT or 7373).
    #[arg(long)]
    port: Option<u16>,
}

#[derive(Subcommand)]
enum TokenAction {
    /// Issue a bearer grant token for a principal.
    Issue(IssueArgs),
    /// Revoke a grant token on a running daemon (the kill-switch).
    /// Requires an admin bearer.
    Revoke(RevokeArgs),
    /// List recorded token revocations on a running daemon.
    Revocations(RevocationsArgs),
}

#[derive(Args)]
struct IssueArgs {
    /// The principal to issue the token to (e.g. `elliott`, `tsume-discord`).
    #[arg(long)]
    principal: String,
    /// The single capability to grant (use `--capabilities` for a
    /// multi-capability grant token). One of the Ijima vocabulary
    /// (memory:read, memory:write, ...). See `ijima-core::capabilities`.
    #[arg(long)]
    capability: Option<String>,
    /// Comma-separated capabilities for a multi-capability grant token,
    /// e.g. `memory:read,memory:write,knowledge:read`. Exactly one of
    /// `--capability` / `--capabilities` is required.
    #[arg(long, value_name = "CSV")]
    capabilities: Option<String>,
    /// Path to the issuer key file. Defaults to `$IJIMA_DIR/issuer.key`
    /// or `~/.ijima/issuer.key`. Created with a fresh seed on first use.
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,
    /// Emit a JSON object (token, principal, capabilities, public_key)
    /// instead of just the bearer string.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RevokeArgs {
    /// The bearer token to revoke — raw or full `Bearer ...` form.
    #[arg(long)]
    token: String,
    /// Daemon base URL.
    #[arg(long, default_value = "http://127.0.0.1:7373")]
    url: String,
    /// Admin bearer token.
    #[arg(long)]
    auth: String,
    /// Operator note recorded with the revocation (e.g. `"leaked in CI log"`).
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Args)]
struct RevocationsArgs {
    /// Daemon base URL.
    #[arg(long, default_value = "http://127.0.0.1:7373")]
    url: String,
    /// Admin bearer token.
    #[arg(long)]
    auth: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    ijima_server::server::init_tracing();
    match cli.command {
        Command::Token { action } => match action {
            TokenAction::Issue(args) => match run_issue(args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!(error = %e, "token issue failed");
                    ExitCode::FAILURE
                }
            },
            TokenAction::Revoke(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(run_revoke(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "token revoke failed");
                        ExitCode::FAILURE
                    }
                }
            }
            TokenAction::Revocations(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(run_revocations(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "token revocations failed");
                        ExitCode::FAILURE
                    }
                }
            }
        },
        Command::Doctrine { action } => match action {
            DoctrineAction::Ingest(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match rt {
                    Ok(rt) => match rt.block_on(run_doctrine_ingest(args)) {
                        Ok(n) => {
                            tracing::info!(entries = n, "doctrine ingested");
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "doctrine ingest failed");
                            ExitCode::FAILURE
                        }
                    },
                    Err(e) => {
                        tracing::error!(error = %e, "runtime build failed");
                        ExitCode::FAILURE
                    }
                }
            }
        },
        Command::Serve(args) => {
            let mut config = ijima_server::server::DaemonConfig::default();
            if let Some(h) = args.host {
                config.host = h;
            }
            if let Some(p) = args.port {
                config.port = p;
            }
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build();
            match rt {
                Ok(rt) => match rt.block_on(ijima_server::server::serve(&config)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "serve failed");
                        ExitCode::FAILURE
                    }
                },
                Err(e) => {
                    tracing::error!(error = %e, "runtime build failed");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Export(args) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match rt {
                Ok(rt) => match rt.block_on(run_export(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("ijima: {e}");
                        ExitCode::FAILURE
                    }
                },
                Err(e) => {
                    eprintln!("ijima: runtime: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        #[cfg(feature = "backend-sqlite")]
        Command::Migrate(args) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match rt {
                Ok(rt) => match rt.block_on(run_migrate(args)) {
                    Ok(report) => {
                        tracing::info!(
                            attempted = report.attempted,
                            imported = report.imported,
                            skipped = report.skipped,
                            "migration complete"
                        );
                        eprintln!(
                            "ijima: migrated {} imported, {} skipped (of {} attempted)",
                            report.imported, report.skipped, report.attempted
                        );
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("ijima: {e}");
                        ExitCode::FAILURE
                    }
                },
                Err(e) => {
                    eprintln!("ijima: runtime: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn run_issue(args: IssueArgs) -> ijima_core::Result<()> {
    // Exactly one of --capability / --capabilities.
    let caps: Vec<String> = match (args.capability.as_deref(), args.capabilities.as_deref()) {
        (Some(_), Some(_)) => {
            return Err(ijima_core::IjimaError::invalid_input(
                "pass either --capability or --capabilities, not both",
            ));
        }
        (Some(c), None) => vec![c.to_string()],
        (None, Some(csv)) => csv
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        (None, None) => {
            return Err(ijima_core::IjimaError::invalid_input(
                "missing required --capability (or --capabilities for a multi-cap grant)",
            ));
        }
    };
    for cap in &caps {
        validate_capability(cap)?;
    }
    let cap_refs: Vec<&str> = caps.iter().map(String::as_str).collect();

    let key_path = match args.key_file {
        Some(p) => p,
        None => key_store::default_key_path()?,
    };
    let seed = key_store::load_or_create(&key_path)?;
    let auth = IjimaAuth::from_embedded_policy_with_seed(seed)?;
    let token = auth.issue_grant_bearer(args.principal.as_str(), &cap_refs)?;
    let public_key = auth.issuer_public_key_hex();

    if args.json {
        let caps_json = caps.join(",");
        println!(
            "{{\"token\":\"{token}\",\"principal\":\"{}\",\"capabilities\":\"{caps_json}\",\"public_key\":\"{public_key}\"}}",
            args.principal
        );
    } else {
        println!("{token}");
    }
    Ok(())
}

fn validate_capability(cap: &str) -> ijima_core::Result<()> {
    if ALL_CAPABILITIES.contains(&cap) {
        Ok(())
    } else {
        Err(ijima_core::IjimaError::invalid_input(format!(
            "unknown capability '{cap}'. Valid: {}",
            ALL_CAPABILITIES.join(", ")
        )))
    }
}

/// `ijima token revoke` — kills a bearer on a running daemon (admin).
async fn run_revoke(args: RevokeArgs) -> ijima_core::Result<()> {
    let client = reqwest::Client::new();
    let endpoint = format!("{}/tokens/revoke", args.url.trim_end_matches('/'));
    let resp = client
        .post(&endpoint)
        .bearer_auth(args.auth.trim())
        .json(&serde_json::json!({
            "token": args.token,
            "reason": args.reason,
        }))
        .send()
        .await
        .map_err(|e| ijima_core::IjimaError::Transport {
            detail: format!("revoke: {e}"),
        })?;
    match resp.status() {
        reqwest::StatusCode::NO_CONTENT => {
            eprintln!(
                "ijima: token revoked (hash {}).",
                ijima_server::auth::bearer_hash(&args.token)
            );
            Ok(())
        }
        reqwest::StatusCode::FORBIDDEN => Err(ijima_core::IjimaError::invalid_input(
            "daemon rejected: the --auth token does not carry admin",
        )),
        status => Err(ijima_core::IjimaError::Transport {
            detail: format!("daemon returned {status}"),
        }),
    }
}

/// `ijima token revocations` — lists the kill-switch ledger (admin).
async fn run_revocations(args: RevocationsArgs) -> ijima_core::Result<()> {
    let client = reqwest::Client::new();
    let endpoint = format!("{}/tokens/revocations", args.url.trim_end_matches('/'));
    let resp = client
        .get(&endpoint)
        .bearer_auth(args.auth.trim())
        .send()
        .await
        .map_err(|e| ijima_core::IjimaError::Transport {
            detail: format!("revocations: {e}"),
        })?;
    match resp.status() {
        reqwest::StatusCode::OK => {
            let revs: Vec<ijima_core::TokenRevocation> =
                resp.json()
                    .await
                    .map_err(|e| ijima_core::IjimaError::Transport {
                        detail: format!("decode: {e}"),
                    })?;
            if revs.is_empty() {
                eprintln!("ijima: no revocations recorded.");
            }
            for r in revs {
                let reason = r.reason.as_deref().unwrap_or("-");
                eprintln!("{}\t{}\t{}", r.revoked_at_unix, r.token_hash, reason);
            }
            Ok(())
        }
        reqwest::StatusCode::FORBIDDEN => Err(ijima_core::IjimaError::invalid_input(
            "daemon rejected: the --auth token does not carry admin",
        )),
        status => Err(ijima_core::IjimaError::Transport {
            detail: format!("daemon returned {status}"),
        }),
    }
}

async fn run_doctrine_ingest(args: IngestArgs) -> ijima_core::Result<usize> {
    let entries = ijima_server::doctrine::read_doctrine_dir(&args.dir)?;
    if entries.is_empty() {
        tracing::warn!(dir = %args.dir.display(), "no *.md doctrine files found");
        return Ok(0);
    }
    tracing::info!(entries = entries.len(), "ingesting doctrine");
    let parsed: Vec<_> = entries.iter().map(|(_, e)| e.clone()).collect();
    ijima_server::doctrine::ingest_to_daemon(&args.url, &args.token, &parsed).await
}

async fn run_export(args: ExportArgs) -> ijima_core::Result<()> {
    let data_dir = ijima_server::config::resolve_data_dir()?;
    let db_path = data_dir.join("ijima.db");
    let store = ijima_server::SurrealStore::open_persistent(&db_path).await?;
    store.export_to(&args.out).await?;
    eprintln!("ijima: exported to {}", args.out.display());
    Ok(())
}

/// One-time corpus migration: read the legacy SQLite stores and import
/// them into the SurrealDB palace under the `global` namespace.
#[cfg(feature = "backend-sqlite")]
async fn run_migrate(
    args: MigrateArgs,
) -> ijima_core::Result<ijima_server::migration::ImportReport> {
    use ijima_server::migration::{
        import_memories, map_pipalace_memory, map_zeroclaw_memory, read_pipalace_memories,
        read_zeroclaw_memories,
    };

    if args.palace.is_none() && args.brain.is_none() {
        return Err(ijima_core::IjimaError::invalid_input(
            "migrate needs at least one of --palace <memories.db> or --brain <brain.db>",
        ));
    }

    let data_dir = ijima_server::config::resolve_data_dir()?;
    let db_path = data_dir.join("ijima.db");

    // Open the store, optionally with the candle embedder so imported
    // memories are embedded at write time (slow for large corpora).
    #[cfg(feature = "embeddings-candle")]
    let store = if args.embed {
        let embedder: std::sync::Arc<dyn ijima_core::Embedder> =
            std::sync::Arc::new(ijima_server::embeddings_candle::CandleEmbedder::from_env()?);
        ijima_server::SurrealStore::open_persistent_with(&db_path, embedder).await?
    } else {
        ijima_server::SurrealStore::open_persistent(&db_path).await?
    };
    #[cfg(not(feature = "embeddings-candle"))]
    let store = ijima_server::SurrealStore::open_persistent(&db_path).await?;

    let ns = NamespaceId::new(&args.namespace);
    let mut all: Vec<ijima_core::Memory> = Vec::new();

    if let Some(palace) = &args.palace {
        let rows = read_pipalace_memories(&palace.to_string_lossy())?;
        eprintln!("ijima: read {} rows from {}", rows.len(), palace.display());
        all.extend(rows.iter().map(map_pipalace_memory));
    }
    if let Some(brain) = &args.brain {
        let rows = read_zeroclaw_memories(&brain.to_string_lossy())?;
        eprintln!("ijima: read {} rows from {}", rows.len(), brain.display());
        all.extend(rows.iter().map(map_zeroclaw_memory));
    }

    eprintln!(
        "ijima: importing {} memories into namespace `{}`…",
        all.len(),
        ns.as_str()
    );
    let report = import_memories(&store, &ns, all).await?;
    Ok(report)
}
