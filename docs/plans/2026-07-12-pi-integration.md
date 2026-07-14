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

## 2. Architecture (Rust core → WebAssembly + thin TS shim)

```text
   pi  ──(integrations/pi/index.ts: thin TS shim)──▶  Rust core (wasm)
        registers tools / lifecycle, holds pi's ctx/ui           │
                                                                 ▼
                                                          ijima daemon (HTTP)
```

**Decision (2026-07-12): build the core in Rust, compile to WebAssembly.**
Both pi runtimes (Node, Bun) load wasm. This reuses `ijima-client` +
`ijima-core` types directly (no TS re-port of the client surface), unifies
CI under `cargo`, and keeps the type-safe mapping logic in Rust.

- **Thin TS shim** (`index.ts`) — the only part that *must* be TS: it calls
  `pi.registerTool` / `pi.on(...)` and holds the TS `ExtensionContext`
  (sessionManager, ui). Its handlers marshal params/results across the wasm
  boundary and call the Rust core.
- **Rust core** (new crate `ijima-pi`, or a `wasm/` target in the workspace)
— the `IjimaClient` + every tool's request/response mapping, compiled to
  `wasm32-unknown-unknown` and bound to JS via `wasm-bindgen`.

**The seam to de-risk first (phase-0 spike):** `ijima-client` is async over
`reqwest`+`tokio`, and tokio doesn't run on wasm. Two viable paths —
(a) make `ijima-client` wasm-compatible (reqwest js/fetch backend +
`wasm-bindgen-futures`), giving full client reuse; or (b) keep HTTP in the
TS shim (host fetch, native to Node/Bun) and put only the **pure mapping +
types** in the wasm core. Path (b) sidesteps the tokio/reqwest-wasm port but
reuses less. The spike decides; (b) is the safe fallback.

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

### 3.6 VALIDATED capability map + backend gaps (2026-07-13 e2e checkpoint)

The table above was written from assumption, not inspection. A live
e2e checkpoint (`ijima serve` + `memory_search` via the wasm shim, fully
working) corrected it by reading the actual `principal.0.may(...)` checks
in `api.rs`. **Corrections:**

- **`memory_status` → `/status` requires `admin`, NOT `memory:read`.**
  Either the pi tool carries an admin token, or Ijima adds a
  `memory:read`-accessible status/count endpoint. Until then this tool is
  blocked or needs the admin capability.
- **Six tools map to routes that DO NOT EXIST in Ijima** (the table
  assumed pi-mempalace parity): `memory_graph`, `memory_tunnel`,
  `memory_list_rooms`, `memory_taxonomy` (`/palace/*`, `/rooms`,
  `/taxonomy`) and `memory_diary_write`, `memory_diary_read` (`/diaries`).
  These need Ijima backend work before the integration can port them.

**Validated map (from `may()` inspection):** `/memories/search`,
`/memories/check`, `GET /memories/{id}`, `/wakeup`, `GET /sessions`,
`GET /sessions/{id}/turns` → `memory:read`. `POST /memories`, `DELETE
/memories/{id}` → `memory:write`. `/status`, `/doctrine` → `admin`.
`/memories/{id}/promote` → `trust:promote`. `/kg/triples` POST +
`/kg/triples/{id}/invalidate` → `knowledge:write`; `/kg/entities/{id}`,
`GET /kg/triples`, `/kg/timeline`, `/kg/stats` → `knowledge:read`.
`POST /sessions`, `/sessions/{id}/end`, `POST /sessions/{id}/turns` →
`session:ingest`. `/mining/queue` + accept/reject → `mining:review`;
`/sessions/{id}/mine` → `mining:trigger`.

**Net: 11 of 17 tools have working backends; 6 are blocked on Ijima
backend additions.** Phase 2 can ship the memory tools (minus status);
phase 4 must add the palace/diary endpoints first.

### 3.5 Search scope — the one real design gap

**The mismatch:** pi-mempalace's `memory_search` is **global** (every
memory, every project). Ijima's `POST /memories/search` resolves to the
principal's **single** namespace. So a pi search in `ns_elliott_private`
will not see the migrated `global` corpus or anything in `shared` — a
visible regression ("search used to find X, now it doesn't").

**Decision: Ijima gains a visible-scope search.** A principal's readable
scope is `{own private, shared, global}` (Ijima's `resolve_ns` already
permits these reads). Add a search mode that queries each readable
namespace and **merges results server-side** with proper cosine ranking
(client-side fan-out would re-rank N lists less accurately and add N round
trips). Concretely: `POST /memories/search` accepts a `scope` param
(`personal` | `visible`, default `visible` for the pi integration); the
daemon expands `visible` to the readable set, queries each, and merges.

This is a small, bounded daemon addition (a search-aggregation path over
existing per-namespace search) and it preserves pi-mempalace's global-search
semantics. The pi extension calls with `scope=visible`. `recall`/`status`/
palace browsing get the same visible-scope treatment where it matters.

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

- `IJIMA_URL` (default `http://127.0.0.1:7373`).
- **Decision (2026-07-12): env-bundle of one-cap Schubert tokens for 0.1.0.**
  Schubert tokens are one-cap-each today, so the integration holds a small
  bundle — `IJIMA_TOKEN_MEMORY_WRITE`, `…_MEMORY_READ`, `…_KNOWLEDGE_*`,
  `…_MINING_*` — and picks per call. This is a deliberate stopgap: Elliott
  will add **multi-capability tokens** to Schubert's next release (one
  token = a grant of many caps), after which the bundle collapses to one
  token. Requirements captured in
  [`../Schubert/docs/handoff-multi-capability-tokens.md`](../../Schubert/docs/handoff-multi-capability-tokens.md)
  (Schubert PR #29).
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

**Decision (2026-07-12): load-by-path for v1.** `settings.json` `extensions`
points at `<ijima-repo>/integrations/pi/index.ts`. Dev-simple, no publish.
Revisit npm packaging (`@anima/ijima-pi`) when distributing across
workstations.

Contents: `integrations/pi/` holds `package.json`, `index.ts` (the thin TS
shim), `tsconfig.json`, and the built wasm artifact (`ijima_pi.wasm` +
`ijima_pi.js` glue from `wasm-pack`). The Rust core lives in a workspace
crate (`ijima-pi` or `integrations/pi-core/`) and is built into the
artifact; CI (`cargo`) type-checks/tests the Rust core.

## 8. Cutover

0. **Prerequisites:** add `--namespace` to `ijima migrate`; add the
   `scope=visible` search mode to the daemon (§3.5).
1. Build the integration (wasm core + TS shim).
2. On one workstation: run `ijima serve`; `ijima migrate --namespace
   ns_<principal>_private` so the corpus lands where pi writes.
3. Verify parity (search/save/wake-up/KG) against the migrated data.
4. Swap `npm:pi-mempalace` → the Ijima integration in `settings.json`.
5. Repeat per workstation (each runs its own Ijima; federation lands in Phase 5).

## 9. Implementation phases

0. **Prerequisites / de-risk.**
   - `ijima migrate --namespace <ns>` (small; unblocks private-namespace cutover). **DONE.**
   - `scope=visible` search mode on the daemon (§3.5). **DONE** — required
     scored search, so `Store::search_memories` now returns
     `Vec<SearchHit>` (memory + cosine similarity; SurrealStore already
     computed the score, was discarding it); `scope=visible` merges the
     principal's private namespace + `global` via a pure, tested
     `merge_search_hits`.
   - **Wasm spike: RESOLVED → path (b).** Path (a) (full ijima-client wasm
     reuse) is blocked — the workspace tokio (`net`/`rt-multi-thread`) drags
     `mio`, native-only. Path (b) confirmed: `ijima-core` (+serde) compiles to
     `wasm32-unknown-unknown` clean (3.3s). So the wasm core reuses the domain
     types + serde for type-safe request/response mapping; HTTP stays native in
     the TS shim (host fetch). No tokio/reqwest in the wasm core.
1. **Scaffold** `integrations/pi/` + the Rust core crate (wasm-bindgen setup). **DONE** (e2e-validated 2026-07-13: jiti loads index.ts, memory_search runs against a live daemon, ranked results correct).
2. **Core + memory tools** (search/save/recall/status/delete/check_duplicate)
   — the highest-value slice. search **DONE + e2e-proven**; status blocked on
   the admin-vs-read issue (§3.6); the rest are mechanical repetition of the
   proven pattern.
3. **Lifecycle hooks** (auto-capture + wake-up) — the "feels like mempalace"
   behavior.
4. **Knowledge graph + palace + diary tools** — ⚠️ **BLOCKED**: the 4 palace +
   2 diary routes do not exist in Ijima yet (§3.6). KG tools (5) are
   unblocked. Add the missing backend endpoints before porting the other 6.
5. **`/memory` command + stats widget** — port.
7. **Cutover** on one workstation; document the per-workstation setup.

## 10. Risks / watch-items

- **Wasm HTTP path** (the de-risk spike, §9.0) — `ijima-client` is async over
  reqwest+tokio, and tokio doesn't run on wasm. Verify path (a) wasm client
  reuse vs (b) HTTP-in-shim before committing; (b) is the safe fallback.
  Bun's wasm support lags Node's — test on both runtimes.
- **Search scope** — the §3.5 `scope=visible` daemon change is required for
  parity; without it pi search silently misses `global`/`shared` memories.
- **Capability-token bundle** — env-bundle for 0.1.0 (§5); collapses to one
  token once Schubert ships multi-cap grants (handoff: Schubert PR #29).
- **Shape drift** — Ijima's REST shapes ≠ pi-mempalace's exactly; the adapter
  layer must be tested, not assumed. Verify the `recall` and `wakeup` route
  shapes before building tools on them.
- **Latency** — every memory op is now HTTP (was in-process). Wake-up adds a
  round-trip to session start. Acceptable; cache aggressively.
- **Offline** — if Ijima is down, pi's memory tools degrade (graceful: tools
  return "memory unavailable", session continues). No local fallback by design.
- **SurrealDB perf** — the index fix (#33) is in; verify search/recall stay
  snappy over HTTP with the full corpus.

## 11. Decisions (2026-07-12)

1. **Architecture: Rust core → WebAssembly + thin TS shim.** Reuses
   `ijima-client`/`ijima-core`, unifies CI under cargo. Phase-0 wasm spike
   de-risks the HTTP path (§2, §9.0).
2. **Namespace: PRIVATE** — pi writes to `ns_<principal>_private`; cutover
   re-migrates the corpus there (`ijima migrate --namespace`, §8.0).
3. **Search scope: Ijima `scope=visible`** — multi-namespace server-side
   merge so pi search matches pi-mempalace's global semantics (§3.5).
4. **Packaging: load-by-path for v1** — revisit npm when distributing.
5. **Tokens: env-bundle for 0.1.0** — collapses to one token after Schubert
   multi-cap grants land (Schubert PR #29).
6. **Stats widget + `/memory` command: port verbatim** for v1 parity.
