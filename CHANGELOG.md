# Changelog

All notable changes to Ijima are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **NixOS support**: root `flake.nix` — `packages.x86_64-linux.ijima`
  (built from the repo's own source on the pinned nightly toolchain the
  release was verified on; nixpkgs' stable rustc mis-selects diskann's
  AVX-512 VNNI intrinsic), `nixosModules.ijima` (hardened systemd service
  module: `services.ijima.{enable,package,dataDir,bindAddress,port,user,
  memoryMax}`), and a `module-eval` flake check that integrates the module
  into a real NixOS evaluation. Book: new "NixOS" guide chapter.

## [0.2.1] — 2026-08-21

### Added

- **Knowledge-graph import** (`ijima import mempalace`): pi-mempalace
  `entities` + `triples` tables now import alongside memories — entities
  re-addressed from opaque `ent_*` hashes to Ijima's id-is-name
  convention (same-name entities merge), triples carry confidence +
  temporal range (`valid_to` applied as invalidation), orphan references
  counted as `unmapped`. Client surface: `add_triple_in`,
  `invalidate_triple_in`, `import_kg`; `POST /kg/triples` honors
  `?namespace=` (found on the first production deployment: a source
  corpus's 170 entities / 124 triples stayed behind).
- CLI import report now nests per-layer counts:
  `{ "memories": …, "knowledge": …, "unmapped": n }`.

### Fixed

- **Client 429 backoff**: all HTTP calls retry on `Too Many Requests`
  with exponential backoff (250 ms doubling, six attempts, ~16 s) before
  surfacing the error. Previously an import against a rate-limited daemon
  silently counted 429'd rows as `skipped` — the first production import
  lost 13,617 of 14,444 memories this way. Regression-tested E2E against
  a live rate limiter.
- **pi extension build** (`integrations/pi`): `package.json` now points
  at the compiled shim (`main: ./index.js`), ships a one-command build
  (`wasm-pack` + `tsc`), and the compiled shim is committed for
  install-from-checkout.

## [0.2.0] — 2026-08-21

The "Central Brain" release: consolidation, hardening, and the
deployment surface — everything needed to run Ijima as the single
central memory instance for the Anima ecosystem.

### Auth (Schubert)
- **GrantToken migration** (#63): multi-capability, partition-signed
  Schubert grants replace single-capability CapabilityTokens; write
  implies read; admin via geometry (point class); ~60-line duplicated
  wire codec deleted. Breaking bearer-wire change.
- **Token revocation** (#66): store-backed SHA-256 bearer-hash list,
  checked after signature verification; survives restarts; admin routes
  + CLI. Raw bearers never persisted.
- **Schubert 0.5 adoption**: signed grant expiry (`--expires-in`,
  inclusive boundary, distinct `GrantExpired` 401 detail) and
  **policy-constrained issuance** — `ijima token issue` signs only what
  the issuance policy entitles (fails closed; principals-only overlay
  files on the embedded partitions). ADR `schubert-0.5-adoption`.

### Namespace membership (WS3)
- **Org walls**: shared namespaces (anything not `_private`, `global`,
  `ns_doctrine`, or `ns_import_*` staging) now require store-backed
  membership — admins bypass. `resolve_ns` enforces on every
  namespace-resolving route (23 call sites).
- **Promotion target gating**: `POST /memories/:id/promote` no longer
  bypasses the wall (targets run the same rule; import staging rejected
  as a target) — closes a pre-WS3 tunnel.
- Admin surface: `POST /namespaces/grant|revoke`,
  `GET /namespaces/members`; `ijima namespace grant|revoke|members`
  CLI. ADR `namespace-membership.md`.

### Import (WS2, #70)
- `ijima import mempalace|zeroclaw --db --source`: streams legacy SQLite
  corpora into a running daemon over HTTP; per-source `ns_import_<source>`
  namespaces; provenance retagging (origin = source, trust dropped to
  AutoCapture); cross-source dedup via `/memories/check`; idempotent;
  per-source `{attempted, added, deduped, skipped}` report.
- Client: `store_memory_in`, `check_duplicate`, `import_memories`,
  `ImportCounts`.
- Store fixes the import surfaced: namespaced Surreal record keys
  (`<ns>:<id>` — cross-namespace id collision) and origin/authority now
  projected in browse/list selects.

### Deployment (WS1, #65/#66 ancestry)
- Config file layer (`ijima.toml`): defaults < file < env < CLI;
  discovery `$IJIMA_CONFIG` > `$IJIMA_DIR/ijima.toml` >
  `/etc/ijima/ijima.toml`; explicit-but-missing config = hard error.
- `/status` version/uptime; systemd unit + example config +
  central-instance runbook (`docs/deploy/`).

### Dependencies
- proserpina 0.3 → **proserpina-agent 0.1.0** (WS0, #68).
- **surrealdb 2.6.5 → 3.2.4** (#71): SerdeWrapper store bridge; tables
  defined up-front (v3 hard-errors SELECT on missing tables);
  surrealkv directory-lock semantics documented. On-disk layout note:
  pre-0.2 dev databases should be re-imported.
- Dependency sweep (#69): sha2 0.11, base64 0.23, axum-server 0.8,
  serde/serde_json/thiserror/async-trait/clap refreshes; dead `rand`
  removed.

### Docs
- The mdBook: 11 stub chapters → 25 real chapters (Schubert standard);
  deploys on tag via Netlify (workflow green, site ijima.industrialalgebra.com).

### Mining tier
- LLM agent surface via `proserpina-agent` (HttpAgent, backend-http).

## [0.1.0] — 2026-08-10

The first release of Ijima — the Anima ecosystem's centralized
"company brain": a multi-tenant memory service (memory palace + knowledge
graph + session-context repository) with Schubert capability auth, local
candle embeddings, semantic search, and a session-mining pipeline.

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
- Global repo directory (`POST /repos`, `GET /repos/resolve?cwd=`) —
  longest-prefix CWD→project resolution. The canonical Anima ecosystem
  roster; solves the "repos move / sessions start elsewhere" problem.

### Auth & security
- **Schubert capability auth**: proof-carrying tokens on **Gr(4,8)**
  (dimension 16). 11 capabilities as Schubert partitions; `intersection_number`
  doubles as rate-limit capacity.
- **Schubert v0.4.0**: key-store persistence delegated to upstream
  `schubert::crypto::KeyStore` (deletes Ijima's reimplemented seed/permission
  logic); per-capability `CapabilityToken` model (multi-cap `GrantToken`
  deferred to a later release).
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

### pi integration (pi-mempalace replacement)
- **`ijima-pi`** wasm core (WebAssembly): builds Ijima HTTP requests +
  parses responses for the pi coding agent — search, save, delete,
  check_duplicate, knowledge graph (add/query/status/invalidate/timeline).
  Client-side memory-id generation; `scope=visible` merges personal +
  global/shared results.
- **`integrations/pi/`** TypeScript shim: registers the Ijima tools as pi
  memory capabilities, token-per-capability via
  `IJIMA_TOKEN_{MEMORY,KNOWLEDGE}_{READ,WRITE}`. The path to replacing
  pi-mempalace's in-process store with a federated Ijima service.
- `ijima migrate --namespace` — imports the legacy pi-mempalace / ZeroClaw
  SQLite corpora into a private namespace.

[Unreleased]: https://github.com/Industrial-Algebra/Ijima/compare/v0.1.0...develop
[0.1.0]: https://github.com/Industrial-Algebra/Ijima/releases/tag/v0.1.0
