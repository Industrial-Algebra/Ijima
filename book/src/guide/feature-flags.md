# Feature Flags

Ijima is additive-feature-gated: compose the build with the surface you
need. The daemon crate's matrix:

| Feature | Enables |
|---|---|
| `std` | `std` prelude integration |
| `http` | The axum HTTP daemon |
| `server-auth` | Schubert auth (GrantTokens, policy, verifier) |
| `backend-surreal` | SurrealDB store (embedded kv-mem / surrealkv) — the primary backend |
| `backend-sqlite` | **Migration-only** SQLite readers for pi-mempalace / ZeroClaw imports |
| `rate-limit` | Schubert intersection-number token buckets |
| `cli` | The `ijima` binary (clap + reqwest + ijima-client) |
| `embeddings-katakuri` | Local embeddings via Katakuri (weights, tokenizer, digest-verified hub) + candle forward pass — the shipped path since 0.4.3 |
| `embeddings-candle` | *Deprecated* (removal candidate 0.5.0): the pre-Katakuri path via `hf-hub`/`tokenizers`. Bit-for-bit parity is pinned by the CI parity job; production moved off it in 0.4.3. |
| `mining` | Extraction pipeline (ijima-miner + proserpina-agent HTTP LLM tier) |
| `tls` | axum-server/rustls HTTPS (`IJIMA_TLS_CERT`/`IJIMA_TLS_KEY`) |
| `federation` | `/federation/*` control-API scaffold + instance identity |

Defaults: `std,http,server-auth,backend-surreal` — a production daemon
without the CLI. (The published CLI binary ships `cli` plus the optional
surfaces.)

Companion crates gate independently: `ijima-client`'s `remote` (reqwest
transport) and `std`; `ijima-core`'s `serde` and `federation`.

## Choosing

- **Server**: default + `cli` + `embeddings-katakuri` (+ `mining`, `tls`,
  `backend-sqlite` during migration).
- **Embedded library use** (tests, in-process): `backend-surreal`
  without `http`/`server-auth` — an unauthenticated in-process store.
- **Thin clients**: `ijima-client` alone.

## Notes

- `backend-sqlite` exists to read legacy databases once; it is not a
  runtime store backend.
- `server-auth` off means unauthenticated single-process mode — for tests
  and embedded use only, never a networked daemon.
- The pi extension builds from `ijima-pi` to WASM — see
  `integrations/pi`.
