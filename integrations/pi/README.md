# @industrialalgebra/ijima-pi

[Ijima](https://ijima.industrialalgebra.com) memory tools for
[pi](https://www.npmjs.com/package/@earendil-works/pi-coding-agent) — the
Anima ecosystem's centralized agent memory service.

Registers nine tools backed by a wasm-mapped client core:

- `memory_search`, `memory_save`, `memory_delete`, `memory_check_duplicate`
- `knowledge_add`, `knowledge_query`, `knowledge_status`,
  `knowledge_invalidate`, `knowledge_timeline`

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
  work.

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

## License

Apache-2.0 — same as the Ijima workspace.
