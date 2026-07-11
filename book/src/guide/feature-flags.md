# Feature Flags

Ijima is additive-feature-gated. Compose the build with the surface you need.

| Feature | Enables |
|---|---|
| `http` (default) | axum HTTP daemon + routes. |
| `server-auth` | Schubert proof-carrying capability auth. |
| `backend-surreal` | SurrealDB store (SurrealKv persistence + `Mem` for tests). |
| `embeddings-candle` | local candle embeddings (`all-MiniLM-L6-v2`, 384-dim). |
| `mining` | session-mining pipeline (rules + Proserpina llm + review queue). |
| `rate-limit` | Schubert rate limiting (capacity scales with capability codimension). |
| `tls` | optional HTTPS (`IJIMA_TLS_CERT` / `IJIMA_TLS_KEY`). |
| `cli` | the `ijima` binary (`serve` / `token` / `ingest` / `export` / `doctrine`). |

## Common combinations

```bash
# Full daemon (everything).
cargo build --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining,rate-limit,tls" --bin ijima

# Minimal store library (no daemon, no embeddings) for embedding into another crate.
cargo build -p ijima-core

# CI parity (what the workflow checks).
cargo clippy --all-features --all-targets -- -D warnings
```

The `ijima-core` crate is intentionally backend- and transport-free: pure
domain types, the `Store` + `KnowledgeGraph` traits, and the capability
vocabulary. Everything backend-specific lives in `ijima-server`.
