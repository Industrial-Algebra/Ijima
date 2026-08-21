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

1. **Reads** the source rows (memories; pi-mempalace corpora also carry
   `entities` + `triples` knowledge-graph tables — see step 5).
2. **Retags provenance**: `origin = <source>`, trust tier **dropped to
   `AutoCapture`** regardless of original classification. Harness
   provenance is preserved (`Pi` for mempalace, `Other` for ZeroClaw).
3. **Routes** into `ns_import_<sanitized-source>` (e.g. `Laptop 01` →
   `ns_import_laptop_01`) — never the global commons. `--namespace`
   overrides for deliberate shared targets.
4. **Dedups**: every memory is pre-checked via `POST /memories/check`
   (content-hash) before storing. The same memory saved on two
   workstations stores once per source namespace; re-running an import
   is idempotent. If the daemon rate-limits the import (HTTP 429) the
   client backs off exponentially (250 ms doubling, ~16 s budget) and
   retries — rate limiting never drops rows.
5. **Imports the knowledge graph** (pi-mempalace sources only):
   entities are re-addressed from opaque `ent_*` hashes to Ijima's
   id-is-name convention (same-name source entities merge), and each
   triple is added with its confidence + temporal range — a source
   `valid_to` is applied as an invalidation so historical facts stay
   historical. Triples referencing unknown entities are counted as
   `unmapped`, never imported.
6. **Reports** per source:

```json
{
  "memories":  { "attempted": 14459, "added": 14459, "deduped": 0, "skipped": 0 },
  "knowledge": { "attempted": 124, "added": 124, "skipped": 0 },
  "unmapped": 0
}
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
