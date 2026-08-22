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

## License

Apache-2.0 — same as the Ijima workspace.
