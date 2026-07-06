# Ijima

**Centralized agentic memory backend for the Industrial Algebra ecosystem.**

Ijima is the single source of truth for agentic memory across the IA
stack — pi, Tsume, Sakamoto, Wallace, Dominic, opencode, and future
harnesses. It replaces fragmented per-harness memory stores with one
service every agent reads from and writes to.

## Status

Pre-release (v0.1.0). The HTTP daemon, SurrealDB store, candle
embeddings, Schubert capability auth, namespace isolation, semantic
search, memory promotion, and doctrine ingest are all wired and tested.
See [`docs/HANDOFF.md`](docs/HANDOFF.md) for the original design and
[`docs/DESIGN.md`](docs/DESIGN.md) for the decision log (D1–D9).

## Two-store model

1. **Memory Palace** — long-term semantic memory + knowledge graph.
   Verbatim storage, candle embeddings, cosine search, temporal triples
   (planned). Import-compatible with the pi-mempalace schema.
2. **Session Context Repository** — raw session transcripts from every
   harness. Append-only, high-fidelity. Mined into the palace by
   `ijima-miner` (planned).

The novel capability: Ijima **mines** raw sessions to extract curated
memory palace entries (decisions, facts, references, triples) with full
provenance — "this memory came from that conversation."

## Multi-tenancy

Every request is scoped to a [namespace](ijima-core/src/namespace.rs):
`Private` (per-operator), `Shared` (team), or `Global` (the legacy
pi-mempalace commons). Cross-principal personal isolation is enforced at
the API layer. Promotion from personal → shared runs a
[redaction filter](ijima-server/src/redaction.rs) at the boundary — the
one place content filtering happens; personal storage is always verbatim.

## Quick start

```bash
# Build the daemon + CLI
cargo build --features http,server-auth,backend-surreal,cli --bin ijima

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

# Promote a personal memory to a shared namespace (redacted)
curl -X POST http://127.0.0.1:7373/memories/m1/promote \
  -H "authorization: Bearer <write-token>" \
  -H "content-type: application/json" \
  -d '{"target_namespace":"ns_team"}'
```

## Workspace

| Crate | Role |
|---|---|
| [`ijima-core`](ijima-core) | Pure contract: domain types, `Store` trait, `Embedder` trait, capability vocabulary, error type. Transport- and backend-free. |
| [`ijima-server`](ijima-server) | HTTP daemon + store backends. The `ijima` binary (serve + token + doctrine CLI). |
| [`ijima-miner`](ijima-miner) | Session-context extraction engine (rules + LLM tiers). Scaffold — lands after Proserpina. |
| [`ijima-client`](ijima-client) | Thin HTTP client / harness adapter crate. Scaffold. |

## Architecture

```
   Harnesses (pi, Tsume, Sakamoto, Wallace, opencode, ...)
        │           │           │           │
        └───────────┴─────┬─────┴───────────┘
                          ▼
                  ┌───────────────┐
                  │  ijima serve  │  axum HTTP daemon
                  │  (the spec)   │
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

## Key decisions (see `docs/DESIGN.md`)

- **D1** — Embeddings: candle + all-MiniLM-L6-v2 (384-dim), consistent
  with Quantizon.
- **D2** — Multi-user/multi-access is a first-class design concern.
- **D4** — Auth: Schubert proof-carrying capability tokens (open,
  Apache-2.0). Authn + authz in one crate. `ia-auth` rejected (closed-source).
- **D5** — Policy: Gr(4,8), derived from `schubert recommend` against
  Ijima's constraints (not hand-picked). Declarative TOML at
  [`policy/policy.toml`](policy/policy.toml).
- **D6** — Storage: SurrealDB (native multi-tenancy + graph + embedded
  deploy). Postgres kept open as a future alt.
- **D7** — Vector search: brute-force cosine now (correct, no ANN
  approximation); HNSW is the planned optimization.
- **D9** — Incorporated the shared-memory-service discovery design:
  `Doctrine` origin tier, redaction-at-promotion, multi-party hybrid model.

## License

Apache-2.0. See [LICENSE](LICENSE) for details.
Commercial licensing available — contact Industrial Algebra.
