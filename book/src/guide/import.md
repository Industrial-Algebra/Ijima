# Importing Legacy Corpora

`ijima import` streams an external SQLite corpus into a running daemon
over HTTP — the files never leave the workstation, the daemon does the
storing. This is the 0.2.0 path for consolidating pi-mempalace (and
ZeroClaw) history into the central instance.

## Usage

```bash
ijima import mempalace --db ~/.pi/agent/mempalace/memories.db \
    --source "elliotthall-laptop"

ijima import zeroclaw --db ~/zeroclaw/brain.db --source "zeroclaw-archive"
```

| Flag | Meaning |
|---|---|
| `--db PATH` | Source SQLite database |
| `--source NAME` | Provenance origin stamp + namespace derivation |
| `--namespace NS` | Override the target namespace |
| `--url URL` | Daemon (default `$IJIMA_URL` or `127.0.0.1:7373`) |
| `--token TOKEN` | A `memory:write` grant (default `$IJIMA_TOKEN`) |

## What the importer does

1. **Reads** the source rows (memories; pi-mempalace knowledge-graph rows
   arrive as triples).
2. **Retags provenance**: `origin = <source>`, trust tier **dropped to
   `AutoCapture`** regardless of original classification. Harness
   provenance is preserved (`Pi` for mempalace, `Other` for ZeroClaw).
3. **Routes** into `ns_import_<sanitized-source>` (e.g. `Laptop 01` →
   `ns_import_laptop_01`) — never the global commons. `--namespace`
   overrides for deliberate shared targets.
4. **Dedups**: every memory is pre-checked via `POST /memories/check`
   (content-hash) before storing. The same memory saved on two
   workstations stores once per source namespace; re-running an import
   is idempotent.
5. **Reports** per source:

```json
{ "attempted": 1284, "added": 1190, "deduped": 91, "skipped": 3 }
```

`skipped` counts per-row failures — one bad row never aborts the run.

## After import

Import namespaces are staging. Review the content and promote what you
trust into personal or shared namespaces (`trust:promote`); the origin
stamp (`elliotthall-laptop`) travels with every promoted memory, so the
workstation trail survives.

## The older `migrate` path

`ijima migrate --palace <db>` is the pre-HTTP one-shot local import (runs
against the daemon's own data directory, no HTTP). Prefer `import` — it
works against remote daemons, dedups per-source, and tags provenance
properly.
