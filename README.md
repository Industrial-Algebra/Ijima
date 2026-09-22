# Ijima

[![Docs](https://img.shields.io/badge/docs-ijima.industrialalgebra.com-blue)](https://ijima.industrialalgebra.com)
[![Crates.io](https://img.shields.io/crates/v/ijima-server)](https://crates.io/crates/ijima-server)

**The centralized memory service for the Anima ecosystem.**

Ijima is the single source of truth for agentic memory — every harness
(pi, Tsume, Sakamoto, Wallace, Dominic, opencode, …) reads from and
writes to one service instead of keeping private memory islands. It holds
a **memory palace** (curated long-term memory + knowledge graph), a
**session-context repository** (raw transcripts), and a **miner** that
turns raw sessions into curated memories with full provenance.

## What Ijima is — and isn't

**Is:** a standalone, multi-tenant memory daemon. Long-term semantic
memory, temporal knowledge-graph triples, raw session storage, automated
session→memory mining, Schubert capability auth, local candle embeddings,
curated-corpus ingestion into org-scoped walls (markdown trees, stable
ids, idempotent re-runs), tier-gated lifecycle (AutoCapture TTL), and
live-daemon KG cleanup + JSONL export. Generalized — any harness can
adapt to it.

**Isn't:** a pi extension, a per-harness adapter, or an in-process store.
Harnesses speak Ijima's HTTP API (or embed `ijima-client`); they don't
re-implement memory logic.

## Status

**v0.3.1 shipped** — all four crates on [crates.io](https://crates.io/crates/ijima-server),
the pi extension on npm ([`@industrialalgebra/ijima-pi`](https://www.npmjs.com/package/@industrialalgebra/ijima-pi)).
In production as a fleet's central memory service since August 2026
(six-week soak reviewed 2026-09-21: error-free since 08-21, hourly mirror,
weekly restore drills). See [`CHANGELOG.md`](CHANGELOG.md) and
[`docs/ROADMAP.md`](docs/ROADMAP.md). The decision log lives in
[`docs/DESIGN.md`](docs/DESIGN.md) (D1–D11) and [`docs/adr/`](docs/adr/);
0.4 design inputs in [`docs/discovery/`](docs/discovery/).

## Two-store model + a miner

1. **Memory Palace** — curated semantic memory + temporal knowledge graph.
   Candle embeddings, cosine search, content-hash + semantic dedup.
   Import-compatible with the pi-mempalace schema.
2. **Session Context Repository** — raw session transcripts from every
   harness. Append-only, high-fidelity.
3. **The Miner** — refines raw sessions into palace entries (decisions,
   facts, references, patterns) with full provenance — *"this memory came
   from that conversation."* A rules tier (always on) plus an optional
   Proserpina LLM tier (Fact + Pattern roles); low-confidence extractions
   stage in a per-namespace review queue.

## Multi-tenancy & provenance

Every request is scoped to a [namespace](ijima-core/src/namespace.rs):
`Private` (per-operator), `Shared` (team), or `Global` — the shared walls
scale to multiple organizations on one daemon (each company its own wall,
membership-gated, with crossover walls where companies build on each
other). Cross-principal personal isolation is enforced at the API layer.
Promotion (personal → shared) runs a [redaction
filter](ijima-server/src/redaction.rs) at the boundary and is gated by the
`trust:promote` capability — the one place content filtering happens;
personal storage is always verbatim.

The pi extension scopes **declaratively**: a repo commits
`.pi/ijima.json` (`{"namespace": "…"}`) and every session in it lands in
that wall — an org's repo cloned anywhere joins its wall on first launch
(precedence: env → project file → default).

Every `Memory` carries **provenance**: origin instance, authority scope,
source tier (`Explicit` / `AutoCapture` / `Mined` / `Doctrine`), harness,
and session. Source tiers map to Schubert trust grades — the foundation
for federation cross-talk policies and context-poisoning protection
([ADR](docs/adr/provenance-tier-model.md)).

## Quick start

```bash
# Build the daemon + CLI (full feature set)
cargo build --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining" --bin ijima

# Issue an admin token (creates the issuer key on first run)
export IJIMA_DIR=~/.ijima
./target/debug/ijima token issue --principal elliott --capability admin

# Start the daemon
./target/debug/ijima serve --port 7373
```

Then talk to it over HTTP:

```bash
# Store a memory (needs a memory:write token)
curl -X POST http://127.0.0.1:7373/memories \
  -H "authorization: Bearer <token>" \
  -H "content-type: application/json" \
  -d '{"id":"m1","content":"Decided to use SurrealDB","project":"ijima","topic":"storage","source":"Explicit","harness":"Pi"}'

# Semantic search (daemon embeds centrally with candle)
curl -X POST http://127.0.0.1:7373/memories/search \
  -H "authorization: Bearer <read-token>" \
  -H "content-type: application/json" \
  -d '{"text":"database choice","limit":5}'

# Mine a session into memories, then review the queue
curl -X POST http://127.0.0.1:7373/sessions/sess_1/mine -H "authorization: Bearer <mining-token>"
curl http://127.0.0.1:7373/mining/queue -H "authorization: Bearer <review-token>"
```

## Feature flags

| Feature | Enables |
|---|---|
| `http` (default) | axum HTTP daemon + routes |
| `server-auth` | Schubert proof-carrying capability auth |
| `backend-surreal` | SurrealDB store (SurrealKv persistence + Mem for tests) |
| `embeddings-candle` | local candle embeddings (`all-MiniLM-L6-v2`, 384-dim) |
| `mining` | session-mining pipeline (rules + Proserpina llm + review queue) |
| `rate-limit` | Schubert rate limiting (capacity scales with capability codim) |
| `tls` | optional HTTPS (`IJIMA_TLS_CERT` / `IJIMA_TLS_KEY`) |
| `cli` | the `ijima` binary (serve / token / import / doctrine tree-ingest / kg-delete / JSONL export) |

## Workspace

| Crate | Role |
|---|---|
| [`ijima-core`](ijima-core) | Pure contract: domain types, `Store` + `KnowledgeGraph` traits, `Embedder`, capability vocabulary, provenance newtypes. Transport- and backend-free. |
| [`ijima-server`](ijima-server) | HTTP daemon + SurrealDB store + Schubert auth + candle embedder + mining orchestration. The `ijima` binary. |
| [`ijima-miner`](ijima-miner) | Session-context extraction engine: rules tier (Decision + Reference) + Proserpina LLM tier (Fact + Pattern). |
| [`ijima-client`](ijima-client) | Typed async HTTP client — the harness adapter crate. |
| [`ijima-pi`](ijima-pi) / [npm](https://www.npmjs.com/package/@industrialalgebra/ijima-pi) | The pi extension (wasm core + thin shim): memory tools, wake-up priming, auto-capture, declarative `.pi/ijima.json` namespaces. |

## Architecture

```
   Harnesses (pi, Tsume, Sakamoto, Wallace, opencode, …)
        │           │           │           │
        └───────────┴─────┬─────┴───────────┘
                          ▼
                  ┌───────────────┐
                  │  ijima serve  │  axum HTTP daemon
                  └───────┬───────┘
                          │
           ┌──────────────┼──────────────┐
           ▼              ▼              ▼
    ┌────────────┐ ┌────────────┐ ┌─────────────┐
    │ Schubert   │ │ SurrealDB  │ │ candle      │
    │ authn+authz│ │ store      │ │ embeddings  │
    │ (Gr(4,8))  │ │ (embedded) │ │ (MiniLM-384)│
    └────────────┘ └────────────┘ └─────────────┘
```

## Documentation

- [`CHANGELOG.md`](CHANGELOG.md) — release history.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — status, shipped phases, future features.
- [`docs/DESIGN.md`](docs/DESIGN.md) — decision log (D1–D11).
- [`docs/adr/`](docs/adr/) — architecture decision records (miner, compaction-recovery, provenance-tier).
- [`policy/policy.toml`](policy/policy.toml) — the Schubert capability vocabulary.

## Security

Ijima's access model is [Schubert](https://github.com/Industrial-Algebra/Schubert)
capability algebra on the Grassmannian **Gr(4,8)**: proof-carrying
multi-capability grant tokens (Schubert 0.5 — expiry, policy-constrained
issuance, revocation reconciliation), with codimension as both
authorization-intersection weight and rate-limit capacity. Unattended
ingest runs on the narrow `doctrine:write` capability — explicitly
targeted non-private walls only. Namespaces enforce isolation; promotion
is the single redaction boundary; trust-tier transitions
(`trust:promote` / `endorse` / `override`) are themselves
capability-gated. See [`docs/DESIGN.md`](docs/DESIGN.md) (D2, D4, D5) and
the [provenance-tier ADR](docs/adr/provenance-tier-model.md).

## License

Apache-2.0. See [LICENSE](LICENSE). Contributions require the
[IA CLA](https://github.com/Industrial-Algebra/.github/blob/main/CLA.md).
