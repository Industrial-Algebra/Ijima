# Phase 3 handoff — DeepSeek v4 Pro execution spec

**Audience:** a DeepSeek v4 Pro coding session executing the remaining
mechanical Phase 3 items autonomously. This doc is intentionally dense and
leaves no design decisions open. If you hit something this doc does not
specify, **stop and flag it** rather than guessing.

**Reserved for GLM-5.2 (do NOT start):** `ijima-miner` (rules + Proserpina
llm tier + review-queue/promote pipeline) and **3.5 multi-party handling**.
Those are the high-difficulty items.

---

## 0. Repo state (read first)

- **Branch off `develop`** (currently `b9c5705`, "session-context repository
  completion (Phase 2.3)").
- **Open PRs NOT on develop yet** — your branches will not have these; do not
  reference their routes/features:
  - #8 `feature/palace-organization` → adds `/rooms`, `/taxonomy`,
    `/palace/graph`, `/palace/tunnel` + Store methods `list_rooms`,
    `taxonomy`, `palace_graph`, `traverse_tunnel`.
  - #9 `feature/hardening` → adds the `rate-limit` feature, `tracing`.
- **develop's `Store` trait methods** (the ones you WILL see): `store_memory`,
  `recall_memory`, `delete_memory`, `list_memories`, `store_stats`,
  `check_duplicate`, `search_memories`, `ingest_turn`, `session_turns`,
  `create_session`, `list_sessions`, `end_session`.
- **develop's `ApiError` variants:** `Forbidden`, `NotFound`, `BadRequest(String)`,
  `Conflict(String)`, `Internal(String)`.

## 1. Non-negotiables (IA coding standards)

- **TDD**: write the test first, watch it fail, then implement. Tests live in
  the same file under `#[cfg(test)] mod tests`.
- **`#![forbid(unsafe_code)]`** at every crate root (already present; keep it).
- **CI gate (must pass before PR):**
  ```
  cargo fmt --all
  cargo clippy --all-features --all-targets -- -D warnings
  cargo test --all-features
  cargo doc --all-features --no-deps
  ```
- **License:** Apache-2.0 header on every new file (copy from an existing
  `ijima-core/src/*.rs`).
- **Gitflow:** branch `feature/<name>` off `develop`; one PR per item via
  `gh pr create --base develop`; **never auto-merge** — wait for human review.

## 2. Daemon build (for verifying server work)

```
cargo build --features http,server-auth,backend-surreal,cli --bin ijima
```
(+ `embeddings-candle` for the full candle-backed daemon; + `rate-limit` once
#9 merges — irrelevant to these items.)

## 3. The established 5-step pattern (follow it exactly)

Every read/write feature in this repo is shaped like this. Match it:

1. **`ijima-core`**: new module (e.g. `src/diary.rs`) with the domain types
   (`#[cfg_attr(feature="serde", derive(serde::Serialize, serde::Deserialize))]`),
   add `pub mod diary;` + `pub use diary::...;` in `lib.rs`, add the trait
   method(s) to `Store` in `src/store.rs` (import the new type there).
2. **`ijima-server/src/backend_surreal.rs`**: a `*_TABLE` const, a private
   `*Record` (`Serialize`+`Deserialize`), impl the trait method on
   `SurrealStore`. Re-export new core types in the `use ijima_core::{...}` block.
3. **`ijima-server/src/api.rs`**: add `.route(...)` in `app()` (after the
   existing routes, before `.layer(Extension(auth))`), add a `*Query`
   `Deserialize` struct + handler, add the route to the doc table at the file
   top, add the new core type to the `use ijima_core::{...}` block.
4. **`ijima-client/src/lib.rs`**: add `#[cfg(feature="remote")] pub async fn`
   method(s); add the type to the `use ijima_core::{...}` block.
5. **Tests**: one backend test + one HTTP test (use the existing
   `app_with_store()` + `bearer()` helpers in `api.rs` tests).

### Auth pattern (copy verbatim from existing handlers)

- **Writes** (POST that mutates): `let ns = principal.0.personal_namespace();`
  (personal namespace, like `store_memory`/`create_session`).
- **Reads** (GET): `let ns = resolve_ns(&principal, q.namespace.as_deref())?;`
  (allows `?namespace=` override; other principals' `*_private` → 403).
- Capability guard: `if !principal.0.may(CAP) { return Err(ApiError::Forbidden); }`
  where `CAP` is `MEMORY_READ`/`MEMORY_WRITE`/etc from
  `ijima_core::capabilities`.

### SurrealDB 2.6 gotchas (these WILL bite you)

- `Surreal::new::<Mem>(())` returns `Surreal<Db>`, **not** `Surreal<Mem>`.
- `.update()` does **not** create missing records — use `.create()` or
  `.upsert()`.
- **ORDER BY expressions must appear in the SELECT list.**
- **Do not use SurrealDB aggregate functions** (`SELECT count()` shape is
  ambiguous). SELECT the raw rows, aggregate in Rust (see `store_stats`).
- `RELATE` rejects bind-parametrized endpoints (not relevant here).
- Constructors: `SurrealStore::open_embedded()` (Mem, tests),
  `open_persistent(path)` (disk, daemon).

---

## Item A — Diaries (Phase 3.3)

Per-agent append-only journals. Niche, pi-specific. ~1 PR.

### A.1 Core (`ijima-core/src/diary.rs`, new file)

```rust
// (Apache-2.0 header)

//! Per-agent diary journals (Phase 3.3).

/// One diary entry: an agent's chronological reflection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiaryEntry {
    /// The agent this diary belongs to (e.g. "claude", "pi").
    pub agent: String,
    /// The entry body.
    pub content: String,
    /// Optional topic tag.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
    pub topic: Option<String>,
    /// ISO-8601 timestamp (display; ordering uses an internal numeric field).
    pub timestamp: String,
}
```
Add `pub mod diary;` and `pub use diary::DiaryEntry;` in `lib.rs`.

**Decisions (do not deviate):**
- **Append-only, id-less, no delete endpoint** (matches pi-mempalace; journals
  are immutable logs).
- **Reuse `MEMORY_WRITE` / `MEMORY_READ`** capabilities. Do **NOT** add a new
  capability — that requires re-running Schubert's recommender and editing
  `policy/policy.toml`, out of scope.

### A.2 Store trait (`ijima-core/src/store.rs`)

Add a new section after the session-context section:
```rust
    // ===== Diaries (Phase 3.3) =====

    /// Appends a diary entry under `ns`.
    async fn write_diary(&self, ns: &NamespaceId, entry: DiaryEntry) -> Result<()>;

    /// Returns the last `limit` entries of `agent`'s diary under `ns`, in
    /// chronological order.
    async fn read_diary(
        &self,
        ns: &NamespaceId,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<DiaryEntry>>;
```
Import `DiaryEntry` in the `use crate::{...}` block at the top of `store.rs`.

### A.3 SurrealStore (`ijima-server/src/backend_surreal.rs`)

- `const DIARY_TABLE: &str = "diaries";` near the other table consts.
- Record (mirrors `SessionTurnRecord`'s shape):
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  struct DiaryRecord {
      agent: String,
      content: String,
      #[serde(default, skip_serializing_if = "Option::is_none")]
      topic: Option<String>,
      timestamp: String,
      /// Internal epoch-millis for correct ORDER BY (string timestamps don't
      /// sort lexicographically across formats). Stamped on write.
      ts: i64,
      namespace: String,
  }
  ```
- `write_diary`: stamp `ts` from `SystemTime::now().duration_since(UNIX_EPOCH)`
  as millis; `db.create(DIARY_TABLE).content(record).await` (append, like
  `ingest_turn`).
- `read_diary`: `SELECT agent, content, topic, timestamp, namespace FROM
  diaries WHERE namespace = $ns AND agent = $agent ORDER BY ts DESC LIMIT $lim`,
  then `.reverse()` for chronological (exactly like `session_turns`).
- Add `DiaryEntry` to the `use ijima_core::{...}` block.

### A.4 HTTP (`ijima-server/src/api.rs`)

- Routes in `app()`:
  ```rust
  .route("/diaries", post(write_diary))
  .route("/diaries/:agent", get(read_diary))
  ```
- `POST /diaries` — `MEMORY_WRITE`, `ns = personal_namespace()`. Body: the
  `DiaryEntry`. If `timestamp` empty, stamp epoch-secs string (see how
  `create_session` stamps `started_at`). Returns `StatusCode::NO_CONTENT`.
- `GET /diaries/:agent?namespace=&limit=` — `MEMORY_READ`,
  `ns = resolve_ns(...)`, `limit.unwrap_or(50).min(500)`. Returns
  `Json<Vec<DiaryEntry>>`.
- Add both rows to the route doc table at the top of `api.rs`.

### A.5 Client (`ijima-client/src/lib.rs`)

- `write_diary(&self, entry: DiaryEntry) -> Result<()>` (POST /diaries).
- `read_diary(&self, agent: &str, namespace: Option<&str>, limit: Option<usize>) -> Result<Vec<DiaryEntry>>`
  (GET /diaries/:agent; reuse `build_path` for the query string).
- Add `DiaryEntry` to the `use ijima_core::{...}` block.

### A.6 Tests

- Backend `write_diary_appends_and_reads_chronologically`: write 3 entries for
  agent "claude" (with small `ts` ordering — note `write_diary` stamps `ts`
  itself, so write them with tiny sleeps OR assert ordering by content index;
  simplest: write 3, read, assert len==3 and content in write order).
- HTTP `diaries_write_then_read_via_http`: POST one entry, GET /diaries/claude,
  assert len==1 and fields round-trip.

**PR title:** `feature: per-agent diaries (Phase 3.3)` — branch
`feature/diaries`.

---

## Item B — Backup/export (Phase 3.4)

MVP = a CLI subcommand that dumps the SurrealDB store to a file. HTTP route is
optional/stretch.

### B.1 SurrealStore export method (`ijima-server/src/backend_surreal.rs`)

Add behind the `backend-surreal` feature:
```rust
/// Exports the entire store as a SurrealDB SQL dump to `path`.
pub async fn export_to(&self, path: impl AsRef<std::path::Path>) -> Result<()> { ... }
```
**The surrealdb 2.6 SDK call must be verified** — inspect
`~/.cargo/registry/src/*/surrealdb-2.6.5/src/api/method/export.rs` and
`mod.rs` (`pub fn export<R>(&self, target: impl IntoExportDestination<R>)`).
The likely shape is `self.db.export(path.as_ref()).await.map_err(store_err)?;`
but confirm the destination trait. If the API streams, write to a file handle.

### B.2 CLI (`ijima-server/src/main.rs`)

Add a `Export` clap subcommand (`ijima export --out <path>` or positional
`<path>`). It opens the **persistent** store (same `IJIMA_DIR`/`ijima.db`
resolution as `server::serve()` — factor that path resolution into a small
shared helper if it isn't already), calls `SurrealStore::export_to(path)`,
prints a `tracing::info!` on success. **No token** — this is a local operator
action on the embedded store (the operator already has filesystem access).
Wire it into the existing `Command` enum and the `main()` match.

### B.3 (Optional/stretch) HTTP `GET /export` — `ADMIN`

Returns the dump as a downloadable body. Only do this if the SDK makes
streaming-to-response clean; otherwise skip and note it in the PR body. Do not
block the PR on it.

**Decision:** MVP is the CLI. Do not over-engineer.

**PR title:** `feature: store backup/export (Phase 3.4)` — branch
`feature/backup`.

---

## Item C — TLS (Phase 3.4)

Env-driven TLS on the daemon. Plain HTTP remains the default.

### C.1 Cargo (`ijima-server/Cargo.toml`)

New feature + deps:
```toml
tls = ["dep:axum-server", "http"]
axum-server = { version = "0.7", features = ["tls-rustls"], optional = true }
```
(Confirm the latest 0.x; `tls-rustls` feature name may be `tls-rustls-no-provider` —
verify against the version you pin and add a crypto provider if required.)

### C.2 Daemon (`ijima-server/src/server.rs`)

In `serve()`, after binding the address: if both `IJIMA_TLS_CERT` and
`IJIMA_TLS_KEY` env vars are set (PEM file paths), load a `rustls` server config
and serve via `axum_server::bind_rustls(addr, config)`; else keep the current
`tokio::net::TcpListener` + `axum::serve` path. Gate the TLS branch with
`#[cfg(feature = "tls")]` and log which mode via `tracing::info!`.

**Decision:** feature-gated, env-driven, plain HTTP default. The candle build is
the long pole — TLS deps add negligible compile time.

**PR title:** `feature: optional TLS for the daemon (Phase 3.4)` — branch
`feature/tls`.

---

## Appendix — backend-sqlite migration (DEFER / escalate)

**Lower priority.** Two judgment calls are pre-decided here; the risk is the
pi-mempalace SQLite **schema** must be inspected first (path unknown — likely
under a `pi-mempalace` checkout, look for the `.db` + its migration/schema
files).

**Pre-decisions:**
1. **Embeddings on import: re-embed** via `CandleEmbedder` (consistent model).
   Do not import pi-mempalace's raw vectors — they're a different model/dim.
2. **Dedup:** `store_memory` already does content-hash dedup; calling it per row
   is correct (dupes return the existing id, which is fine).

**Shape:** a CLI `ijima import-sqlite --path <mempalace.db> --namespace
<ns_id>` that opens the sqlite file (`rusqlite`, already a `backend-sqlite`
dep), reads rows, maps each to `Memory` (source = `MemorySource::Explicit` or a
new `Imported` variant — prefer reusing `Explicit` to avoid touching the enum),
and calls `store_memory`.

**Escalate to GLM if:** the schema inspection is surprising, the transform
exceeds ~400 lines, or pi-mempalace stores data with no clean `Memory`
mapping. **Recommendation:** attempt only after Items A–C land clean; otherwise
leave for the GLM miner session.

---

## Final checklist (per PR)

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --all-features --all-targets -- -D warnings` (zero warnings)
- [ ] `cargo test --all-features` (note the test count in the PR body; develop
      currently passes ~80 tests — yours should add to that)
- [ ] `cargo doc --all-features --no-deps` builds
- [ ] `gh pr create --base develop` with a body listing surface + test count
- [ ] Do **not** merge — human review.
