# Getting Started

## Prerequisites

- A recent Rust toolchain (see [`rust-toolchain.toml`](https://github.com/Industrial-Algebra/Ijima/blob/develop/rust-toolchain.toml)).
- (Optional) a model endpoint for the mining LLM tier — DeepSeek by default.

## Build the daemon + CLI

```bash
git clone https://github.com/Industrial-Algebra/Ijima.git
cd Ijima
cargo build --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining" --bin ijima
```

## First run

```bash
# Where the issuer key + database live.
export IJIMA_DIR=~/.ijima

# Issue an admin token (creates the issuer key on first run).
./target/debug/ijima token issue --principal elliott --capability admin

# Start the daemon (HTTP on port 7373).
./target/debug/ijima serve --port 7373
```

## Store and search a memory

```bash
# Store (needs a memory:write token).
curl -X POST http://127.0.0.1:7373/memories \
  -H "authorization: Bearer <token>" \
  -H "content-type: application/json" \
  -d '{"id":"m1","content":"Decided to use SurrealDB","project":"ijima","topic":"storage","source":"Explicit","harness":"Pi"}'

# Semantic search (the daemon embeds centrally with candle).
curl -X POST http://127.0.0.1:7373/memories/search \
  -H "authorization: Bearer <read-token>" \
  -H "content-type: application/json" \
  -d '{"text":"database choice","limit":5}'
```

## Next

- [Feature Flags](./guide/feature-flags.md) — the full build matrix.
- [The Mining Pipeline](./guide/mining.md) — turn sessions into memories.
- [Security](./design/security.md) — the capability model.
