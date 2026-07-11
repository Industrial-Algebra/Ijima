# Installing and Configuring

## Environment variables

| Variable | Default | Purpose |
|---|---|---|
| `IJIMA_DIR` | `~/.ijima` | Issuer key + SurrealKv database location. |
| `IJIMA_LOG` | `ijima=info` | `tracing` filter (`debug`, `ijima=debug`, etc.). |
| `IJIMA_TLS_CERT` / `IJIMA_TLS_KEY` | — | Set both to serve HTTPS (`tls` feature). |
| `IJIMA_RATE_DISABLE` | — | Disable rate limiting (`rate-limit` feature). |
| `IJIMA_RATE_BASE` / `IJIMA_RATE_MULTIPLIER` | — | Tune rate-limit capacity. |
| `IJIMA_LLM_BASE_URL` | `https://api.deepseek.com/v1` | Mining LLM endpoint (`mining` feature). |
| `IJIMA_LLM_MODEL` | — | Mining LLM model; unset → rules-only mining. |
| `IJIMA_LLM_API_KEY` | — | Mining LLM key; unset → rules-only mining. |

## Deployment model

Ijima runs as a daemon on a host (or a private Tailscale node), with
harnesses connecting over HTTP. Trust-by-default on the private network;
opt-in TLS for elsewhere. A single SurrealKv database file under `IJIMA_DIR`
for easy backup/migration.

## Backups

```bash
./target/debug/ijima export --out ijima-backup.db
```

Uses SurrealDB's `db.export()` against the embedded store.
