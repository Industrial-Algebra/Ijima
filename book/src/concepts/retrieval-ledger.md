# The retrieval ledger

> Since 0.4.2. The ledger records what the read paths actually serve.

Every wake-up, search, and browse response appends one event per
served memory to an append-only ledger. The events carry ids and
metadata only — never memory content and never query text.

## Why

Before 0.4.2 the read paths had no telemetry at all. Which memories
reach agent context, how often, from which wall, and — just as
important — which *never* do: these were unanswerable after the fact.
Policy questions (is the recency stratum admitting too much? does
visible search drown small walls?) need data before they need
opinions. Instrumentation before policy.

## The event

| Field | Meaning |
|---|---|
| `retrieval_id` | Groups all events from one request |
| `kind` | `WakeupRecent`, `WakeupLexicographic`, `WakeupDoctrine`, `Search`, `Browse` |
| `namespace` | The wall the memory was served from |
| `memory_id` | Join key back to the palace |
| `principal` | Who triggered the retrieval |
| `rank` | 0-based position in the served list |
| `similarity` | Cosine similarity (Search events only) |
| `ts_unix` | Unix seconds |

Wake-up strata are recorded separately: the recency stratum (admitted
first, the starvation fix) and the lexicographic fill (importance
order) tag their events distinctly, so stratum distribution is a
one-query question.

## Reading it

Admin-gated:

```sh
# newest 100 events on one wall
curl -H "authorization: Bearer $ADMIN" \
  "http://127.0.0.1:7373/retrieval/ledger?namespace=ns_shared&limit=100"

# aggregate counts
curl -H "authorization: Bearer $ADMIN" \
  "http://127.0.0.1:7373/retrieval/ledger/stats?namespace=ns_shared"

# prune older than a timestamp (returns count deleted)
curl -X DELETE -H "authorization: Bearer $ADMIN" \
  "http://127.0.0.1:7373/retrieval/ledger?before_unix=1770000000"
```

## Design notes

- **Fire-and-logged.** An append failure logs a warning and the read
  still succeeds — observability must not become a new failure mode.
- **Ids, not content.** The ledger joins back to the palace by
  `memory_id`; it never duplicates content or query text.
- **Append-only until pruned.** No process rewrites events; pruning
  is an explicit admin action with a returned count.
