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

## `ijima export`

```bash
ijima export --out dump.sql
```

Dumps the store as SurrealDB SQL (backup/migration aid).

## Exit codes and errors

Errors print `ijima: <message>` on stderr and exit non-zero. Remote
commands surface HTTP detail verbatim — a `403` from the daemon names
the missing capability.
