# Handoff: Ijima backend — the blocked routes (unblock the remaining pi tools + 1 Dominic dep)

**Status:** ready to execute. **Branch:** work off `feature/pi-integration`.
**Prerequisite read:** [`docs/plans/2026-07-12-pi-integration.md`](../plans/2026-07-12-pi-integration.md) §3.6 (the validated capability map + which tools are blocked and why).

> Groups A–C unblock the 8 pi tools. **Group D** (RepoDirectory) is a separate
driver — a Dominic/Tsume dependency (CWD → project resolution), tracked
in [`../../Dominic/docs/handoff/ijima-backend-dependencies.md`](../../Dominic/docs/handoff/ijima-backend-dependencies.md).
It's the same store+route gap pattern, so it rides in this handoff.

## The key finding — this is mostly mechanical route-wiring, NOT new backend logic

The e2e checkpoint flagged 8 pi tools as "blocked" because their HTTP routes don't
exist in `api.rs`. But investigation shows **the entire store layer is already built
and implemented in SurrealDB**: `list_rooms`, `taxonomy`, `palace_graph`,
`traverse_tunnel`, `write_diary`, `read_diary`, `list_memories` are all declared in
the `Store` trait (`ijima-core/src/store.rs`) AND implemented in
`SurrealStore` (`ijima-server/src/backend_surreal.rs`).

So 6 of the 8 tools need **zero new store logic** — just an HTTP route + handler that
calls the existing method. Only `memory_recall` needs a small filtered-list addition,
and `memory_status` needs a read-accessible stats derivation.

> **Efficiency note:** ~80% mechanical (route + handler + test, copying the existing
> pattern). Run on **DeepSeek-v4-pro**. TDD per IA standards (test the route first,
> see it fail, implement). One work unit — backend routes only; the integration ports
> (wasm+TS for these 8 tools) are a **follow-on handoff** once routes exist.

---

## Handler pattern (copy this — from `wakeup`, api.rs:539)

Every new handler is the same shape:
```rust
async fn my_handler(
    principal: AuthPrincipal,
    Extension(store): Extension<Arc<dyn Store>>,
    [Path/Query/Json extractors as needed],
) -> Result<Json<MyResponse>, ApiError> {
    if !principal.0.may(THE_CAPABILITY) {
        return Err(ApiError::Forbidden);
    }
    let ns = principal.0.personal_namespace();   // or resolve_ns(&principal, q.namespace.as_deref())?
    let result = store.the_method(&ns, ...).await.map_err(internal)?;
    Ok(Json(result))
}
```
Register in the router builder (api.rs ~line 68-92): `.route("/my-path", get(my_handler))`.

---

## Group A — pure route-wiring (6 routes, no store changes)

These call existing `Store` methods directly. Add route + handler + test for each.
**All `memory:read`** capability (except diary write).

| Route | Store method (signature in store.rs) | Returns | Cap |
|---|---|---|---|
| `GET /rooms?project=&limit=` | `list_rooms(ns, project: Option<&str>, limit)` (line 100) | `Vec<Room>` `{project, topic, count}` | memory:read |
| `GET /taxonomy` | `taxonomy(ns)` (line 109) | `Vec<ProjectTaxon>` `{project, rooms: Vec<Room>, total}` | memory:read |
| `GET /palace/graph` | `palace_graph(ns)` (line 113) | `PalaceGraph` `{projects: Vec<String>, tunnels: Vec<Tunnel>}` | memory:read |
| `GET /palace/tunnel?topic=&project_a=&project_b=&limit=` | `traverse_tunnel(ns, topic, project_a, project_b, limit)` (line 117) | `TunnelTraversal` `{topic, project_a, project_b, memories_a, memories_b}` | memory:read |
| `POST /diaries` (body: `DiaryEntry`) | `write_diary(ns, entry)` (line 165) | `204` (or `IdResponse`-style ack) | memory:write |
| `GET /diaries/{agent}?limit=` | `read_diary(ns, agent, limit)` (line 169) | `Vec<DiaryEntry>` `{agent, content, topic?, timestamp}` | memory:read |

**Query structs:** define a small `#[derive(Deserialize)]` query struct per GET route
(e.g. `RoomsQuery { project: Option<String>, limit: Option<usize> }`, default limit ~50).
The palace + diary types are re-exported from `ijima-core` (`palace::{Room, ProjectTaxon,
PalaceGraph, TunnelTraversal}`, `diary::DiaryEntry`) and already derive Serialize —
return them directly.

**DiaryEntry POST body shape:**
```json
{"agent": "claude", "content": "entry text", "topic": "optional", "timestamp": "2026-07-13T..."}
```

## Group B — memory_recall (1 small store addition + 1 route)

pi's `memory_recall(project?, topic?, n_results?)` browses memories by project/topic.
The existing `list_memories(ns, limit)` has **no project/topic filter** (it's
importance-ranked for wake-up). Add a filtered variant with a **default trait impl**
(no forced backend change):

In `ijima-core/src/store.rs`, add to the `Store` trait:
```rust
/// Lists memories in `ns`, optionally filtered to project/topic. For
/// `memory_recall` browsing (not wake-up ranking).
async fn list_memories_filtered(
    &self, ns: &NamespaceId, project: Option<&str>, topic: Option<&str>, limit: usize,
) -> Result<Vec<Memory>> {
    // Default: fetch a cap, filter in Rust, truncate. Backends with native
    // filtered queries MAY override (SurrealQL: add AND project=$p AND topic=$t
    // to the list_memories WHERE clause — see backend_surreal.rs:600).
    let cap = if project.is_some() || topic.is_some() { 500 } else { limit };
    let mut mems = self.list_memories(ns, cap).await?;
    if let Some(p) = project { mems.retain(|m| m.project == p); }
    if let Some(t) = topic { mems.retain(|m| m.topic == t); }
    mems.truncate(limit);
    Ok(mems)
}
```
Then route: `GET /memories?project=&topic=&limit=` (add `.get(recall_handler)` to the
existing `/memories` route, which currently is `post(store_memory)` only).
**Cap: memory:read.** Returns `Vec<Memory>` directly (already Serializes).

> Optional optimization (NOT required for 0.1.0): override in `SurrealStore` with a
> native filtered query. The default impl is correct for the ~8k corpus.

## Group C — memory_status (1 route, derive stats)

`/status` is admin-gated (§3.6). Add a **read-accessible** stats endpoint instead:
`GET /memories/stats` → `memory:read` → derive the principal's namespace breakdown.

Simplest correct derivation: call `list_rooms(ns, None, 1000)` (all topics), sum counts
for the total, group by project. Return a small struct:
```rust
#[derive(Serialize)]
struct NamespaceStats { total: usize, projects: Vec<ProjectCount> }
#[derive(Serialize)]
struct ProjectCount { project: String, count: usize }
```
(Or reuse `ProjectTaxon` from palace.rs if its shape fits.) **Cap: memory:read.**

## Group D — RepoDirectory (Dominic dependency; global, NOT namespace-scoped)

Driven by the Dominic/Tsume use case (CWD → project resolution for dispatch
context), not the pi tools. Tracked cross-project in
[`../Dominic/docs/handoff/ijima-backend-dependencies.md`](../../Dominic/docs/handoff/ijima-backend-dependencies.md).

The value type exists (`ijima-core/src/repo.rs`: `Repository { name, path,
remote, role }` + `normalize_path`), but there is **no Store persistence
and no HTTP route** — the same gap pattern as the palace/diary routes.
Unlike those, the registry is **global** (not namespace-scoped — per the
`repo.rs` doc comment), so the Store methods take no `ns` param.

Add to the `Store` trait (ijima-core) + `SurrealStore` impl (backend_surreal.rs):
```rust
async fn register_repo(&self, repo: Repository) -> Result<()>;
async fn resolve_repo(&self, cwd: &str) -> Result<Option<Repository>>;  // normalize_path first
async fn list_repos(&self) -> Result<Vec<Repository>>;
```
Routes (new, memory:read for queries; memory:write or a new admin cap for
register — pick consistent with the existing policy):
- `POST /repos` (body: `Repository`) → register/upsert
- `GET /repos/resolve?cwd=<path>` → `Repository` or 404
- `GET /repos` → `Vec<Repository>` (the canonical Anima member list)

`Repository` already derives Serialize; check it derives Deserialize (needs
it for the POST body + a `DEFINE INDEX` on `path` for the resolve lookup).

---

## TDD — one test per route (follow the existing api.rs test pattern)

`api.rs` has an inline test module (e.g. `status_requires_admin_and_reports_counts` at
~line 1609) using `app_with_store()` + `bearer(&auth, "user", MEMORY_READ)`. Add one
test per new route: seed a memory/triple, call the route with the right capability,
assert the response shape; also assert the wrong capability → 403.

## Verify (daemon is up; tokens at /tmp)
```bash
cd ~/working/industrial-algebra/Ijima
cargo build --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining" --bin ijima  # ~if stale
# daemon already running on :7373; restart after rebuild to pick up new routes
IJIMA_LOG=ijima=info ./target/debug/ijima serve &
# curl each new route:
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" http://127.0.0.1:7373/rooms
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" http://127.0.0.1:7373/taxonomy
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" http://127.0.0.1:7373/palace/graph
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" "http://127.0.0.1:7373/palace/tunnel?topic=efficiency&project_a=ijima&project_b=possum"
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" "http://127.0.0.1:7373/memories?project=ijima&limit=5"
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" http://127.0.0.1:7373/memories/stats
curl -s -X POST -H "Authorization: Bearer $(cat /tmp/ijima-write.token)" -H "Content-Type: application/json" \
  -d '{"agent":"test","content":"diary entry","timestamp":"2026-07-13T12:00:00Z"}' http://127.0.0.1:7373/diaries
curl -s -H "Authorization: Bearer $(cat /tmp/ijima-read.token)" http://127.0.0.1:7373/diaries/test
```

## Done criteria
- `cargo fmt --check` clean; `cargo clippy --all-features --all-targets -- -D warnings` clean; `cargo test --all-features` green (1 test per new route).
- All 8 routes respond correctly via curl against the live daemon.
- Commit to `feature/pi-integration`; push.

## Follow-on (NOT this handoff)
Once these 8 routes exist, the pi integration ports (wasm `parse_*` + TS tool
registration) for these 8 tools are a **separate mechanical handoff** — exact same
pattern as the 8 tools just completed (`docs/handoff/pi-integration-remaining-tools.md`).
That handoff can be written once these routes land. After THAT, only lifecycle hooks
(Phase 3) + cutover remain.
