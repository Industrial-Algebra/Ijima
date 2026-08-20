# Installing and Configuring

## Configuration precedence

Ijima resolves settings in a strict layer order — later wins:

```
defaults  <  ijima.toml file  <  environment variables  <  CLI flags
```

## Config file discovery

The daemon looks for `ijima.toml` in order:

1. `$IJIMA_CONFIG` — explicit pointer (a missing or malformed file at an
   explicit pointer is a **hard error**, not a silent fallback)
2. `$IJIMA_DIR/ijima.toml`
3. `/etc/ijima/ijima.toml`

## Config keys

```toml
# /etc/ijima/ijima.toml
host = "127.0.0.1"
port = 7373
data_dir = "/var/lib/ijima"
issuer_key = "/var/lib/ijima/issuer.key"
rate_base = 10
rate_multiplier = 1.0
embedding_model = "sentence-transformers/all-MiniLM-L6-v2"
```

Unknown keys are ignored (forward compatibility).

## Environment variables

| Variable | Purpose |
|---|---|
| `IJIMA_DIR` | Data directory (default `~/.ijima`) |
| `IJIMA_CONFIG` | Explicit config file path |
| `IJIMA_POLICY` | Issuance-policy file (policy-constrained `ijima token issue`; see [Token Management](./tokens.md)) |
| `IJIMA_HOST` / `IJIMA_PORT` | Bind address overrides |
| `IJIMA_KEY` | Issuer key path override |
| `IJIMA_RATE_BASE` / `IJIMA_RATE_MULTIPLIER` / `IJIMA_RATE_DISABLE` | Rate-limit tuning |
| `IJIMA_TLS_CERT` / `IJIMA_TLS_KEY` | PEM paths for the `tls` feature |
| `IJIMA_EMBED_MODEL` / `IJIMA_EMBED_REVISION` | Embedding model pinning |
| `IJIMA_LLM_MODEL` / `IJIMA_LLM_BASE_URL` / `IJIMA_LLM_API_KEY` | Mining LLM tier endpoint |
| `IJIMA_INSTANCE_ID` / `IJIMA_INSTANCE_ROLE` / `IJIMA_INSTANCE_SCOPES` | Federation instance identity |
| `IJIMA_LOG` | Log filter |

Client-side (thin clients and the CLI's remote commands): `IJIMA_URL`
(default `http://127.0.0.1:7373`) and `IJIMA_TOKEN` (the bearer grant).

## Feature selection at install time

```bash
cargo install ijima-server \
    --features "cli,backend-sqlite,embeddings-candle,mining,tls"
```

See [Feature Flags](./feature-flags.md) for the full matrix. The default
set (`std,http,server-auth,backend-surreal`) runs a production daemon;
add `backend-sqlite` only for one-time imports (it exists to read legacy
databases).
