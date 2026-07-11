# Ijima — Roadmap

> The IA ecosystem's **Company Brain**: a centralized, multi-tenant
> memory service that every harness reads from and writes to. This
> roadmap supersedes the "Next Steps" in `docs/HANDOFF.md` §9, which
> predate the v0.1.0 implementation.

## Vision

Ijima is the single source of truth for agentic memory across the IA
stack — pi, Tsume, Sakamoto, Wallace, Dominic, opencode, and future
harnesses. It holds:

1. **The Memory Palace** — long-term semantic memory + knowledge graph.
2. **The Session Context Repository** — raw transcripts, mined into the
   palace.

The goal: any agent can ask *"what do we know about X?"* and get an
answer drawing on every harness's accumulated context — scoped to what
that agent is allowed to see.

## Current state (unreleased — toward 0.1.0)

> **Not yet shipped.** The features below are merged to `develop` (118
tests) but nothing is 0.1.0 until the crates are published to crates.io
> and the release is tagged. The authoritative status of what's merged:

| Capability | Status |
|---|---|
| Memory palace CRUD + list + dedup (hash + semantic) | ✅ |
| Semantic search (candle + cosine) | ✅ |
| Multi-tenancy (personal/shared/doctrine + isolation) | ✅ |
| Promotion (personal→shared, redacted) — gated by `trust:promote` | ✅ |
| Knowledge graph (temporal triples, timeline, stats) | ✅ |
| Session-context repository (sessions + turns) | ✅ |
| Doctrine ingest (Git → CI → service) | ✅ |
| Wake-up composition (L0 + L1a + L1b) | ✅ |
| Palace organization (graph, tunnels, rooms, taxonomy) | ✅ |
| Per-agent diaries | ✅ |
| **Mining pipeline** (rules + Proserpina llm, review queue, trigger) | ✅ |
| **Context Mapper** (global repo directory, CWD→project) | ✅ |
| **Provenance-tier model** (origin/authority + trust_grade + transition caps) | ✅ |
| Schubert auth (proof-carrying tokens, Gr(4,8)) + rate limiting | ✅ |
| Persistence (SurrealKv) + structured logging + backup/export + TLS | ✅ |
| `ijima-client` (typed harness adapter) | ✅ |
| `ijima token/serve/ingest/export/doctrine` CLI | ✅ |

See `docs/DESIGN.md` (D1–D11) and `docs/adr/` for the decision log.

**Remaining for/after 0.1.0:** backend-sqlite corpus migration (import the
live pi-mempalace corpus), pi integration (the migration's forcing
function), 3.5 multi-party handling, and the Phase 4/5 future features
(context-poisoning protection, federation).

---

## Phase 1 — Critical (blocks the mission)

> **✅ Merged to develop (unreleased).** Phases 1–3 below are retained as the historical
> design rationale for what landed; the authoritative status is the
> *Current state* table above. The remaining open work is [Phase 4/5
> (future features)](#phase-4--security-doctrine-health--context-poisoning-protection-proposed),
> the backend-sqlite corpus migration, and 3.5 multi-party.

### 1.1 Persistence

**Problem**: `SurrealStore::open_embedded()` uses the `Mem` engine —
pure in-memory. **All data is lost on every daemon restart.** A Company
Brain that forgets everything is not viable.

**Fix**: Add a persistent engine (`RocksDb` / `SurrealKv`) behind a
config option. `IJIMA_DIR/ijima.db` on disk; `Mem` retained for tests.
Construction change in `SurrealStore::open_*`; the `Store` trait is
unaffected.

**Effort**: Small. **Branch**: `feature/persistence`.

### 1.2 Knowledge Graph

**Problem**: The KG — entities + temporal triples — is **entirely
absent**. It's ~half the pi-mempalace RPC surface and the structured-
facts layer (*"Who depends on Y?" "What did we decide about X?"*).

**Surface to add** (pi-mempalace parity):
`add_triple`, `query_entity`, `invalidate_triple`, `kg_timeline`,
`find_triple`, `knowledge_stats`.

**Design**: SurrealDB's native graph (entities = nodes, triples = edges)
is a natural fit. Temporal validity (`valid_from`/`valid_to`) on edges.
Namespace-scoped, like memories.

**Effort**: Medium. **Branch**: `feature/knowledge-graph`.

---

## Phase 2 — Important (core value missing)

### 2.1 Stats & status

`GET /status` — memory counts, namespace counts, entity counts (once the
KG lands). Cheap, high daily-value for operators.

**Effort**: Small. **Branch**: `feature/stats`.

### 2.2 Dedup (`checkDuplicate`)

Content-hash + semantic-similarity check before `store_memory` accepts a
write. Without it the palace fills with near-duplicates — critical
before mining lands. The pi-mempalace `checkDuplicate` model is the
reference.

**Effort**: Medium. **Branch**: `feature/dedup`.

### 2.3 Session-context repository completion ✅

The `Session` struct (harness, channel, started/ended) **isn't stored**.
Add: `POST /sessions` (create w/ metadata), `GET /sessions` (list by
harness/namespace), session-end signaling. Completes the repository the
miner will read from.

**Done** (`feature/sessions`): `Store::create_session` / `list_sessions`
(with optional harness filter) / `end_session`; SurrealDB `sessions`
table (upsert by id); `POST /sessions`, `GET /sessions?harness=&namespace=&limit=`,
`POST /sessions/:id/end`; client methods; `Harness::from_wire_str`.

**Effort**: Medium. **Branch**: `feature/sessions`.

---

## Phase 3 — Nice-to-haves (organizational + operational)

### 3.1 Palace graph & tunnels ✅

`getPalaceGraph`, `traverseTunnel` — cross-project topic connections.
*"What connects these two projects?"* Powerful for discovery, not
essential for v1.

**Done** (`feature/palace-organization`): `Store::palace_graph` +
`traverse_tunnel`; `GET /palace/graph`, `GET /palace/tunnel`.

### 3.2 Rooms & taxonomy browsing ✅

`listRooms`, `getTaxonomy` — projects/topics with counts. Navigation.

**Done** (`feature/palace-organization`): `Store::list_rooms` +
`taxonomy`; `GET /rooms`, `GET /taxonomy`. All four palace-organization
endpoints are namespace-scoped reads (`memory:read` + `resolve_ns`).

### 3.3 Diaries

`diaryWrite`, `diaryRead` — per-agent journals. Niche; pi-specific.

### 3.4 Operational hardening

- **Structured logging** ✅ — `tracing`/`tracing-subscriber` replace
  `eprintln!`; `IJIMA_LOG` env filter (`ijima=info` default).
- **Rate limiting** ✅ — Schubert `RateLimiter` wired into the
  `AuthPrincipal` extractor (the capability token drives authn + authz +
  throughput). Capacity scales with the capability's Schubert intersection
  number (codimension): `memory:read`→1×, `memory:write`→2×, `admin`→16×.
  429 on exhaustion. `IJIMA_RATE_BASE`/`IJIMA_RATE_MULTIPLIER`/`IJIMA_RATE_DISABLE`.
- **Backup/export** — SurrealDB `db.export()` (SDK supports it; follow-up).
- **TLS** — plain HTTP today; acceptable on Tailscale, worth noting.

### 3.5 Multi-party handling (D9 §3)

The meeting hybrid model (auto-capture per-attendee → curated summary
promoted to team) is documented but not implemented.

---

## Phase 4 — Security: doctrine health & context-poisoning protection (proposed)

**Status: under investigation — blocked on an incident report.**

A trusted, front-loaded system/doctrine prompt degraded a GPT-5.6 session
to uselessness — but only on one task class (AI research: "overbearing and
sloppy"). Because the poison *is* trusted instructions, the usual
retrieved-content sandboxing defense does not apply. The leading hypothesis
is a **dose-dependent stance/persona directive** that activates on a
specific task domain and accreted via drift. Full analysis and resume
checklist: [`docs/discovery/context-poisoning-protection.md`](discovery/context-poisoning-protection.md).

Candidate directions (to be confirmed against the report):

- **B — Doctrine-health registry + outcome correlation** (drift detection,
  version↔outcome correlation, bisect + rollback). Likely the spine.
- **C — Stance-budget + task-domain profiles** (cap dose-dependent
  stance/persona directives at serve time; serve a curated, tested doctrine
  subset per task domain). The containment mechanism.
- **A-slice — Ingest-time stance-directive validation** (cheap static
  checks, not full behavioral regression). Catches obvious pathological
  directives before they serve.

**Gating open question:** is Ijima the authoritative doctrine store
(ingest+serve gating on the table), or is the poisoned context largely
harness-level (Ijima as an opt-in doctrine-health contract)? The incident
report must establish this before the design is finalized.

---

## Phase 5 — Federation: networked instances & cross-talk policies (proposed)

**Status: proposed future feature (v0.2+). Not blocked — forward design.**
Full analysis: [`docs/discovery/networked-instances-federation.md`](discovery/networked-instances-federation.md).

Multiple Ijima daemons federating, for use cases one shared daemon cannot
serve: airgap/sovereignty, offline/edge + central, multi-org/multi-trust-
domain federation, resilience, and hub-spoke specialization (a unifying
instance delegating to domain-authority instances or an archive/backup
instance).

**Architecture: Dominic orchestrates, Ijima enforces locally.** The
federation control plane lives in **Dominic** (`../Dominic`), the Anima
meta-orchestrator — not in Ijima. Ijima instances are the memory plane;
Dominic brokers cross-talk (routing, domain delegation, conflict
adjudication, offline coordination). Ijima does **not** need a P2P
consensus protocol — it needs boundary policy enforcement + a
Dominic-facing control API. The trust-tier egress and scope filters are
**Ijima-local and non-bypassable**, so sovereignty holds even when Dominic
is unreachable or compromised (defense in depth). *Note: Dominic is a
greenfield stub today; the orchestration-side work is net-new there.*

**Shared foundation with Phase 4:** both rest on an explicit
trust/provenance-tier model on `Memory` (`MemorySource` + origin-instance +
authority-scope). Design that extension first, in 0.1.x — it unlocks both
Phase 4 and Phase 5.

---

## Deferred (external dependencies)

- **`ijima-miner` extraction** — blocked until Proserpina is finalized.
  *(Proserpina 0.3.0 has shipped; the miner is implemented and merged.
  This entry is retained for history.)*
- **`backend-sqlite` migration** — one-time import of the live
  pi-mempalace corpus. Self-contained; pick up when migrating off the
  Node.js mempalace.

---

## Workflow

All work follows **IA Gitflow** (`docs/CONTRIBUTING.md`):

1. Branch from `develop` (`feature/...`, `docs/...`, `chore/...`).
2. Implement with TDD (test first).
3. PR → `develop`. CI must pass (fmt, clippy `--all-features -D
   warnings`, tests, docs).
4. Human review + merge.
5. Release PR `develop` → `main` when cutting a release.
