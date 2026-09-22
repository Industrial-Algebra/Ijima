# @industrialalgebra/ijima-pi

[Ijima](https://ijima.industrialalgebra.com) memory tools for
[pi](https://www.npmjs.com/package/@earendil-works/pi-coding-agent) — the
Anima ecosystem's centralized agent memory service.

Registers nine tools backed by a wasm-mapped client core:

- `memory_search`, `memory_save`, `memory_delete`, `memory_check_duplicate`
- `knowledge_add`, `knowledge_query`, `knowledge_status`,
  `knowledge_invalidate`, `knowledge_timeline`

## Per-project configuration (`.pi/ijima.json`)

A repo can declare which memory wall it belongs to — the declarative form
of org scoping. Put a config at the project root:

```json
{ "namespace": "ns_orthant_shared" }
```

Optional keys `url` and `token_file` pin the daemon and credentials the
same way. Resolution walks up from the session root (nearest file wins),
and precedence is always **environment variable → project file → default**,
so an exported `IJIMA_NAMESPACE` still overrides the declaration. Invalid
JSON is ignored silently — a broken config never breaks a session.

## Setup

In `~/.pi/agent/settings.json`:

```json
{ "packages": ["npm:@industrialalgebra/ijima-pi"] }
```

Point it at your daemon with one multi-capability grant:

```bash
IJIMA_URL=http://your-ijima-host:7373
IJIMA_TOKEN=<grant from: ijima token issue --principal <name> \
  --capabilities memory:read,memory:write,knowledge:read,knowledge:write>
```

Both may also live in your shell environment. The grant needs one
capability per tool family; a missing capability surfaces in the tool
error with the exact `ijima token issue` invocation to fix it.

## Building from source

```bash
npm run build   # wasm-pack (the ijima-pi crate) + tsc shim
```

## Auto-capture and wake-up (the loop-closers)

The extension closes the memory loop without agent diligence:

- **Auto-capture** — after each assistant turn, the exchange is stored
  at the `AutoCapture` trust tier (importance 0.5, length-gated,
  truncated at 2000 chars, silent failure — capture never interrupts a
  session).
- **Wake-up injection** — every system prompt gains an `## Agent Memory
  (ACTIVE)` block: a tool reminder plus your wake-up essentials and
  doctrine (`GET /wakeup`), refreshed per session.
- **Token fallback** — `IJIMA_TOKEN` env, then `~/.config/ijima/token`
  (or `$IJIMA_TOKEN_FILE`), so shells that didn't source the env still
  work. `IJIMA_URL` falls back to `~/.config/ijima/url` the same way.
- **Home namespace** — set `IJIMA_NAMESPACE=ns_<org>_shared` and captures,
  saves, and wake-up operate in that wall instead of the personal
  namespace: shared knowledge stays shared (and other orgs stay walled).
  Requires daemon ≥ 0.2.5 for namespaced wake-up; captures work on 0.2.2+.
- **Content-derived ids** — memories are id'd by content hash
  (`mem_<hash16>`): same content, same id, meaningful in transcripts.

`memory_save` remains the deliberate path: explicit saves land at the
`Explicit` tier with higher importance — auto-capture is the floor,
not the ceiling. An E2E harness (`e2e.mjs`, run against a live daemon)
proves the full cycle: capture → wake-up → injection.

## The bundled skill

The package ships a `ijima` skill (auto-registered by pi alongside the
tools): the namespace mental model, why "empty" results are usually
scoping rather than missing data, a diagnostics ladder, and
memory-saving conventions. If an agent reports the brain looks empty,
point it at that skill before it speculates about server state.

## Production track record (stability report)

First production deployment: August 2026. Reviewed at six weeks of soak
(2026-09-21). One central daemon, a seven-host agent fleet, ~38,700
memories across namespace walls at review time.

| Signal | State at review |
|---|---|
| Uptime | All restarts operator-driven (config switches); zero crashes, zero auto-restarts |
| Error log | 43 error lines total across the whole soak — all from one known incident (a database lock race against a backup drill, cause fixed and doctrine-encoded); **error-free since** |
| Capture volume | ~90–180 new memories/day; growth is entirely agent captures + curated corpus |
| Release cadence | 0.1.0 → 0.3.0 in two months; npm publishes via OIDC trusted publishing in CI |
| Backup discipline | Hourly mirror to a second filesystem, weekly restore drills that boot the mirror, daily census snapshots |

The loop features (`memory_search` priming at wake-up, auto-capture at low
trust, `memory_save` for deliberate persistence) have been running unattended
across the fleet for the entire soak. Three documented organic-learning
events from that period — an agent that learned a publishing calendar it was
never told about, a research radar that identified a transferable
architecture seam, and a cross-repo sprint planned from remembered dives on a
different repository — are written up on the
[company blog](https://industrialalgebra.com/blog).

The full soak log, runbooks, and restore-drill records live in the
[repository](https://github.com/Industrial-Algebra/Ijima).

## License

Apache-2.0 — same as the Ijima workspace.
