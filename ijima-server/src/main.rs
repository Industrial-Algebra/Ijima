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
    /// Manage shared-namespace membership (WS3 org walls) on a running
    /// daemon. Requires an admin bearer.
    Namespace {
        #[command(subcommand)]
        action: NamespaceAction,
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
    /// Import an external SQLite corpus into a running daemon over HTTP
    /// (WS2 multi-source import: per-source namespace, provenance
    /// tagging, dedup pre-checks).
    #[cfg(feature = "backend-sqlite")]
    Import(ImportArgs),
}

/// Arguments to `ijima export`.
#[derive(Args, Debug)]
struct ExportArgs {
    /// Output path for the SurrealDB SQL dump.
    #[arg(long, short)]
    out: std::path::PathBuf,
}

/// Which SQLite corpus `ijima import` reads.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum ImportKind {
    /// pi-mempalace `memories.db` (memories + knowledge graph rows).
    Mempalace,
    /// ZeroClaw `brain.db` (Sara's Discord memories).
    Zeroclaw,
}

/// Arguments to `ijima import <kind>`.
#[derive(Args, Debug)]
struct ImportArgs {
    /// Source corpus kind to read.
    #[arg(value_enum)]
    kind: ImportKind,
    /// Path to the source SQLite database.
    #[arg(long, value_name = "PATH")]
    db: std::path::PathBuf,
    /// Source name: stamped as the `origin` provenance of every imported
    /// memory and used to derive the default target namespace
    /// (`ns_import_<source>`).
    #[arg(long, value_name = "NAME")]
    source: String,
    /// Target namespace override (default: `ns_import_<source>`).
    #[arg(long, value_name = "NS")]
    namespace: Option<String>,
    /// Daemon base URL (default: `$IJIMA_URL` or
    /// `http://127.0.0.1:7373`).
    #[arg(long, value_name = "URL")]
    url: Option<String>,
    /// Bearer grant with memory:write (default: `$IJIMA_TOKEN`).
    #[arg(long, value_name = "TOKEN")]
    token: Option<String>,
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

/// `ijima namespace <action>` — org-wall membership management (WS3).
#[derive(Subcommand, Debug)]
enum NamespaceAction {
    /// Grant a principal membership in a shared namespace.
    Grant(NsMembershipArgs),
    /// Revoke a principal's membership (idempotent).
    Revoke(NsMembershipArgs),
    /// List a namespace's members, oldest grant first.
    Members(NsMembersArgs),
}

#[derive(Args, Debug)]
struct NsMembershipArgs {
    /// The shared namespace (e.g. `ns_ia_shared`).
    namespace: String,
    /// The principal to grant or revoke.
    principal: String,
    /// Daemon base URL.
    #[arg(long, default_value = "http://127.0.0.1:7373")]
    url: String,
    /// Admin bearer token.
    #[arg(long)]
    auth: String,
}

#[derive(Args, Debug)]
struct NsMembersArgs {
    /// The shared namespace to list.
    namespace: String,
    /// Daemon base URL.
    #[arg(long, default_value = "http://127.0.0.1:7373")]
    url: String,
    /// Admin bearer token.
    #[arg(long)]
    auth: String,
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
    /// Path to the issuance policy TOML (Schubert 0.5 #20.3). Default
    /// resolution: `$IJIMA_POLICY` > `$IJIMA_DIR/policy.toml` > the
    /// embedded policy. The policy must entitle the principal to every
    /// requested capability — issuance fails closed otherwise.
    #[arg(long, value_name = "PATH")]
    policy: Option<PathBuf>,
    /// Grant lifetime in seconds; the grant dies when `now >= now +
    /// seconds` (inclusive, Schubert ADR-0001). Omit for a never-expiring
    /// grant (pre-0.5 behavior). Service principals should always carry
    /// an expiry.
    #[arg(long, value_name = "SECONDS")]
    expires_in: Option<u64>,
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
        Command::Namespace { action } => match action {
            NamespaceAction::Grant(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(run_ns_grant(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "namespace grant failed");
                        ExitCode::FAILURE
                    }
                }
            }
            NamespaceAction::Revoke(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(run_ns_revoke(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "namespace revoke failed");
                        ExitCode::FAILURE
                    }
                }
            }
            NamespaceAction::Members(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                match rt.block_on(run_ns_members(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        tracing::error!(error = %e, "namespace members failed");
                        ExitCode::FAILURE
                    }
                }
            }
        },
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
        Command::Import(args) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match rt {
                Ok(rt) => match rt.block_on(run_import(args)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("ijima: import failed: {e}");
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

    // Policy-constrained issuance (Schubert 0.5 #20.3): the resolved
    // policy must entitle the principal to every requested capability —
    // fails closed, no geometry smuggling under an allowed id.
    let policy_toml = IjimaAuth::resolve_issuance_policy(args.policy.as_deref())?;
    let policy_cfg = IjimaAuth::issuance_policy_from_source(&policy_toml)?;
    let grant_policy = schubert::crypto::GrantPolicy::from_policy(&policy_cfg)
        .map_err(|e| ijima_core::IjimaError::invalid_input(format!("grant policy: {e}")))?;
    let expires_at = args.expires_in.map(|secs| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() + secs)
            .unwrap_or(secs)
    });
    let token = auth.issue_grant_bearer_under_policy(
        args.principal.as_str(),
        &cap_refs,
        &grant_policy,
        expires_at,
    )?;
    let public_key = auth.issuer_public_key_hex();

    if args.json {
        let caps_json = caps.join(",");
        match expires_at {
            Some(at) => println!(
                "{{\"token\":\"{token}\",\"principal\":\"{}\",\"capabilities\":\"{caps_json}\",\"expires_at_unix\":{at},\"public_key\":\"{public_key}\"}}",
                args.principal
            ),
            None => println!(
                "{{\"token\":\"{token}\",\"principal\":\"{}\",\"capabilities\":\"{caps_json}\",\"public_key\":\"{public_key}\"}}",
                args.principal
            ),
        }
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
/// `ijima namespace grant <ns> <principal>` — WS3 org-wall grant (admin).
async fn run_ns_grant(args: NsMembershipArgs) -> ijima_core::Result<()> {
    let client = reqwest::Client::new();
    let endpoint = format!("{}/namespaces/grant", args.url.trim_end_matches('/'));
    let resp = client
        .post(&endpoint)
        .bearer_auth(args.auth.trim())
        .json(&serde_json::json!({
            "namespace": args.namespace,
            "principal": args.principal,
        }))
        .send()
        .await
        .map_err(|e| ijima_core::IjimaError::Transport {
            detail: format!("namespace grant: {e}"),
        })?;
    match resp.status() {
        reqwest::StatusCode::OK => {
            eprintln!(
                "ijima: `{}` is now a member of `{}`.",
                args.principal, args.namespace
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

/// `ijima namespace revoke <ns> <principal>` — WS3 org-wall revoke (admin).
async fn run_ns_revoke(args: NsMembershipArgs) -> ijima_core::Result<()> {
    let client = reqwest::Client::new();
    let endpoint = format!("{}/namespaces/revoke", args.url.trim_end_matches('/'));
    let resp = client
        .post(&endpoint)
        .bearer_auth(args.auth.trim())
        .json(&serde_json::json!({
            "namespace": args.namespace,
            "principal": args.principal,
        }))
        .send()
        .await
        .map_err(|e| ijima_core::IjimaError::Transport {
            detail: format!("namespace revoke: {e}"),
        })?;
    match resp.status() {
        reqwest::StatusCode::NO_CONTENT => {
            eprintln!(
                "ijima: `{}` membership in `{}` revoked.",
                args.principal, args.namespace
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

/// `ijima namespace members <ns>` — WS3 membership listing (admin).
async fn run_ns_members(args: NsMembersArgs) -> ijima_core::Result<()> {
    let client = reqwest::Client::new();
    let endpoint = format!(
        "{}/namespaces/members?namespace={}",
        args.url.trim_end_matches('/'),
        args.namespace
    );
    let resp = client
        .get(&endpoint)
        .bearer_auth(args.auth.trim())
        .send()
        .await
        .map_err(|e| ijima_core::IjimaError::Transport {
            detail: format!("namespace members: {e}"),
        })?;
    match resp.status() {
        reqwest::StatusCode::OK => {
            let members: serde_json::Value =
                resp.json()
                    .await
                    .map_err(|e| ijima_core::IjimaError::Transport {
                        detail: format!("members decode: {e}"),
                    })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&members).unwrap_or_default()
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
/// `ijima import <kind> --db <path> --source <name>` — WS2 multi-source
/// import against a running daemon over HTTP. Reads the source SQLite
/// rows, retags provenance (origin = source, AutoCapture trust), and
/// streams them through the daemon's dedup-checked store path into
/// `ns_import_<source>` (or `--namespace`).
async fn run_import(args: ImportArgs) -> ijima_core::Result<()> {
    use ijima_server::migration::{
        default_import_ns, map_pipalace_memory, map_zeroclaw_memory, read_pipalace_memories,
        read_zeroclaw_memories, retag_imported,
    };

    let url = args
        .url
        .or_else(|| std::env::var("IJIMA_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:7373".to_string());
    let token = args
        .token
        .or_else(|| std::env::var("IJIMA_TOKEN").ok())
        .ok_or_else(|| {
            ijima_core::IjimaError::invalid_input(
                "import needs --token or $IJIMA_TOKEN (a memory:write grant)",
            )
        })?;

    let memories: Vec<_> = match args.kind {
        ImportKind::Mempalace => {
            let rows = read_pipalace_memories(&args.db.to_string_lossy())?;
            eprintln!("ijima: read {} rows from {}", rows.len(), args.db.display());
            rows.iter().map(map_pipalace_memory).collect::<Vec<_>>()
        }
        ImportKind::Zeroclaw => {
            let rows = read_zeroclaw_memories(&args.db.to_string_lossy())?;
            eprintln!("ijima: read {} rows from {}", rows.len(), args.db.display());
            rows.iter().map(map_zeroclaw_memory).collect::<Vec<_>>()
        }
    }
    .into_iter()
    .map(|m| retag_imported(m, &args.source))
    .collect();

    let ns = args
        .namespace
        .unwrap_or_else(|| default_import_ns(&args.source).as_str().to_string());

    let client = ijima_client::Client::new(
        ijima_client::ClientConfig::new(url, ijima_core::harness::Harness::Pi).with_token(token),
    );
    eprintln!(
        "ijima: importing {} memories from `{}` into namespace `{ns}`…",
        memories.len(),
        args.source
    );
    let counts = client.import_memories(&ns, memories).await?;
    eprintln!(
        "ijima: import `{}` complete — {} added, {} deduped, {} skipped (of {} attempted)",
        args.source, counts.added, counts.deduped, counts.skipped, counts.attempted
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&counts).unwrap_or_default()
    );
    Ok(())
}

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
