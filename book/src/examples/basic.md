# Basic Usage

A round-trip: store, search, mine, promote. Assumes a running daemon
(`ijima serve`) and a grant issued via `ijima token issue`.

## Store and recall

```bash
TOKEN="..."

curl -s -H "Authorization: Bearer $TOKEN" -H "content-type: application/json" \
     -d '{"id":"mem_1","content":"Amari is the flagship math library",
          "project":"amari","topic":"project-context",
          "source":"Explicit","harness":"Pi",
          "importance":0.7,"created_at":"0"}' \
     localhost:7373/memories

curl -s -H "Authorization: Bearer $TOKEN" localhost:7373/memories/mem_1
```

## Dedup pre-check

```bash
curl -s -H "Authorization: Bearer $TOKEN" -H "content-type: application/json" \
     -d '{"content":"Amari is the flagship math library"}' \
     localhost:7373/memories/check
# {"duplicate":"mem_1"}
```

## From Rust

```rust
let client = Client::new(
    ClientConfig::new("http://127.0.0.1:7373", Harness::Pi).with_token(token),
);

client.store_memory(memory).await?;
let hits = client
    .search_memories(&SearchQuery { text: "flagship".into(), ..Default::default() }, None)
    .await?;
```

## Mine a session

```bash
# ingest a turn
curl -s -H "Authorization: Bearer $INGEST_TOKEN" -H "content-type: application/json" \
     -d '{"turn_index":0,"role":"User","content":"we decided to use surrealdb","timestamp":"..."}' \
     localhost:7373/sessions/sess_1/turns

# trigger mining (mining:trigger grant)
curl -s -X POST -H "Authorization: Bearer $MINER_TOKEN" \
     localhost:7373/sessions/sess_1/mine

# review the queue (mining:review grant)
curl -s -H "Authorization: Bearer $REVIEW_TOKEN" localhost:7373/mining/queue
```

Accepting a queued extraction files it as a `Mined` memory with the
source session in its provenance; rejecting archives it.

## Promote

An imported or mined memory earns trust explicitly:

```bash
curl -s -X POST -H "Authorization: Bearer $TRUST_TOKEN" \
     localhost:7373/memories/mem_imported_1/promote
```

## Full walkthroughs

- [Multi-Source Import](./import.md) — consolidating workstation corpora.
- Guides: [Import](../guide/import.md), [Mining](../guide/mining.md),
  [Deploying](../guide/deploy.md).
