# Laniakea runbook — Ijima central instance

Target host: **laniakea** (Ryzen 7 NAS, 64 GB RAM, 1 TB NVMe, 20 TB ZFS
pool, tailnet-visible). This instance is the central memory brain for
Industrial Algebra, Kellas Cat Games, Shiroyama Electric Music Company,
and personal projects/writings.

## 1. Layout

| Path | Medium | Holds |
|---|---|---|
| `/var/lib/ijima/` | NVMe | `ijima.db` (SurrealKv store), `issuer.key` |
| `/var/lib/ijima/hf-cache/` | NVMe | candle MiniLM model (~90 MB) |
| `/etc/ijima/ijima.toml` | NVMe | daemon config (see `deploy/ijima.toml.example`) |
| `zpool/backups/ijima` | ZFS pool | snapshot + export backups |

SurrealKv does random IO; keep it on NVMe. The 20 TB pool is for backups
and future satellites' checkpoint archives, not the live store.

## 2. Install

```bash
# user + dirs
sudo useradd --system --home /var/lib/ijima --shell /usr/sbin/nologin ijima
sudo mkdir -p /var/lib/ijima /etc/ijima
sudo chown -R ijima:ijima /var/lib/ijima

# binary (build on laniakea — Rust toolchain via rustup)
git clone https://github.com/Industrial-Algebra/Ijima
cd Ijima && cargo build --release --bin ijima \
  --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining"
sudo cp target/release/ijima /usr/local/bin/

# config + unit
sudo cp deploy/ijima.toml.example /etc/ijima/ijima.toml  # then edit
sudo cp deploy/ijima.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now ijima
```

The first `systemctl start` creates `/var/lib/ijima/issuer.key` — **back
it up immediately** (§5).

## 3. TLS / tailnet exposure

Two supported shapes; do **not** bind the daemon to `0.0.0.0`.

1. **`tailscale serve`** (recommended): daemon stays on `127.0.0.1:7373`,
   tailnet TLS terminates in front:
   `tailscale serve --bg --https=ijima https://127.0.0.1:7373` (adjust to
   your tailnet DNS name). Grants are then presented over tailnet TLS.
2. **Native TLS**: Ijima's `tls` feature reads `IJIMA_TLS_CERT` /
   `IJIMA_TLS_KEY` (MagicDNS cert files work); bind to the tailscale
   interface IP from `ijima.toml`.

## 4. Principals + grants (thin clients)

Every workstation runs pi with `IJIMA_URL` pointed here and a single
multi-capability grant:

```bash
sudo -u ijima /usr/local/bin/ijima token issue \
  --principal elliott \
  --capabilities memory:read,memory:write,knowledge:read,knowledge:write
# → one bearer; export as IJIMA_TOKEN on the workstation
```

Service feeds (Minoru, Quantizon) get narrower grants:
`session:ingest` (+ `memory:write` if they push curated memories
directly). `admin` grants are minted rarely, stored in the operator's
password manager, and never exported into shell envs that log.

Revocation (kill-switch): `ijima token revoke` lands with the 0.2.0
revocation PR — until then, rotating the issuer key (replace the seed
file + restart + re-mint all grants) is the emergency lever.

## 5. Backups

1. **ZFS snapshots** (the pool's job):
   `zfs snapshot zpool/backups/ijima@$(date +%F)` — cron nightly; keep
   `zfs-auto-snapshot` defaults if installed. Snapshots are point-in-time
   of the *export* tree below, not the live NVMe store.
2. **Cold exports** (portable, restore anywhere):
   ```bash
   sudo -u ijima /usr/local/bin/ijima export --out /zpool/backups/ijima/ijima-$(date +%F).json
   ```
   Weekly, plus before any upgrade. The export is a full JSON dump.
3. **Issuer key**: copy `/var/lib/ijima/issuer.key` to the offline
   password vault whenever it changes (i.e. once, at install).

Restore drill: `ijima.db` from snapshot (or replay an export via the
import path), `issuer.key` from the vault, `systemctl start ijima`, then
`GET /status` (admin) to confirm counts.

## 6. Scheduled mining (WS5, pending)

Once `ijima mine --pending` lands: a systemd timer runs the nightly pass
over un-mined sessions; triage the review queue weekly (or Dominic does,
via `mining:review`).

## 7. Upgrades

```bash
cd Ijima && git fetch --tags && git checkout v0.2.0
cargo build --release --bin ijima --features "http,server-auth,backend-surreal,embeddings-candle,cli,mining"
sudo systemctl stop ijima
sudo cp target/release/ijima /usr/local/bin/
sudo systemctl start ijima
curl -s -H "Authorization: Bearer $ADMIN" http://127.0.0.1:7373/status | jq
```

`/status` now reports `version`, `started_at_unix`, `uptime_secs` —
verify the version matches the tag after every upgrade.

## 8. Operations quick reference

| Task | Command |
|---|---|
| Health | `curl -s http://127.0.0.1:7373/health` |
| Status (admin) | `curl -s -H "Authorization: Bearer $ADMIN" .../status \| jq` |
| Logs | `journalctl -u ijima -f` |
| Mint grant | §4 (`ijima token issue --capabilities ...`) |
| Rate-limit tuning | `/etc/ijima/ijima.toml` `rate_base` / `rate_multiplier` + restart |
| Store size | `du -sh /var/lib/ijima` |
