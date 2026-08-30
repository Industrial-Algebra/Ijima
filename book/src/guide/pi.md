# pi Thin-Client Integration

pi (the coding-agent harness) talks to Ijima as a thin client — no local
store, no local daemon. The extension is `@industrialalgebra/ijima-pi`
(npm), built from the `ijima-pi` crate compiled to WASM.

## Setup

Three environment variables — only the token is required:

| Variable | Meaning | Fallback file |
|---|---|---|
| `IJIMA_TOKEN` | A multi-capability grant (below) | `~/.config/ijima/token` |
| `IJIMA_URL` | Daemon base URL | `~/.config/ijima/url` |
| `IJIMA_NAMESPACE` | **Home namespace** (see below) | — (personal when unset) |

Mint the grant on the daemon:

```bash
ijima token issue --principal pi-workstation \
    --capabilities memory:read,memory:write,knowledge:read,knowledge:write
```

The file fallbacks mean a host needs **no shell configuration at all** —
drop the token (and optionally the url) file in place and every process
(agent tabs, cron, systemd units, non-login shells) just works.

## Agent homes

By default a principal's memories land in its personal namespace —
private, invisible to siblings. For fleet-institutional knowledge that
is exactly wrong: set the home to your org wall and every member
agent shares one pool:

```bash
export IJIMA_NAMESPACE="ns_ia_shared"   # this org's wall
```

Captures, saves, dedup checks, **and wake-up** all operate in the home
namespace — a memory saved on one host greets the next session on every
other member host. A separate org's machine sets its own wall instead
(`ns_other_org_shared`); membership gating applies as usual, so walls
keep meaning what walls mean. Unset = personal, private by default.
(Namespaced wake-up requires daemon ≥ 0.2.5; captures work on 0.2.2+.)

## The loop-closers

The extension does the remembering so the agent doesn't have to:

- **Auto-capture** — after each assistant turn, the exchange is stored
  at the `AutoCapture` trust tier (importance 0.5, length-gated,
  2000-char truncation, silent failure — capture never interrupts a
  session). Content-hash dedup absorbs repeats.
- **Wake-up injection** — every system prompt gains an `## Agent Memory
  (ACTIVE)` block: a tool reminder, the memory-model cheatsheet
  (namespaces, what "empty" means), and the home's wake-up essentials
  (top-N by importance + recency, refreshed per session).
- **The bundled `ijima` skill** — the deep-dive reference: namespace
  topology, the diagnostics ladder, and "never conclude wrong-data-dir
  from agent-side probes."

`memory_save` remains the deliberate path: explicit saves land at the
`Explicit` tier with higher importance — auto-capture is the floor,
not the ceiling.

## What the extension provides

| pi tool | Ijima surface |
|---|---|
| `memory_search` (scope=visible: personal + global + staging + member walls) | `/memories/search` |
| `memory_save` (content-derived id: `mem_<hash16>`) | `/memories` |
| `memory_check_duplicate`, `memory_delete` | `/memories/check`, `/memories/{id}` |
| `knowledge_add/query/timeline/status/invalidate` | `/kg/*` |

Requests are built and parsed in WASM (the `ijima-pi` crate's
request/response types), so the wire contract is compiled once and
shared — the pi process itself does no JSON hand-rolling.

## Why thin clients

The 0.2.0 topology decision: **all workstations are thin clients of one
central instance** (the "Central Brain" deployment). Local memory state
on workstations means fragmentation again — the thing Ijima exists to
end. Full local instances with checkpoint sync (satellites) are the 0.3
design, not the 0.2 reality.

## Replacing pi-mempalace

If the workstation has an existing pi-mempalace database, import it once:

```bash
ijima import mempalace --db <memories.db> --source "$(hostname)"
```

then point pi at Ijima (env vars or config files), set the home
namespace, and retire the local store. The old database remains as a
cold backup — nothing in the migration is destructive.
