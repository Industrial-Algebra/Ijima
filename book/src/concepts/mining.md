# Sessions & the Miner

The session repository is Ijima's raw-material intake; the mining pipeline
turns it into curated memory. This is Ijima's most distinctive loop: raw
conversation in one end, reviewed, provenance-carrying memory out the
other.

## The flow

```
harness ──POST /sessions/:id/turns──▶ session repository (verbatim)
                                          │
                            mining:trigger│
                                          ▼
                                    extraction engine
                              (rules tier → LLM tier)
                                          │
                       Auto-confidence ───┼─── PendingReview
                       (auto-filed)            │
                                               ▼
                                        review queue
                                               │ mining:review
                                               ▼
                                     accept → palace (Mined tier)
                                     reject → archived
```

1. **Ingest** (`session:ingest`) — harnesses stream turns verbatim. No
   filtering, no interpretation: the repository is the audit record.
2. **Extract** (`mining:trigger`) — the extraction engine runs over a
   session. The rules tier (deterministic, cheap) catches decisions,
   TODOs, and entity mentions; the LLM tier (optional, via a Proserpina
   HTTP agent) proposes richer candidates with confidence scores.
3. **Route by confidence** — high-confidence extractions file
   automatically as `Mined` memories; the rest wait in the review queue.
4. **Review** (`mining:review`) — a human or agent reviews the queue,
   accepting (→ palace, provenance `Mined` + source session) or
   rejecting (→ archived, kept for the record).

## Why sessions stay verbatim

The repository is deliberately unprocessed: mining proposals can be
rejected, improved, and re-run, but the source transcript is immutable
evidence. Provenance on every mined memory points back at the session and
turn range it came from.

## Trust flow

Mined memories enter at the `Mined` tier — above nothing, below
`Explicit`. Promoting them (or any memory) to higher trust is a separate,
deliberate act gated by `trust:promote`. The pipeline never silently
raises trust.

See [The Mining Pipeline](../guide/mining.md) for operation, and the
miner architecture ADR for the tier design.
