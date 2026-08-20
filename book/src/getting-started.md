# Getting Started

## Prerequisites

- A recent Rust toolchain (see
  [`rust-toolchain.toml`](https://github.com/Industrial-Algebra/Ijima/blob/develop/rust-toolchain.toml)).
- (Optional) a model endpoint for the mining LLM tier — DeepSeek by default.
- (Optional) a Tailscale network for remote thin clients.

## Install

Ijima is published on crates.io:

```bash
cargo install ijima-server --features "cli,backend-sqlite,embeddings-candle"
```

The default features give you the daemon, HTTP, Schubert auth, and the
SurrealDB backend. Add `backend-sqlite` if you will import from a legacy
pi-mempalace or ZeroClaw database, and `embeddings-candle` for semantic
search (downloads all-MiniLM-L6-v2 on first use).

## Start a daemon

```bash
ijima serve
```

The daemon listens on `127.0.0.1:7373`, creates a data directory at
`~/.ijima` (an embedded SurrealDB store + issuer key), and is ready.
`IJIMA_DIR` relocates the data directory; see
[Installing and Configuring](./guide/installation.md).

## Mint a grant

Every request needs a Schubert GrantToken. Mint one for yourself:

```bash
ijima token issue --principal elliott \
    --capabilities memory:read,memory:write,knowledge:read,knowledge:write
```

The token is a base64 GrantToken blob — proof-carrying, signed by the
issuer key in the data directory. Store it somewhere safe (a password
manager); it is a *credential*.

## First requests

```bash
TOKEN="..."  # from the step above

# store a memory
curl -s -H "Authorization: Bearer $TOKEN" -H "content-type: application/json" \
     -d '{"id":"mem_first","content":"Ijima is running","project":"ijima",
          "topic":"notes","source":"Explicit","harness":"Pi",
          "importance":0.5,"created_at":"0"}' \
     localhost:7373/memories

# recall it
curl -s -H "Authorization: Bearer $TOKEN" \
     localhost:7373/memories/mem_first
```

The response provenance block reports what the daemon saw: source tier,
harness, origin instance, authority scope.

## From a harness

Rust harnesses use `ijima-client`:

```rust
let client = ijima_client::Client::new(
    ijima_client::ClientConfig::new("http://127.0.0.1:7373", Harness::Pi)
        .with_token(token),
);
let id = client.store_memory(memory).await?;
```

pi users: the [pi integration](./guide/pi.md) is a single env pair
(`IJIMA_URL`, `IJIMA_TOKEN`).

## Next steps

- [Concepts](./concepts/two-store-model.md) — the two-store model, provenance, capabilities.
- [Importing Legacy Corpora](./guide/import.md) — bring your pi-mempalace history in.
- [The Mining Pipeline](./guide/mining.md) — turn session transcripts into memories.
