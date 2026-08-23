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

## The bundled skill

The package ships a `ijima` skill (auto-registered by pi alongside the
tools): the namespace mental model, why "empty" results are usually
scoping rather than missing data, a diagnostics ladder, and
memory-saving conventions. If an agent reports the brain looks empty,
point it at that skill before it speculates about server state.

## License

Apache-2.0 — same as the Ijima workspace.
