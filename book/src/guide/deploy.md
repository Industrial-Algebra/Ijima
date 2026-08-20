# Deploying on a Server

The reference deployment is a single central instance on a trusted
server, with every workstation as a thin client. This chapter condenses
the full runbook (`docs/deploy/laniakea.md` in the repository) to the
essentials.

## Topology

```
workstations (pi, agents)          laniakea (or any always-on host)
┌────────────────────┐   HTTP    ┌──────────────────────────┐
│ IJIMA_URL=...      │──────────▶│ ijima serve (systemd)    │
│ IJIMA_TOKEN=...    │  tailnet  │ /var/lib/ijima (NVMe)    │
└────────────────────┘           │ zpool/backups (nightly)  │
                                 └──────────────────────────┘
```

- **Network**: Tailscale; the daemon binds the tailnet interface. TLS via
  `tailscale serve` (terminates on the tailnet's certs) or the `tls`
  feature directly.
- **Storage**: data directory on fast local disk (SurrealDB/surrealkv);
  nightly snapshots to bulk storage.

## Provision checklist

1. Install the binary (see [Getting Started](../getting-started.md)) with
   `cli,backend-sqlite,embeddings-candle,mining,tls` as needed.
2. `/etc/ijima/ijima.toml` — host/port/data_dir/issuer_key (copy from
   `deploy/ijima.toml.example`).
3. `deploy/ijima.service` → systemd; `systemctl enable --now ijima`.
4. Mint grants per principal (operators, harnesses, machine feeds) —
   [Token Management](./tokens.md) — via the principals overlay policy.
5. Grant shared-namespace memberships for the org walls:
   `ijima namespace grant ns_ia_shared <principal> --auth <admin>`
   (repeat for `ns_kellas_shared`, `ns_shiroyama_shared`,
   `ns_writing_shared` as needed).
6. `curl .../health` liveness; `GET /status` (admin) for counts.
7. Import workstation corpora — [Importing Legacy Corpora](./import.md).
8. Point each workstation's `IJIMA_URL`/`IJIMA_TOKEN` at the instance.

## Backup & restore

The store is a directory. The drill:

```bash
systemctl stop ijima
zfs snapshot zpool/backups/ijima@$(date +%F)   # or rsync the directory
systemctl start ijima
```

Restore = stop, replace the directory, start. Run the drill once before
trusting it.

## Upgrades

```bash
systemctl stop ijima && <install new binary> && systemctl start ijima
```

Schema definitions are idempotent at open. Check the CHANGELOG for
on-disk-layout notes before skipping multiple minors.

## First-day verification

After provisioning: import one real workstation, run a pi session against
the central instance for a day, and confirm `/status` counts grow. The
deployment isn't real until a harness has lived on it.
