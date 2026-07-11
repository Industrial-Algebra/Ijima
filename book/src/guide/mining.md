# The Mining Pipeline

Ijima's novel capability: turning raw session transcripts into curated
memory palace entries with full provenance.

## Tiers

- **Rules tier** (always on, no model): deterministic extraction of
  **Decisions** ("we decided to…") and **References** (URLs / `scheme:`
  links). Fast, free, side-effect-free.
- **LLM tier** (optional, `mining` feature): [Proserpina](https://github.com/Industrial-Algebra/Proserpina)-backed
  **Fact** and **Pattern** roles. Single-shot per role (no panel
  cross-examination in v0). Emits one JSON object per line:
  `{"content","project","topic","confidence"}`.

Configure the LLM tier with `IJIMA_LLM_BASE_URL` / `IJIMA_LLM_MODEL` /
`IJIMA_LLM_API_KEY`. When model/key are unset, mining runs **rules-only**.

## Routing

- **Auto** extractions archive straight to the palace (content-hash dedup
  applies).
- **PendingReview** extractions stage in a per-namespace review queue.
  Confidence ≥ 0.85 overrides a role's default to `Auto` — a high-confidence
  fact auto-archives.

## Trigger

```bash
# Mine a session, then review what landed in the queue.
curl -X POST http://127.0.0.1:7373/sessions/sess_1/mine \
  -H "authorization: Bearer <mining:trigger-token>"

curl http://127.0.0.1:7373/mining/queue \
  -H "authorization: Bearer <mining:review-token>"

curl -X POST http://127.0.0.1:7373/mining/queue/<id>/accept \
  -H "authorization: Bearer <mining:review-token>"
```

## Architecture notes

Extraction is **pure and synchronous**; a separate async ingest step writes
to the store. Because the Proserpina `HttpAgent::respond` blocks on its own
tokio runtime, the daemon runs the sync `mine_all` pass inside
`spawn_blocking` — owning the concrete (Send) agent and coercing to
`&mut dyn Agent` inside the closure. See
[`docs/adr/miner-architecture.md`](https://github.com/Industrial-Algebra/Ijima/blob/develop/docs/adr/miner-architecture.md).
