# Handoff: pi integration — port the remaining tools (Phase 2 + KG)

**Status:** ready to execute. **Branch:** `feature/pi-integration`.
**Prerequisite reads:** [`docs/plans/2026-07-12-pi-integration.md`](../plans/2026-07-12-pi-integration.md) §3 (tool map) + §3.6 (validated capability map + gaps).
**Phase 1 is DONE + e2e-proven:** `memory_search` works end-to-end (jiti loads `index.ts`, wasm core maps shapes, live daemon returns ranked results). The scaffold is the reference implementation — copy its pattern.

> **Efficiency note:** ~90% mechanical (replicate a proven pattern 8×, add wasm
> serde pairs, wire tokens). Run on **DeepSeek-v4-pro**. One work unit per
> context — this handoff covers ONLY the 8 ready tools below; blocked tools +
> lifecycle hooks + cutover are later sessions.

---

## Starting point (the proven reference)
- `ijima-pi/src/lib.rs` — `build_search_request` + `parse_search_response` (the
  pattern: a `#[wasm_bindgen] pub fn` returning a JSON `String`; deserialize
  via `ijima-core` types, re-serialize to a pi-friendly subset).
- `integrations/pi/index.ts` — registers `memory_search` (the pattern: pick
  token by capability, `fetch`, parse, format, graceful-offline).
- Build: `wasm-pack build ijima-pi --target nodejs --out-dir ../integrations/pi/pkg`
  (rebuild after editing lib.rs). `--target nodejs` NOT web (Node `fetch()` can't
  load `file://` wasm).

## REFACTOR FIRST: extract a `makeTool` factory
Before adding tool #2, factor the `memory_search` boilerplate out of `index.ts`
into a helper so the 8 tools aren't 8× copy-paste. Every tool does:
1. pick the capability's bearer token from env
2. `fetch` (GET/POST/DELETE) with the token
3. handle non-OK / offline / parse-error uniformly
4. format a text result

Suggested shape (adapt as needed):
```ts
type Capability = "memory:read" | "memory:write" | "knowledge:read" | "knowledge:write";
const TOKEN_ENV: Record<Capability, string> = {
  "memory:read": "IJIMA_TOKEN_MEMORY_READ",
  "memory:write": "IJIMA_TOKEN_MEMORY_WRITE",
  "knowledge:read": "IJIMA_TOKEN_KNOWLEDGE_READ",
  "knowledge:write": "IJIMA_TOKEN_KNOWLEDGE_WRITE",
};
function ijimaFetch(path: string, cap: Capability, init?: RequestInit): Promise<{ok, status, text}>
```
Then each tool's `execute` is ~10 lines of param→body mapping + result formatting.

## Tools to port (8 — all have working backends)

For each: wasm pair (`build_X_request` / `parse_X_response`) in `lib.rs` +
tool registration in `index.ts`. Token = the capability listed.

### Memory tools (3)

| Tool | Route | Cap | Request body | Response |
|---|---|---|---|---|
| `memory_save` | `POST /memories` | memory:write | full `Memory` JSON (id can be dummy; daemon fills `created_at` if empty; `source`="Explicit", `harness`="Pi" are sane pi defaults; `project`/`topic`/`content`/`importance` from params) | `{"id": "mem_..."}` |
| `memory_delete` | `DELETE /memories/{id}` | memory:write | — (id in path) | `204 No Content` (empty body) |
| `memory_check_duplicate` | `POST /memories/check` | memory:read | `{"content": "..."}` | `{"duplicate": "mem_.." \| null}` |

**Pi param shapes** (match pi-mempalace for drop-in):
- `memory_save(content, project?, topic?, importance?)` — `importance` defaults 0.8.
- `memory_delete(id)`.
- `memory_check_duplicate(content, threshold?)` — Ijima ignores `threshold` (exact content-hash dedup); accept it, don't use it.

**Memory JSON shape** (for `memory_save` body — `#[serde(default)]` fields optional):
```json
{"id":"mem_x","content":"...","project":"...","topic":"...","source":"Explicit","harness":"Pi","session_id":null,"importance":0.8,"created_at":""}
```
(`origin`/`authority` default server-side; omit or send `"local"`.)

### Knowledge-graph tools (5)

| Tool | Route | Cap | Request | Response |
|---|---|---|---|---|
| `knowledge_add` | `POST /kg/triples` | knowledge:write | `{"subject","predicate","object","valid_from"?,"confidence"?,"source_memory_id"?}` | `Triple` |
| `knowledge_query` | `GET /kg/entities/{id}` | knowledge:read | — (entity id in path) | `EntityRecord` |
| `knowledge_status` | `GET /kg/stats` | knowledge:read | — | `{"entities":N,"triples":N}` |
| `knowledge_invalidate` | `POST /kg/triples/{id}/invalidate` | knowledge:write | — (triple id in path) | `204` |
| `knowledge_timeline` | `GET /kg/timeline?limit=N` | knowledge:read | — | `[Triple]` |

**Pi param shapes:**
- `knowledge_add(subject, predicate, object, valid_from?, valid_to?, project?)` —
  Ijima's `add_triple` has no `valid_to`/`project`; accept them, ignore `valid_to`,
  ignore `project` (KG is namespace-scoped server-side). `confidence` defaults 1.0.
- `knowledge_query(entity, at_time?, project?)` — `entity`→path id; `at_time`/`project` ignored for now.
- `knowledge_invalidate(subject, predicate, object, ended?)` — ⚠️ pi identifies
  by (subject,predicate,object); Ijima's route takes a **triple id**. The wasm/TS
  must first `find_triples` (`GET /kg/triples?subject=&predicate=&object=`,
  knowledge:read) to resolve the id, THEN invalidate. **This tool needs two
  HTTP calls** (or skip until Ijima adds a by-predicate invalidate).
- `knowledge_timeline(entity?)` — `entity` ignored (endpoint is whole-namespace).

**Types** (for wasm `parse_*` — `EntityId` is `#[serde(transparent)]` String):
- `Triple` = `{id, subject:String, predicate, object:String, valid_from?, valid_to?, confidence:f32, namespace, source_memory_id?}`
- `EntityRecord` = `{entity?: {id, name, entity_type}, outgoing:[Triple], incoming:[Triple]}`
- `KgStats` = `{entities:usize, triples:usize}`

## How to test (daemon is up; tokens at /tmp)
The daemon was left running (nohup'd). Verify + re-mint if needed:
```bash
cd ~/working/industrial-algebra/Ijima
./target/debug/ijima serve &              # if not already running
./target/debug/ijima token issue --principal elliott --capability memory:write
./target/debug/ijima token issue --principal elliott --capability knowledge:read
./target/debug/ijima token issue --principal elliott --capability knowledge:write
```
**Two test layers** (both required for "done"):
1. **curl** each endpoint against the live daemon (validates route + capability + shape).
2. **jiti-load** the extension (the proven script at `/tmp/jiti-load.cjs` pattern —
   `createJiti` with `alias:{typebox}`, `jiti.import(absPath, {default:true})`, mock
   `pi.registerTool`, then call `execute()`). Invoke each new tool's execute().

For write tools, curl to seed data then verify via the read tools.

## Done criteria (all must pass)
- `cargo fmt --check` clean; `cargo clippy --all-features --all-targets -- -D warnings` clean; `cargo test --all-features` green (add a unit test per wasm pair).
- `wasm-pack build --target nodejs` succeeds.
- Each tool: a curl round-trip against the live daemon returns correct shape.
- At least 3 tools (one write + two read across memory+KG) pass the jiti-load execute() test.
- Commit to `feature/pi-integration`; push.

## BLOCKED tools (do NOT port — no backend)
- `memory_recall` — pi's browse-by-project/topic needs `GET /memories?project=&topic=`; Ijima only has `GET /memories/{id}` (single recall). Needs a list endpoint added to Ijima first.
- `memory_status` — `/status` requires `admin`, not `memory:read` (§3.6). Needs an admin token or a new read-accessible status endpoint.
- `memory_graph`/`memory_tunnel`/`memory_list_rooms`/`memory_taxonomy` — `/palace/*`, `/rooms`, `/taxonomy` routes don't exist.
- `memory_diary_write`/`memory_diary_read` — `/diaries` routes don't exist.

These are tracked in the plan §3.6; they need Ijima backend work (a separate session), not integration work.

## Out of scope for this handoff
Lifecycle hooks (Phase 3), the blocked backend additions, `/memory` command,
stats widget, and cutover — all later fresh contexts.
