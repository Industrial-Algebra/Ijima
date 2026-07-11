# Changelog

All notable changes to Ijima are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first release of Ijima — the Anima ecosystem's centralized
"company brain": a multi-tenant memory service (memory palace + knowledge
graph + session-context repository) with Schubert capability auth, local
candle embeddings, semantic search, and a session-mining pipeline.

These changes are merged to the `develop` branch but **not yet released**
— Ijima ships when the crates are published to crates.io and the release
is tagged.

### Memory palace
- CRUD + list, scoped to namespaces (`store_memory`, `recall_memory`,
  `search_memories`, `delete_memory`).
- **Semantic search** via local candle embeddings (`all-MiniLM-L6-v2`,
  384-dim) + cosine similarity — no external embedding API.
- **Multi-tenancy**: personal / shared / doctrine namespaces with
  per-principal isolation; promotion (personal → shared, redacted).
- **Dedup**: content-hash (SHA-256) exact + semantic similarity check
  before write (`check_duplicate`).
- **Palace organization**: cross-project graph + tunnels, rooms, taxonomy
  browsing.
- **Wake-up composition**: L0 (doctrine) + L1a (project) + L1b (recency)
  context front-load.
- **Per-agent diaries** (`diary_write`, `diary_read`).
- **Provenance-tier model**: every `Memory` carries `origin` (InstanceId) +
  `authority` (AuthorityScope) provenance; `MemorySource::trust_grade()`
  maps tiers to Schubert codimensions. (Foundation for federation +
  context-poisoning protection.)

### Knowledge graph
- Temporal triples (entities + facts with `valid_from`/`valid_to`):
  `add_triple`, `query_entity`, `invalidate_triple`, `kg_timeline`,
  `find_triples`, `knowledge_stats`.

### Session-context repository
- Sessions (`create_session`, `list_sessions`, `end_session`) + raw turn
  ingestion (`ingest_turn`, `session_turns`) — the ore the miner refines.

### Mining pipeline
- **Rules tier** (no model): Decision + Reference extraction, always on.
- **LLM tier** (Proserpina): Fact + Pattern roles, single-shot, JSON-lines
  contract, confidence-routed (≥0.85 auto-archives).
- **Review queue**: per-namespace staging (`enqueue`, `list_pending`,
  `accept`, `reject`) for PendingReview extractions.
- **Trigger**: `POST /sessions/:id/mine` runs rules + llm (when configured)
  and ingests. Daemon constructs the LLM agent from `IJIMA_LLM_*` env;
  `spawn_blocking` bridges the sync miner to the async HTTP daemon.

### Context Mapper
- Global repo directory (`POST /repos`, `GET /repos/resolve?path=`) —
  longest-prefix CWD→project resolution. The canonical Anima ecosystem
  roster; solves the "repos move / sessions start elsewhere" problem.

### Auth & security
- **Schubert capability auth**: proof-carrying tokens on **Gr(4,8)**
  (dimension 16). 11 capabilities as Schubert partitions; `intersection_number`
  doubles as rate-limit capacity.
- **Rate limiting**: capacity scales with capability codimension
  (`memory:read`→1×, `memory:write`→2×, `admin`→16×); 429 on exhaustion.
- **Trust-tier transitions as capabilities**: `trust:promote` (codim 4)
  gates promotion — raising trust is costlier than writing at a tier.
  `trust:endorse` (5), `trust:override` (6, default-deny, Phase 5).

### Operations
- **Persistence**: SurrealKv on disk (`IJIMA_DIR`); `Mem` retained for tests.
- **Structured logging**: `tracing` + `tracing-subscriber`, `IJIMA_LOG` filter.
- **Backup/export**: `ijima export --out` (SurrealDB `db.export()`).
- **TLS**: optional HTTPS via `IJIMA_TLS_CERT` / `IJIMA_TLS_KEY`.
- **Doctrine ingest**: Git → CI → service pipeline.

### Crates
- `ijima-core` — pure domain types (no backend).
- `ijima-server` — SurrealStore, Schubert auth, candle embedder, axum daemon,
  mining orchestration, `ijima` CLI.
- `ijima-miner` — extraction engine (rules + Proserpina llm).
- `ijima-client` — typed async HTTP client for harnesses.

### CLI
- `ijima token issue` — mint proof-carrying capability tokens.
- `ijima serve` — run the HTTP daemon.
- `ijima ingest` / `ijima doctrine` — load content.
- `ijima export` — backup/export the store.

[Unreleased]: https://github.com/Industrial-Algebra/Ijima/compare/HEAD...develop
