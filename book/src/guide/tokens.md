# Token Management

Grants are credentials. This chapter is the operator's lifecycle guide.

## Issuing

Issuance is **policy-constrained** (Schubert 0.5 #20.3): `ijima token
issue` signs only what the issuance policy entitles, failing closed on
unknown principals and over-entitled requests. The policy resolves as
`--policy PATH` > `$IJIMA_POLICY` > `$IJIMA_DIR/policy.toml` > the
embedded default — which seeds **no principals**, so a fresh install
mints nothing until the operator provisions a policy file (bootstrap
below).

The operator file can be a minimal **principals-only overlay**:

```toml
# $IJIMA_DIR/policy.toml
[principals.elliott]
grants = ["memory:read", "memory:write", "knowledge:read", "knowledge:write"]

[principals.minoru]
grants = ["session:ingest", "mining:review"]
```

Partitions always derive from the embedded policy — an overlay can
assign capabilities but never redefine the geometry. Adding a
workstation = one file edit; no rebuild, no daemon restart (the daemon
verifies proof-carrying grants, not principals).

```bash
# a full personal grant
ijima token issue --principal elliott \
    --capabilities memory:read,memory:write,knowledge:read,knowledge:write

# a machine feed grant — always expiring
ijima token issue --principal minoru \
    --capabilities session:ingest,mining:review \
    --expires-in 2592000 --json   # 30 days

# an operator/admin grant (rare; the point class)
ijima token issue --principal ops --capability admin --json
```

`--json` emits `{token, principal, capabilities, expires_at_unix?,
public_key}`. `--expires-in <SECONDS>` sets a signed expiry (Schubert
0.5 ADR-0001; boundary inclusive — dead when `now >= expires_at`);
omit for never-expiring. Renewal = re-issue (fresh nonce), never
mutation.

## Deploying to clients

Thin clients need two env vars:

```bash
export IJIMA_URL="http://ijima.tailnet:7373"
export IJIMA_TOKEN="<grant blob>"
```

The pi extension, `ijima-client`, and the CLI's remote subcommands all
honor them.

## Revoking

The kill-switch for leaked or rotated credentials:

```bash
ijima token revoke --token "<bearer>" \
    --url http://127.0.0.1:7373 --auth "<admin-bearer>" \
    --reason "leaked in CI log"
```

- Revocation is **store-backed and survives restarts**: the daemon
  persists a SHA-256 hash of the bearer and hydrates an in-memory set at
  boot. Raw bearer values never touch the store, logs, or backups.
- The check composes with signature verification: a revoked bearer is
  rejected even though its signature is valid.
- Past revocations: `ijima token revocations --auth "<admin-bearer>"`
  (admin), oldest first.

## Rotation practice

- Prefer short expiries + re-issue for machine feeds (`--expires-in`);
  `revoke` for incidents; issuer-key rotation remains the emergency
  lever for key compromise (invalidates every grant at once).
- Schubert 0.5 expiry is adopted; the instance-side revocation list
  stays as defense-in-depth (kills leaked bearers with no issuer
  involvement). CRDT nonce-tombstones arrive with 0.3 satellites.
