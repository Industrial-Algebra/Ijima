# CLI Reference

The `ijima` binary (build with the `cli` feature) is the operator's
surface: daemon control, token lifecycle, imports, migration.

## `ijima serve`

Runs the HTTP daemon. Flags override env/config: `--host`, `--port`,
`--data-dir`, `--issuer-key`. See [Running the Daemon](../guide/daemon.md).

## `ijima token`

```bash
ijima token issue --principal NAME (--capability CAP | --capabilities A,B,C) [--json]
ijima token revoke --token "<bearer>" --url URL --auth "<admin>" [--reason "why"]
ijima token revocations --auth "<admin>" [--url URL]
```

- `issue` runs **offline** — it signs with the issuer key in the local
  data directory, so run it where the daemon's key lives (or point
  `IJIMA_DIR`/`IJIMA_KEY` at it).
- `revoke`/`revocations` are **remote** calls to a running daemon's
  admin routes (`--url` defaults to `$IJIMA_URL`).

See [Token Management](../guide/tokens.md).

## `ijima import`

```bash
ijima import (mempalace|zeroclaw) --db PATH --source NAME
             [--namespace NS] [--url URL] [--token TOKEN]
```

Streams a legacy SQLite corpus into a running daemon with provenance
retagging, per-source namespaces, and dedup pre-checks. Defaults from
`$IJIMA_URL` / `$IJIMA_TOKEN`. See
[Importing Legacy Corpora](../guide/import.md).

## `ijima migrate`

```bash
ijima migrate [--palace PATH] [--brain PATH] [--embed] [--namespace NS]
```

The older one-shot local import (writes into the daemon's own data
directory, no HTTP). Prefer `import`.

## `ijima namespace`

```bash
ijima namespace grant <NS> <PRINCIPAL> --url URL --auth "<admin>"
ijima namespace revoke <NS> <PRINCIPAL> --url URL --auth "<admin>"
ijima namespace members <NS> --url URL --auth "<admin>"
```

Shared-namespace membership management (WS3 org walls) on a running
daemon — admin capability required. Grants are upserts (idempotent),
revokes are idempotent, members list oldest-grant-first with the
audit trail (`granted_by`, `granted_at_unix`).

## `ijima doctrine ingest`

Flat seed-pack mode (frontmatter + body files):

```bash
ijima doctrine ingest --dir doctrine/ --url http://127.0.0.1:7373 --token <admin>
```

Tree mode (v0.3.0) — walk a markdown corpus into a wall:

```bash
ijima doctrine ingest --root ../IA-documents \
  --include 'RESEARCH_REPORTS/*' --exclude 'ARXIV*' \
  --namespace ns_ia_doctrine --url ... --token <admin-or-doctrine:write> \
  --dry-run
```

Stable `doct_<hash12(relpath)>` ids make re-runs idempotent (edits
upsert, additions append); id-carrying frontmatter passes through
verbatim; byte-identical files collapse to one row. `--dry-run`
prints the plan without a daemon (`--url`/`--token` optional there).

## `ijima kg-delete`

Hard-deletes one triple (the misplaced-data cleanup tool — soft
retirement is `invalidate`):

```bash
ijima kg-delete --id 'X:relates_to:Y' --namespace ns_wall --url ... --token <admin>
```

Admin-only; `--namespace` is required (destructive ops never default
silently); private walls are valid targets.

## `ijima export`

```bash
ijima export --url http://127.0.0.1:7373 --token <admin> [--namespace ns] [--out file]
```

Logical JSONL export through the daemon (v0.3.0): one
`{"namespace": …, "memory": {…}}` object per line, row count in the
`x-export-count` header, stdout by default. Works against a live
daemon — the old direct-to-store SQL dump (which lost the store LOCK
race) is gone. See the deploy guide for the backup tiering and the
re-ingest restore loop.

## Exit codes and errors

Errors print `ijima: <message>` on stderr and exit non-zero. Remote
commands surface HTTP detail verbatim — a `403` from the daemon names
the missing capability.
