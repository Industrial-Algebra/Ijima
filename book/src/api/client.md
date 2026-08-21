# The Client Crate

`ijima-client` is the typed async HTTP client for harnesses and tools.
It is transport-thin: it builds requests, attaches the bearer, decodes
responses — and nothing else. All policy lives in the daemon.

## Setup

```toml
[dependencies]
ijima-client = "0.1"
```

```rust
use ijima_client::{Client, ClientConfig};
use ijima_core::harness::Harness;

let client = Client::new(
    ClientConfig::new("http://ijima.tailnet:7373", Harness::Pi)
        .with_token(token),
);
```

`ClientConfig` is cloneable; `with_token` accepts the raw grant (the
`Bearer ` prefix is added internally).

## Surface (selected)

```rust
// memories
client.store_memory(memory).await?;                  // personal ns
client.store_memory_in("ns_import_laptop", m).await?; // explicit ns
client.recall_memory("mem_x", Some("ns_ia_shared")).await?;
client.delete_memory("mem_x", None).await?;
client.search_memories(&query, Some("ns_ia_shared")).await?;
client.check_duplicate("content", Some(ns)).await?;   // dedup pre-check
client.import_memories(ns, memories).await?;          // dedup-checked bulk

// knowledge graph
client.add_triple(triple).await?;
client.query_entity("amari", Some(ns)).await?;

// sessions & mining
client.ingest_turn(session_id, turn).await?;
client.trigger_mining(session_id).await?;

// palace surfaces
client.list_rooms(Some(ns)).await?;
client.taxonomy(Some(ns)).await?;
client.palace_graph(Some(ns)).await?;
```

`import_memories` returns `ImportCounts { attempted, added, deduped,
skipped }` — the WS2 import loop in a single call.

## Errors

Everything surfaces as `IjimaError`: `Transport` (HTTP failure, carrying
status + body detail), `Store`, or domain errors. A `404` from
`recall_memory` maps to `Ok(None)` — absence is not an error.

## Feature notes

- `remote` (default) brings reqwest; disable for an in-process stub in
  tests.
- The client identifies its harness in provenance fields — set the real
  one; the daemon records it on every write.
- For pi specifically, the compiled extension (`integrations/pi`) wraps
  this surface for WASM — pi processes never hand-roll JSON.
