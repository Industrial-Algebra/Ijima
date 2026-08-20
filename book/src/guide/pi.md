# pi Thin-Client Integration

pi (the coding-agent harness) talks to Ijima as a thin client — no local
store, no local daemon. The extension is `integrations/pi` (npm), built
from the `ijima-pi` crate compiled to WASM.

## Setup

Two environment variables replace the old four-token pi-mempalace bundle:

```bash
export IJIMA_URL="http://ijima.tailnet:7373"
export IJIMA_TOKEN="<grant blob>"
```

Mint the grant on the daemon:

```bash
ijima token issue --principal pi-workstation \
    --capabilities memory:read,memory:write,knowledge:read,knowledge:write
```

## What the extension provides

The pi tool surface maps onto Ijima routes:

| pi tool | Ijima surface |
|---|---|
| Memory search / save / dedup-check | `/memories`, `/memories/search`, `/memories/check` |
| Knowledge add / query / timeline | `/kg/*` |
| Rooms, taxonomy, palace graph | `/rooms`, `/taxonomy`, `/palace/graph` |
| Diary write/read | `/diaries` |

Requests are built and parsed in WASM (the `ijima-pi` crate's
request/response types), so the wire contract is compiled once and shared
— the pi process itself does no JSON hand-rolling.

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

then point pi at Ijima with the env vars above and retire the local
store.
