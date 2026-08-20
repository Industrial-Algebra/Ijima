# Running the Daemon

## Foreground

```bash
ijima serve                     # 127.0.0.1:7373, data at ~/.ijima
ijima serve --host 0.0.0.0 --port 7373
IJIMA_DIR=/var/lib/ijima ijima serve
```

## systemd (production)

The repository ships a hardened unit at
[`deploy/ijima.service`](https://github.com/Industrial-Algebra/Ijima/blob/develop/deploy/ijima.service)
plus a commented
[`deploy/ijima.toml.example`](https://github.com/Industrial-Algebra/Ijima/blob/develop/deploy/ijima.toml.example):

```bash
sudo install -m644 deploy/ijima.toml.example /etc/ijima/ijima.toml
sudo install -m644 deploy/ijima.service /etc/systemd/system/
sudo systemctl enable --now ijima
```

The unit runs with a dedicated user, `ProtectSystem=strict`,
`ReadWritePaths=/var/lib/ijima`, and `Restart=on-failure`.

## Observability

- `GET /health` — liveness (no auth).
- `GET /status` — memory/namespace/entity/triple counts, version, start
  time, uptime (admin).
- Structured logs via `tracing` (`IJIMA_LOG` filter; `RUST_LOG` also
  respected by convention).

## TLS

With the `tls` feature, set `IJIMA_TLS_CERT` and `IJIMA_TLS_KEY` (PEM
paths) — the daemon binds HTTPS via axum-server/rustls. On a private
Tailscale network, `tailscale serve` in front of plain HTTP is the
documented alternative.

## Restart semantics

The SurrealDB (surrealkv) engine holds a directory `LOCK` while open.
Dropping the in-process handle does not release it synchronously —
background engine tasks must wind down. This is invisible across process
boundaries (the OS releases the lock at exit), which is the normal
restart path for the daemon. Only embedders opening/closing the same
data directory sequentially inside one process need the spawn-and-yield
pattern (documented on `SurrealStore::open_persistent`).

## Upgrades

Stop the daemon, replace the binary, start. The store's schema is defined
idempotently at open (`DEFINE TABLE IF NOT EXISTS` / `DEFINE INDEX IF
NOT EXISTS`), so minor upgrades with an unchanged on-disk layout need no
migration step. Pre-0.2 development databases (pre-namespaced record
keys) should be re-imported rather than carried forward — see
[Importing Legacy Corpora](./import.md).
