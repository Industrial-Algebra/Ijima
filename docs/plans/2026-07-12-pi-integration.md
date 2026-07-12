# Plan: pi ↔ Ijima integration (`integrations/pi/`)

> **Status:** Plan, 2026-07-12. Replaces the `pi-mempalace` extension with an
> Ijima-backed one, so pi's memory federates across workstations via Ijima.
> Lives in `integrations/` (room for Wallace + other harnesses later).

## 1. Goal & scope

**Full replacement** of the current `pi-mempalace` npm extension. pi's memory
tools, auto-capture, and wake-up all route to an Ijima daemon over HTTP. No
local SQLite fallback — Ijima *is* the store (and the federation substrate
for Phase 5's multi-workstation cross-talk).

**Out of scope** (explicit): cross-workstation federation itself (Phase 5);
this integration is a single-workstation pi→Ijima client. Federation comes
for free once each workstation runs Ijima + this integration.

## 2. Architecture

```
   pi  ──(extension: integrations/pi/index.ts)──▶  IjimaClient (TS, HTTP)
                                                       │  Bearer capability token
                                                       ▼
                                              ijima daemon (HTTP, SurrealDB)
```

- A TypeScript pi extension (pi extensions are TS modules using
  `@…/pi-coding-agent`'s `ExtensionAPI`).
- An `IjimaClient` class that maps each memory operation to Ijima's REST API.
- Same tool surface + lifecycle hooks as `pi-mempalace`, so pi's behavior is
  unchanged from the agent's POV — only the backend swaps.

## 3. Tool surface mapping (17 tools → Ijima routes)

| pi tool | Ijima route | Capability |
|---|---|---|
| `memory_search` | `POST /memories/search` | `memory:read` |
| `memory_save` | `POST /memories` | `memory:write` |
| `memory_recall` | `GET /memories?project=&topic=` (namespace-scoped list) | `memory:read` |
| `memory_status` | `GET /status` | `memory:read` |
| `memory_delete` | `DELETE /memories/{id}` | `memory:write` |
| `memory_check_duplicate` | `POST /memories/check` | `memory:read` |
| `memory_graph` | `GET /palace/graph` | `memory:read` |
| `memory_tunnel` | `GET /palace/tunnel?…` | `memory:read` |
| `memory_list_rooms` | `GET /rooms` | `memory:read` |
| `memory_taxonomy` | `GET /taxonomy` | `memory:read` |
| `knowledge_add` | `POST /kg/triples` | `knowledge:write` |
| `knowledge_query` | `GET /kg/entities/{id}` | `knowledge:read` |
| `knowledge_status` | `GET /kg/stats` | `knowledge:read` |
| `knowledge_invalidate` | `POST /kg/triples/{id}/invalidate` | `knowledge:write` |
| `knowledge_timeline` | `GET /kg/timeline` | `knowledge:read` |
| `memory_diary_write` | `POST /diaries` | `memory:write` |
| `memory_diary_read` | `GET /diaries/{agent}` | `memory:read` |

The shapes are close but **not identical** — the integration owns the
translation (e.g. pi's `{project, topic, n_results}` recall → Ijima's
namespace-scoped list with project/topic filters; Ijima `Memory` fields →
pi's `{text, project, topic, timestamp, …}` result shape). A thin adapter
layer per tool.

## 4. Lifecycle hooks (preserve from pi-mempalace)

- **`session_start` / `session_tree`**: reconstruct state — ping Ijima
  `/status`, cache counts, pre-fetch wake-up text.
- **`turn_end`** (auto-capture): after each assistant turn, store the
  exchange via `POST /memories` (source `AutoCapture`, project = cwd-derived).
- **`before_agent_start`** (wake-up): fetch `GET /wakeup` and inject into the
  system prompt (same "Agent Memory (ACTIVE)" preamble).
- **`session_shutdown`**: clear runtime cache.
- **`/memory` command** + stats overlay widget: port as-is (calls Ijima).

## 5. Auth & config

- `IJIMA_URL` (default `http://127.0.0.1:7373`), `IJIMA_TOKEN` (a Schubert
  capability bearer). **One token can't hold every capability** (Schubert
  tokens are one-cap-each) — so the integration holds a **small bundle of
  tokens** (`IJIMA_TOKEN_MEMORY_WRITE`, `…_MEMORY_READ`, `…_KNOWLEDGE_*`,
  `…_MINING_*`) and picks the right one per call. *Open question §11.*
- Config file (`~/.pi/agent/memory/config.json`) for autoCapture/wake-up
  toggles — same as today.

## 6. Namespace & project model

- pi writes to the **principal's namespace** encoded in its tokens
  (`ns_<principal>_private`), matching Ijima's D2 multi-tenancy. **Decision
  (2026-07-12): private namespace** — clean isolation, correct long-term.
- pi-mempalace's `project` → Ijima `Memory.project` (a field, not a
  namespace). Project auto-detected from cwd (reuse `detectProject`).
- **Migrated-corpus implication:** the corpus was migrated into `global`,
  so it won't mingle with new private writes by default. Cutover options:
  (a) re-run `ijima migrate` targeting the principal's private namespace,
  or (b) pi reads across `global` + private (Ijima `resolve_ns` permits
  shared/global reads). Default: (a) re-migrate into private so the
  operator's full history lives where new writes land.

## 7. Packaging / how pi loads it

Two viable options (decide in §11):
- **(A) Load by path**: `settings.json` `extensions` array points at
  `<ijima-repo>/integrations/pi/index.ts`. Simplest, dev-friendly, no publish.
- **(B) npm package**: publish `@anima/ijima-pi` (or similar) and load via
  `npm:…` like today's `pi-mempalace`. Cleaner for installing across
  workstations without a repo checkout.

Either way: `integrations/pi/` holds `package.json`, `index.ts`, `ijima_client.ts`,
`tsconfig.json`; `fetch`/`undici` for HTTP (no native deps).

## 8. Cutover

1. Build the integration + an `ijima-client` TS module.
2. On one workstation: run `ijima serve`; `ijima migrate` the local corpus in
   (already done for this machine); point pi at it via the extension.
3. Verify parity (search/save/wake-up/KG) against the migrated data.
4. Swap `npm:pi-mempalace` → the Ijima integration in `settings.json`.
5. Repeat per workstation (each runs its own Ijima; federation lands in Phase 5).

## 9. Implementation phases

1. **Scaffold** `integrations/pi/` (package.json, tsconfig, index.ts skeleton).
2. **IjimaClient (TS)** — one method per route, bearer auth, error mapping.
   Unit-test the mapping against a stubbed fetch (Vitest or node:test).
3. **Memory tools** (search/save/recall/status/delete/check_duplicate) — the
   highest-value slice; ship + verify before the rest.
4. **Lifecycle hooks** (auto-capture + wake-up) — the "feels like mempalace"
   behavior.
5. **Knowledge graph + palace + diary tools** — parity completeness.
6. **`/memory` command + stats widget** — port.
7. **Cutover** on one workstation; document the per-workstation setup.

## 10. Risks / watch-items

- **Capability-token bundle** — Schubert's one-cap-per-token model means the
  integration juggles several tokens. A future "scoped multi-cap token" in
  Schubert would simplify this; until then, a token bundle + per-call pick.
- **Shape drift** — Ijima's REST shapes ≠ pi-mempalace's exactly; the adapter
  layer must be tested, not assumed.
- **Latency** — every memory op is now HTTP (was in-process). Wake-up adds a
  round-trip to session start. Acceptable; cache aggressively.
- **Offline** — if Ijima is down, pi's memory tools degrade (graceful: tools
  return "memory unavailable", session continues). No local fallback by design.
- **SurrealDB perf** — the index fix (#33) is in; verify search/recall stay
  snappy over HTTP with the full corpus.

## 11. Decisions (2026-07-12) + remaining defaults

1. **Namespace: PRIVATE** (decided) — pi writes to `ns_<principal>_private`.
   Cutover re-migrates the corpus into private so history + new writes
   coexist (§6).
2. **Packaging: load-by-path for v1** (default) — `settings.json` extensions
   points at `<ijima-repo>/integrations/pi/index.ts`. Dev-simple, no publish.
   Revisit npm packaging when distributing across workstations.
3. **Token strategy: env-bundle of one-cap Schubert tokens** (default) —
   `IJIMA_TOKEN_MEMORY_WRITE/READ`, `…_KNOWLEDGE_*`, etc., picked per call.
   A future Schubert multi-capability token would simplify this; deferred.
4. **Stats widget + `/memory` command: port verbatim** (default) — full
   parity with pi-mempalace for v1.
