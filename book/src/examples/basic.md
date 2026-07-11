# Basic Usage

A round-trip: store, search, and mine. Assumes a running daemon
(`ijima serve`) and tokens issued via `ijima token issue`.

```bash
# 1. Ingest a raw session turn (the ore).
curl -X POST http://127.0.0.1:7373/sessions/sess_1/turns \
  -H "authorization: Bearer <session:ingest-token>" \
  -H "content-type: application/json" \
  -d '{"session_id":"sess_1","turn_index":0,"role":"User",
       "content":"We decided to use SurrealDB for storage. See https://surrealdb.com",
       "timestamp":"0"}'

# 2. Mine the session into curated memories (rules + llm).
curl -X POST http://127.0.0.1:7373/sessions/sess_1/mine \
  -H "authorization: Bearer <mining:trigger-token>"
# => {"archived":2,"queued":0}   (a Decision + a Reference, both Auto)

# 3. Semantic search across the palace.
curl -X POST http://127.0.0.1:7373/memories/search \
  -H "authorization: Bearer <memory:read-token>" \
  -H "content-type: application/json" \
  -d '{"text":"storage choice","limit":5}'

# 4. (Optional) review low-confidence extractions before they archive.
curl http://127.0.0.1:7373/mining/queue \
  -H "authorization: Bearer <mining:review-token>"
```

For harness integration, prefer the typed `ijima-client` crate over raw
curl — see the client crate's docs for the 1:1 method→route mapping.
