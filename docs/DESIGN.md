# Ijima — Design Decisions

> Living document of design decisions made after the initial handoff.
> Pairs with [`HANDOFF.md`](./HANDOFF.md). Decisions recorded here
> supersede the corresponding open questions in HANDOFF §8.

---

## D1. Embedding & Vector Backend: Candle

**Decision**: Ijima uses **candle** (Hugging Face's pure-Rust ML
framework) as its embedding backend. This resolves HANDOFF §8 Q3/Q4
(fastembed-rs vs candle vs sqlite-vec vs remote API).

**Rationale**:
- Consistency with the IA ecosystem — Quantizon already depends on
  `candle-core` / `candle-nn` / `candle-transformers`.
- Pure Rust, no Python runtime, embeds cleanly in a daemon.
- CPU-default with optional CUDA headroom.

**Conventions**:
- Default model: `all-MiniLM-L6-v2`, **384 dimensions**, so the live
  `~/.pi/agent/memory/memories.db` corpus migrates without re-embedding.
- `candle-core` is wired into `ijima-server` behind the
  `embeddings-candle` feature. `candle-nn` / `candle-transformers` land
  in the TDD pass that loads the model + tokenizer.
- GPU/CUDA is a future opt-in. Adding it requires excluding the `cuda`
  feature from CI's `--all-features` matrix (matches how GPU crates are
  typically handled), so it is intentionally not in the default feature
  set today.
- Vector index: still `sqlite-vec` for v0 (migration parity with
  pi-mempalace). Candle produces the vectors; sqlite-vec stores/searches
  them. This split keeps the embedding backend and the storage index
  decoupled.

**Where it lives**:
- `ijima-core::embeddings` — pure `Embedder` trait + `Embedding` type +
  `DEFAULT_EMBEDDING_DIM` constant. No ML dependency.
- `ijima-server::embeddings_candle` — `CandleEmbedder` impl behind
  `embeddings-candle`.

---

## D2. Multi-User / Multi-Access (supersedes pi-mempalace's model)

**Decision**: Ijima must be designed for **multi-user, concurrent
access** from the start. This is a first-class design concern, not a
bolt-on. pi-mempalace as it currently stands is single-user,
single-access, and that model does not scale to Ijima's role.

### The pi-mempalace assumption being replaced

pi-mempalace assumes a single trusted operator:
- **One shared bearer token** (`PI_MEMPALACE_TOKEN`). Every client that
  knows the token gets full read/write of the entire store. There is no
  notion of *who* is calling.
- **No per-user namespacing** in the schema. `project` / `topic` are
  organizational, not access-control boundaries.
- **No authz** — the token is all-or-nothing.
- **SQLite WAL** gives reasonable concurrent-read performance, but there
  is no explicit write coordination policy, no request isolation, and no
  multi-tenancy story.

This is fine for one person's pi. It breaks the moment Wallace serves
multiple operators, or Dominic dispatches on behalf of several users, or
two harnesses race to mine the same session.

### The expanded design surface

These are the axes Ijima must address. Each lands via its own TDD pass.

1. **Identity model.** Replace the single shared token with first-class
   principals:
   - **Operators** — humans (Elliott, future additional operators).
   - **Harnesses** — the agents (`Harness` enum already in
     `ijima-core`). A request is always `(operator?, harness, action)`.
   - **Sessions** — already tracked for provenance; reuse as a scoping
     unit.
   A typed `PrincipalId` newtype (IA newtype convention) carries
   identity through every call, not a stringly-typed header.

2. **Per-user / per-namespace memory isolation.** Schema gains an
   owner/namespace dimension on `memories`, `triples`, and `sessions`:
   - **Private namespaces** — an operator's personal memory.
   - **Shared namespaces** — project/team memory visible to a group.
   - **Global** — the legacy pi-mempalace "everyone sees everything"
     mode, preserved for backward compatibility and for genuinely
     shared context.
   The migration import maps all existing rows to a default operator +
   global visibility, preserving current behavior as the baseline.

3. **Authorization, not just authentication.** Move beyond bearer-token
   all-or-nothing to capability-style checks (consistent with the IA
   Schubert capability model):
   - Per-action capabilities: `memory:read`, `memory:write`,
     `knowledge:write`, `session:ingest`, `mining:trigger`,
     `mining:review`, `admin`.
   - Capabilities scoped per namespace, not just globally.
   - The mining review queue (HANDOFF §4) is itself a capability-gated
     workflow.

4. **Concurrency & consistency model.** Explicit policy, not implicit:
   - **Writes**: serialized through the store's single writer
     (SQLite's serialized writes + a connection-pool reader/writer
     split). Synchronous ack for decisions/knowledge; async
     fire-and-forget acceptable for session-context ingestion.
   - **Mining**: re-entrant — a mining pass locks a session-range, not
     the whole store. Two miners can run on disjoint sessions
     concurrently.
   - **Dedup** becomes concurrency-critical: the content-hash UNIQUE
     constraint already prevents exact duplicates at the DB layer; the
     semantic-dedup check must hold a short-lived advisory lock to avoid
     two concurrent writers both accepting a near-duplicate.

5. **Multi-tenancy deployment.** Single Ijima daemon serves many
   operators on a private network (Tailscale), the same deployment
   model as the pi-mempalace fork's remote backend. Each request is
   authenticated to an operator; per-operator namespacing provides
   isolation without separate databases.

6. **Audit.** Every write records `(principal, harness, action, target,
   timestamp)`. This is mandatory once multiple actors share the store —
   it's how "who archived this and when" gets answered. Reuses the
   provenance fields but adds the actor dimension pi-mempalace lacks.

### What this changes vs. the v0 schema

The v0 schema (HANDOFF §3) is the migration baseline and stays
import-compatible. Ijima's schema is a **superset**: existing columns
are unchanged, and new columns/tables (`principals`, `capabilities`,
`namespaces`, plus `owner_namespace` and `actor` columns on the
existing tables) are added with defaults that reproduce today's
single-user behavior. The `principal_id`/`namespace_id` columns default
to a sentinel "self/operator-elliott" + "global" so an unmodified
pi-mempalace client continues to work during migration.

### Open sub-questions (for the design pass, not this scaffold)

- Token format: opaque bearer per operator, or per-harness API keys
  issued under an operator? Probably both — operators authenticate with
  a long-lived credential and mint short-lived harness keys.
- Is the mining review queue per-operator or global-with-reviewers?
  Likely: proposed memories carry a target namespace; reviewers of that
  namespace see the queue.
- Should knowledge-graph triples be namespace-scoped or always global?
  Triples describe reality ("Quantizon depends on candle") and may want
  to be global even when the proposing memory is private. Tentative:
  triples are global, but their *provenance* records the source
  namespace.

---

## D6. Storage Backend: Off SQLite, to Postgres and/or SurrealDB

**Decision**: **SurrealDB** is the primary backend, behind a `Store`
trait. Postgres remains a future feature-gated alternative (not v0).
SQLite is retained **only** as the one-time migration read path for the
live pi-mempalace corpus.

### Why SurrealDB for Ijima

model (SQLite + sqlite-vec, single writer, single file) is recorded
throughout HANDOFF §3/§5/§8 but does not fit Ijima's multi-user,
concurrent-write, multi-tenant role (D2). Ijima targets **Postgres**
and/or **SurrealDB**, abstracted behind a `Store` trait with
feature-gated backends so the choice is not locked at the type level.

### Why leave SQLite

- **Single writer** (WAL helps reads; writes serialize) — a real
  bottleneck once session ingest, memory writes, and mining run
  concurrently across harnesses.
- **sqlite-vec** is finicky under concurrent access and ties Ijima to an
  extension's lifecycle.
- **Multi-tenancy** (D2) wants first-class namespacing SQLite doesn't
  offer.

### The abstraction (decided regardless of which backend)

A pure `Store` trait in `ijima-core` (mirrors the `Embedder` pattern):
`store`, `search`, `recall`, knowledge-graph ops, session-context ops.
Backend impls live in `ijima-server` behind additive features:

| Feature | Backend | Role |
|---|---|---|
| `backend-postgres` | Postgres + `pgvector` | production, relational |
| `backend-surreal` | SurrealDB (`surrealdb` crate, embedded or server) | production, graph + native multi-tenancy |
| `backend-sqlite` | SQLite + sqlite-vec | **migration only** — the one-time import path for the live pi-mempalace corpus |

At least one production backend is required; `backend-sqlite` exists
solely to read the legacy corpus during migration, then is dropped.

### Tradeoff: Postgres vs SurrealDB

**Postgres** — the conservative choice:
- `pgvector` is battle-tested and fast.
- `sqlx` is mature; `ia-actix-common` already optionally depends on it
  (some composition available).
- Universally understood ops.
- *Cost*: requires running a separate Postgres process (breaks the
  single-daemon, Tailscale-private, trust-by-default deploy model).
  Multi-tenancy and the knowledge graph are app-level (triples as rows,
  namespaces as a column) — workable but not native.

**SurrealDB** — the interesting fit for Ijima specifically:
- **Multi-tenancy is native** (`NS` / `DB` scope) — maps directly onto
  D2's namespace dimension. Per-operator / per-namespace isolation is a
  first-class concept, not a bolt-on column.
- **Graph is native** — knowledge-graph triples become graph edges,
  entities become nodes. No join tables.
- **Embedded mode** (`surrealdb::engine::any`) preserves SQLite's
  single-daemon, easy-deploy property while adding real concurrency;
  can scale to server mode later without code change.
- Document model fits variable session/memory metadata.
- Vector search supported.
- *Cost*: greener than Postgres in the Rust ecosystem; smaller community;
  the `surrealdb` crate's API is still evolving.

**Both** (feature-gated): viable since the `Store` trait isolates them,
but doubles the schema/migration test surface. Probably pick one primary
and leave the other as a future feature-gated alternative.

### Migration

The live `~/.pi/agent/memory/memories.db` (SQLite) corpus is imported via
`backend-sqlite` as a **one-time read path** into the chosen production
backend. Ijima never depends on SQLite at runtime in production.

### Note: no ecosystem composition here

`amari-surreal` and Schubert's `surreal_trust` are surreal *numbers*
(mathematical), **not SurrealDB**. There is no SurrealDB or Postgres
store abstraction anywhere in the IA ecosystem — this is greenfield for
Ijima. `ia-actix-common`'s optional `sqlx` (Postgres) is the only
adjacent composition, and we are not adopting `ia-actix-common` (D3).

---

## D8. Ecosystem survey: Orlando / Karpal / Amari for the vector problem

After the typed-vector/HNSW friction (D7), we surveyed whether Orlando,
Karpal, or Amari could help. **Verdict: no composition benefit for
Ijima's current problems.**

- **Karpal `VectorSpace`/`Module`/`Field`** — algebraic scale/add
  traits, but **no inner product, norm, or cosine**. Would require a
  new `InnerProductSpace` trait + `Vec<f32>` impls. Build-up, not
  composition; heavier than a dot product.
- **Karpal `Iso`** — isomorphism optic (`forward`/`backward` fns). The
  *principled* abstraction for a bidirectional `Vec<f32>` ↔ typed-vector
  mapping, but it does not solve the actual blocker (the surrealdb SDK
  binds `Vec<f32>` as a plain array regardless). Serde already covers
  record mapping.
- **Karpal `karpal-index`** — an **API documentation indexer** (walks
  source trees), *not* a data/vector index. Red herring.
- **Amari geometric algebra** (`inner_product`, `norm`, `Multivector`,
  `geometric_product`) — the math is right but operates on phantom-typed
  `Multivector<P,Q,R>`, not `Vec<f32>`. Over-engineering for cosine.
- **Orlando `top_k` / transducers** — streaming ranking primitives;
  server-side `ORDER BY ... LIMIT` is the correct layer for DB search.

The genuine future composition for richer retrieval is **Minuet**
(holographic memory on `amari-holographic`, already a Schubert optional
dependency via its `holographic` feature). If Ijima outgrows flat
cosine — compositional / reduced-distribution retrieval — Minuet is the
principled path and composes cleanly. Not v0.

---

## D9. Incorporated shared-memory-service discovery design

The parallel-context design at
`docs/discovery/memory-service-design.md` ("Shared Kai Memory Service")
is Ijima's predecessor/spiritual-sibling brainstorm. It maps directly
onto Ijima and largely aligns. The divergent decisions are already
settled with good reason; the novel concepts have been adopted.

### Aligned (already in Ijima)

- **Scope axis** (`personal` / `team`) = `NamespaceKind::Private` / `Shared`
  / `Global` (D2).
- **"Store everything verbatim"** = pi-mempalace lineage, verbatim store.
- **Per-user isolation + deliberate sharing** = namespace-scoped `Store` trait.
- **Central embedder** = candle behind `Store` trait (D1).
- **Wake-up composition** (L0/L1/L2/L3) = embedding search ready; wakeup
  lands later.

### Divergences (already decided, SurrealDB stands)

- **Postgres+pgvector** → Ijima chose **SurrealDB** (D6). The discovery doc
  predates D6 and was framed for a team with a data engineer. SurrealDB
  achieves the same goals via a different mechanism; this is a settled
  decision with good rationale.
- **MCP front** → Ijima chose **HTTP daemon** first, MCP deferred (D3).
  Both can coexist.

### Adopted from the discovery doc (new in Ijima)

- **`MemorySource::Doctrine`** — curated, Git-versioned, PR-reviewed origin
  (the "3b seed pack"). Authored in Git, mirrored into the service;
  never written directly by agents. The highest-trust, lowest-write-rate
  tier. **Wired** as a new `MemorySource` variant.
- **Redaction at promotion boundary** — personal → team/shared promotion
  runs a scrub/redaction filter (secrets, PII). This is the *one* place
  filtering happens; never at auto-capture. Documented as a requirement
  now; implementation lands with the `memory_promote` endpoint.
- **Multi-party hybrid model for meetings** — auto-capture per-attendee
  into personal scope; a curated summary explicitly promoted to
  team/shared. Aligns with the session-context repository.

---

## Decision Index

| ID | Topic | Resolves | Status |
|----|-------|----------|--------|
| D1 | Embedding/vector backend = candle | HANDOFF §8 Q3, Q4 | Decided |
| D2 | Multi-user/multi-access design | HANDOFF §8 Q10, Q11 | Decided (design) |
| D3 | IA-ecosystem composition map | — | Decided |
| D4 | Server + auth stack = axum + schubert-only (proof-carrying tokens do authn AND authz); ia-auth rejected (closed-source, ~90% irrelevant SaaS machinery) | D3 web-framework fork | Decided (revised) |
| D5 | Schubert policy = Gr(4,8) dim 16, features std/crypto/policy, TOML vocabulary — derived from `schubert recommend` against Ijima's constraints (not hand-picked) | D3 authz detail | Decided (wired) |
| D6 | Storage backend: **SurrealDB primary** (native multi-tenancy + graph + embedded/server); abstract behind a `Store` trait; SQLite retained only as a migration-only read path | HANDOFF §3, §5, §8 Q4 | Decided |
| D7 | Vector search: Cosine similarity, **brute-force** now (correct, no ANN approximation); HNSW index is the planned optimization once the SDK emits typed `<N>f32` vectors (MTREE deprecated in SurrealDB 2.6) | D6 vector search | Decided (wired) |
| D8 | Surveyed Orlando / Karpal / Amari for the typed-vector + cosine problems — **no composition benefit for v0**; Minuet is the future path for richer retrieval | D7 follow-up | Decided (no wiring) |
| D9 | Incorporated `docs/discovery/memory-service-design.md` (parallel-context team design). Adopted: `Doctrine` origin, redaction-at-promotion boundary, multi-party hybrid model. Noted Postgres-vs-SurrealDB tension (SurrealDB stands). | D2, D6, memory model | Decided (aligned + Doctrine wired) |
|----|-------|----------|--------|
| D1 | Embedding/vector backend = candle | HANDOFF §8 Q3, Q4 | Decided |
| D2 | Multi-user/multi-access design | HANDOFF §8 Q10, Q11 | Decided (design) |
| D3 | IA-ecosystem composition map | — | Decided |
| D4 | Server + auth stack = axum + schubert-only (proof-carrying tokens do authn AND authz); ia-auth rejected (closed-source, ~90% irrelevant SaaS machinery) | D3 web-framework fork | Decided (revised) |
| D5 | Schubert policy = Gr(4,8) dim 16, features std/crypto/policy, TOML vocabulary — derived from `schubert recommend` against Ijima's constraints (not hand-picked) | D3 authz detail | Decided (wired) |
| D6 | Storage backend: **SurrealDB primary** (native multi-tenancy + graph + embedded/server); abstract behind a `Store` trait; SQLite retained only as a migration-only read path | HANDOFF §3, §5, §8 Q4 | Decided |
| D7 | Vector search: Cosine similarity, **brute-force** now (correct, no ANN approximation); HNSW index is the planned optimization once the SDK emits typed `<N>f32` vectors (MTREE deprecated in SurrealDB 2.6) | D6 vector search | Decided (wired) |
| D8 | Surveyed Orlando / Karpal / Amari for the typed-vector + cosine problems — **no composition benefit for v0**; Minuet is the future path for richer retrieval | D7 follow-up | Decided (no wiring) |

---

## D3. IA Ecosystem Composition (don't reinvent)

IA coding-standards principle #4: **compose, don't recreate.** Before
writing more Ijima code, this section maps every Ijima component to the
existing IA crates that already solve it, and records the fit. Several of
Ijima's hardest problems (D2 in particular) are largely *already solved*
by ecosystem crates.

### Composition map

| Ijima need | Ecosystem crate | Mechanism | Fit | Recommendation |
|---|---|---|---|---|
| **Authn** (who is calling: operators + harnesses) | `ia-auth` (member of `ia-rust-common` workspace) | git dep + `axum` feature; `JwtAuthLayer`, `AuthenticatedUser(Claims)`, `ApiKeyAuthLayer`, `ValidApiKey`, `ApiKeyRecord` | **Direct** — axum-native JWT + API-key middleware, password hashing, token stores (memory/db/redis), lockout, email verification | **Adopt** — this is the IA-standard authn layer; already consumed the same way by `ultramarine-red` and `sigmund` |
| **Authz** (per-action capability checks, namespace scoping) | `schubert` (crates.io 0.3.0, Apache-2.0) | crates.io dep; `PrincipalId`, `Capability`/`CapabilityKind` (ReadLike/WriteLike/AdminLike), `AccessDecision` (Granted/Impossible/Denied/Underconstrained), `audit`, `rate_limit`, `policy` (TOML), ed25519 capability tokens, CRDT for distributed policy | **Direct** — the capability model D2 calls for *is* Schubert; reuses the `CapabilityKind` axes (memory:read/write = ReadLike/WriteLike, admin = AdminLike) | **Adopt** — collapses D2 §3 (authz) entirely into composition |
| **Web framework + server scaffolding** | `ia-actix-common` (git, MIT OR Apache-2.0, v0.1.0) | health/CORS/error/middleware/db-pool/websocket — **actix-web only** | **Partial / tension** — Ijima is wired on axum; ia-actix-common is actix-only. ia-auth *does* ship axum middleware, so axum is viable without ia-actix-common | **Keep axum.** Do not adopt ia-actix-common (would force a framework switch). Reuse ia-auth's axum middleware; the health/CORS pieces are trivial and Ijima can vendor minimal versions or contribute axum equivalents upstream later |
| **Miner: LLM extraction engine** | `proserpina` (crates.io 0.2.0, Apache-2.0) | `Agent` trait + `AgentId` newtype, backend abstraction (`echo`/`http`/`roster`), `credentials` TOML config, `transcript`, `graph` interaction graph, `Panel` of personas, OpenAI-compatible HTTP provider | **Strong** — mining is a constrained LLM task; Proserpina's `Agent` trait + transcript/graph model fits "read session turns → propose extractions" | **Adopt as the miner substrate** behind `ijima-miner`'s `llm` feature. Proserpina's persona/panel model maps onto extraction roles (decision-extractor, fact-extractor, reference-extractor) |
| **Miner: agent loop / tool use** | `virtuoso` | Think-Act-Learn loop, holographic working memory, tool use, self-model, Claude backend | **Optional** — relevant if the miner needs tool-using agents (e.g. fetch URL → extract). Heavier; Proserpina alone may suffice for v0 | **Defer** — revisit if mining needs agentic tool-use rather than single-shot extraction |
| **Embeddings** (decided: candle) | `quantizon` candle pattern + `mishima` candle reference | Quantizon: `candle-core`/`nn`/`transformers` from git w/ optional cuda. Mishima: `CandleDriveInferencer` + `EpistemicEmbedding` | **Reference** — Quantizon is the established dep pattern; Mishima shows the loader stub | **Follow Quantizon's dep pattern**; revisit Mishima's epistemic scoring for mining-confidence reuse |
| **Memory store (semantic + KG)** | `minuet` (crates.io 0.3.0) | `MemoryStore`/`Retriever`/`MemoryTrace` traits, holographic binding algebra, sharded/layered stores, resonator retrieval, capacity/eviction, journaling | **Different paradigm** — Minuet is holographic/vector-symbolic (binding algebra), not embedding+SQLite. Not a drop-in store | **Do NOT replace the SQLite store.** Keep Minuet as a *future optional retrieval/consolidation tier* (its eviction/consolidation/journaling concepts may inform palace maintenance) |
| **MCP surface** (let pi/harnesses speak MCP) | `IA-MCP` / `amari-mcp` | config-driven MCP server exposing a Rust API as ground-truth reference | **Adjacent** — Ijima is primarily an HTTP daemon, but an MCP facade is a natural adapter for MCP-native harnesses | **Future adapter**, behind `ijima-client` or a new `ijima-mcp` crate; not v0 |
| **Capability crypto / provenance tokens** | `schubert::crypto` | ed25519 `CapabilityToken`/`Issuer`/`Verifier` | **Direct** — if mined-memory provenance needs signed, transferable provenance | **Reuse via Schubert** when provenance tokens land |

### What this collapses

Adopting `ia-auth` + `schubert` resolves most of **D2** without Ijima
authoring auth code:

- D2 §1 (identity model) → `ia-auth::AuthenticatedUser` (operators) +
  Ijima's existing `Harness` enum (harnesses). `PrincipalId` flows from
  Schubert.
- D2 §3 (authorization) → Schubert `Capability` + `AccessDecision`,
  scoped per namespace. No bespoke authz enum.
- D2 §6 (audit) → `schubert::audit`.
- Rate limiting → `schubert::rate_limit`.
- Password/token infra → `ia-auth` (argon2, JWT, refresh, API keys,
  lockout, token stores).

What Ijima still owns: the **namespace** dimension on the schema
(private/shared/global visibility per operator), the **session-context
repository**, the **mining review queue**, and the **SQLite + sqlite-vec
store + candle embeddings**. These are genuinely novel; the rest is
composition.

### Dependency mechanism (match ecosystem convention)

- `schubert`, `proserpina`, `minuet`, `amari-*` → **crates.io version
  pins** (they are published). Match the workspace version discipline
  (e.g. `amari-enumerative = "0.23"`).
- `ia-auth` / `ia-rust-common` / `ia-payments` → **git dependency**
  (`git = "https://github.com/Industrial-Algebra/ia-rust-common.git"`),
  matching `ultramarine-red` and `sigmund`. Not yet on crates.io.
- `ia-actix-common` → **not adopted** (framework mismatch).
- `candle-*` → **git dependency** matching Quantizon (`default-features
  = false`, optional cuda).

### Open sub-questions

1. **Auth scope**: does Ijima authenticate *operators* only (harnesses
   trust the operator's authenticated session and present a harness
   claim), or does each harness get its own `ApiKeyRecord` under an
   operator? `ia-auth` supports both (JWT for operators, API keys for
   harnesses) — likely both, mirroring how `ultramarine-red`/`sigmund`
   use it.
2. **Schubert policy model**: Ijima's capabilities (memory:read,
   memory:write, knowledge:write, session:ingest, mining:trigger,
   mining:review, admin) map onto `CapabilityKind::ReadLike`/
   `WriteLike`/`AdminLike`. Decide the Grassmannian parameters (k,n) and
   partition assignments when the policy TDD pass begins.
3. **Proserpina as miner**: confirm the extraction task fits
   Proserpina's single-shot `Agent::respond` vs needing multi-turn
   cross-examination. v0 mining is likely single-shot per session
   window; Proserpina's panel model is overkill until review queues need
   adversarial validation.
4. **License note**: `ia-rust-common` workspace is `MIT OR Apache-2.0`;
  `ia-actix-common` likewise. Ijima is `Apache-2.0` only. Composition
  is compatible (Apache-2.0 is one of the offered licenses). Confirm the
  CLA covers the git-dep consumption as the ia-rust-common repos mature.
