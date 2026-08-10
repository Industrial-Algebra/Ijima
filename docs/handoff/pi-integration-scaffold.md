# Handoff: pi integration — Phase 1 scaffold + `memory_search` proof

**Status:** ready to execute. **Branch:** `feature/pi-integration`.
**Full context:** [`docs/plans/2026-07-12-pi-integration.md`](../plans/2026-07-12-pi-integration.md) — read §2 (architecture), §3 (tool map), §5 (auth), §7 (packaging), §9 (phases) before starting.
**Phase 0 is DONE** (wasm spike → path b; `migrate --namespace`; `scope=visible` scored search). 131 tests green on `develop`.

> **Efficiency note:** This unit is ~90% mechanical (crate scaffolding, wiring,
> one tool port). Run it on **DeepSeek-v4-pro**. Escalate to GLM-5.2 only if the
> wasm-bindgen/serde bridge or the ExtensionAPI shape throws a real design
> question. One work unit per context — don't carry this into phases 3–5.

---

## Goal of this handoff
Stand up the integration scaffold (Rust wasm core + TS shim) and prove it
end-to-end with **one** tool (`memory_search`). Ship + verify before fanning
out to the other 16 tools (those are later sessions).

## Toolchain (already installed — verify, don't install)
- `wasm-pack`, `wasm-bindgen-cli` (in `~/.cargo/bin`)
- Node 24 + npm 11
- `rustup target list --installed` includes `wasm32-unknown-unknown`
- `ijima-core` compiles to `wasm32-unknown-unknown` clean (~3s, confirmed by spike)

## Step 1 — Rust wasm core crate `ijima-pi`
1. Create `ijima-pi/` as a workspace member (add to root `Cargo.toml`
   `[workspace] members` alongside `ijima-core`/`ijima-server`/…).
2. `ijima-pi/Cargo.toml`:
   - `crate-type = ["cdylib", "rlib"]`
   - deps: `ijima-core = { path = "../ijima-core", features = ["serde"] }`,
     `serde = { workspace = true, features = ["derive"] }`, `serde_json`,
     `wasm-bindgen`, and likely `serde-wasm-bindgen` (see watch-item 1).
   - `#![deny(unsafe_code)]` + Apache-2.0 license header (IA standard).
3. `ijima-pi/src/lib.rs` — **pure mapping only, no HTTP, no tokio** (path b).
   Export via `#[wasm_bindgen]`:
   - `build_search_request(text: String, limit: Option<usize>, scope: Option<String>) -> JsValue`
     → JSON body for `POST {IJIMA_URL}/memories/search?scope=visible`:
     `{ "text", "limit", "scope": "visible" }`. (Hardcode `scope=visible` for
     the pi path — §3.5/§6; this is what restores pi-mempalace global search.)
   - `parse_search_response(json: JsValue) -> JsValue`
     → maps Ijima `{ memories: [{ memory: {…}, similarity }] }` to the pi
     result shape (`{ text, project, topic, timestamp, score }[]`).
4. **Build:** `wasm-pack build ijima-pi --target nodejs --out-dir ../integrations/pi/pkg`
   (verify it emits `ijima_pi.js` + `ijima_pi_bg.wasm`).

## Step 2 — TS shim `integrations/pi/`
1. `package.json` (ESM), `tsconfig.json`.
2. `index.ts` — the ExtensionAPI extension. **Read pi's
   `docs/extensions.md` first** for the exact tool-registration API. Register
   ONE tool, `memory_search`:
   - read `IJIMA_URL` (default `http://127.0.0.1:7373`) + `IJIMA_TOKEN_MEMORY_READ`
   - import the wasm pkg; call `build_search_request(...)`
   - `fetch(POST)` with `Authorization: Bearer <token>`
   - `parse_search_response(...)`; format results as text (incl. `% match` from
     `similarity`); return.
   - **Graceful offline:** on fetch failure, return "memory unavailable", don't
     throw (§10).
3. Lifecycle hooks (`turn_end`, `before_agent_start`) are **out of scope** here
   (Phase 3). Leave stubs only.

## Step 3 — Verify
- `cargo build -p ijima-pi --target wasm32-unknown-unknown` → clean.
- `cargo clippy --all-features --all-targets -- -D warnings` → clean (IA gate).
- `cargo test --all-features` → still 131+ (add unit tests for the two mapping fns).
- `wasm-pack build` succeeds; a Node one-liner loads the pkg and
  `build_search_request("x", 5, "visible")` returns valid JSON.
- **Full e2e** (needs `ijima serve` + corpus): from a pi session, call
  `memory_search`; confirm it returns merged private + global results (§3.5).

## Watch-items (escalate to GLM if these bite)
1. **wasm-bindgen ↔ serde bridge.** `serde-wasm-bindgen` is the usual clean
   path (`to_value`/`from_value`); confirm it round-trips `ijima-core` types
   with the `serde` feature. If a type resists, map through `serde_json` to
   `JsValue` manually rather than fighting the derive.
2. **ExtensionAPI shape.** Read `docs/extensions.md` for the precise
   tool-registration + return contract; don't assume from memory.
3. **Bun vs Node.** pi runs on both; Bun's wasm load lags Node — test both.
4. **Token env-bundle (§5).** Search needs `IJIMA_TOKEN_MEMORY_READ` only for
   this slice. The bundle (`_MEMORY_WRITE`, `…_KNOWLEDGE_*`, …) gets wired as
   later tools land.

## Out of scope for this handoff (do in later fresh contexts)
- Phases 3–5: the other 16 tools, lifecycle hooks (auto-capture/wake-up),
  KG/palace/diary tools, `/memory` command, stats widget.
- Cutover (`ijima serve` + `migrate --namespace ns_<principal>_private` +
  swap `npm:pi-mempalace` → load-by-path) — its own session after the tools exist.
