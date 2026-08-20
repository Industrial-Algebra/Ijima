# Architecture

## The workspace

```
ijima-core      domain types + Store/KnowledgeGraph traits + capability vocab
ijima-server    axum daemon, SurrealDB backend, Schubert auth, CLI
ijima-client    typed async HTTP client (thin clients)
ijima-miner     extraction engine (rules + LLM tiers)
ijima-pi        pi extension types (compiled to WASM for integrations/pi)
```

Dependencies flow one direction: server → core/miner/client; miner →
core (+ proserpina-agent for the LLM tier); client → core. `ijima-core`
has no HTTP, no database, no async runtime beyond trait definitions —
the domain is pure.

## Store backends

- **SurrealDB** (`backend-surreal`) — the primary backend. Embedded
  engines: `kv-mem` (tests, ephemeral) and `surrealkv` (persistent
  single-file). Record keys are namespace-composite
  (`<ns>:<id>`); tables are defined idempotently at open; every filtered
  column has a `DEFINE INDEX` (SurrealDB does not auto-index).
- **SQLite** (`backend-sqlite`) — migration-only readers for the legacy
  pi-mempalace / ZeroClaw corpora. Never a runtime backend.

## The daemon

`serve` builds the router once at boot: middleware order is
authentication (bearer → GrantToken verify → revocation check) →
rate limiting (intersection-number token buckets) → capability check
per-route → `resolve_ns` → handler. All handlers are thin: they map
HTTP onto `Store`/`KnowledgeGraph` trait calls. The mining feature adds
the extraction pipeline as an in-process stage over the session
repository.

## Deployment modes

- **Daemon + thin clients** (the 0.2.0 "Central Brain" topology): one
  instance on an always-on host; workstations point `IJIMA_URL` at it.
- **Embedded in-process**: build `backend-surreal` without `http`/`server-auth`
  for tests and embedding — an unauthenticated direct store.
- **Satellites** (0.3 design): full local instances with checkpoint sync
  to the center; the federation control API scaffold is the seed.

## Provenance by construction

Provenance is not a bolt-on: `Memory` *is* content + provenance in the
domain type. Every write path (HTTP, import, mining, doctrine ingest)
must produce a full provenance block; every read path carries it back.
This is what makes trust tiers and (future) federation conflict
resolution enforceable at the type level.
