// Copyright (C) 2026 Industrial Algebra
// SPDX-License-Identifier: Apache-2.0

//! `ijima` — the Ijima daemon and admin CLI.
//!
//! Today: `ijima token issue` mints a Schubert capability token from the
//! persistent issuer key (see [`ijima_server::key_store`]). The HTTP
//! daemon (`ijima serve`) lands once the store + auth HTTP routes are
//! wired.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

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
}

/// Arguments to `ijima export`.
#[derive(Args, Debug)]
struct ExportArgs {
    /// Output path for the SurrealDB SQL dump.
    #[arg(long, short)]
    out: std::path::PathBuf,
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
    /// Issue a bearer capability token for a principal.
    Issue(IssueArgs),
}

#[derive(Args)]
struct IssueArgs {
    /// The principal to issue the token to (e.g. `elliott`, `tsume-discord`).
    #[arg(long)]
    principal: String,
    /// The capability to grant. One of the Ijima vocabulary
    /// (memory:read, memory:write, ...). See `ijima-core::capabilities`.
    #[arg(long)]
    capability: String,
    /// Path to the issuer key file. Defaults to `$IJIMA_DIR/issuer.key`
    /// or `~/.ijima/issuer.key`. Created with a fresh seed on first use.
    #[arg(long, value_name = "PATH")]
    key_file: Option<PathBuf>,
    /// Emit a JSON object (token, principal, capability, public_key)
    /// instead of just the bearer string.
    #[arg(long)]
    json: bool,
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
    }
}

fn run_issue(args: IssueArgs) -> ijima_core::Result<()> {
    validate_capability(&args.capability)?;

    let key_path = match args.key_file {
        Some(p) => p,
        None => key_store::default_key_path()?,
    };
    let seed = key_store::load_or_create(&key_path)?;
    let auth = IjimaAuth::from_embedded_policy_with_seed(seed)?;
    let token = auth.issue_bearer(args.principal.as_str(), &args.capability)?;
    let public_key = auth.issuer_public_key_hex();

    if args.json {
        println!(
            "{{\"token\":\"{token}\",\"principal\":\"{}\",\"capability\":\"{}\",\"public_key\":\"{public_key}\"}}",
            args.principal, args.capability
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
    let store = ijima_server::SurrealStore::open_persistent(&db_path).await?;
    store.export_to(&args.out).await?;
    eprintln!("ijima: exported to {}", args.out.display());
    Ok(())
}
