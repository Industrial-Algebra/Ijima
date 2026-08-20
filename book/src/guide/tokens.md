# Token Management

Grants are credentials. This chapter is the operator's lifecycle guide.

## Issuing

```bash
# a full personal grant
ijima token issue --principal elliott \
    --capabilities memory:read,memory:write,knowledge:read,knowledge:write

# a machine feed grant (session ingest + mining only)
ijima token issue --principal minoru \
    --capabilities session:ingest,mining:review --json

# an operator/admin grant (rare; the point class)
ijima token issue --principal ops --capability admin --json
```

`--json` emits `{token, principal, capabilities, public_key}` for
scripting. `--capabilities` takes a CSV (multi-capability GrantToken);
`--capability` takes a single value. The grant is signed by the issuer
key in the data directory (`IJIMA_KEY` / `issuer_key` config to relocate)
— **tokens minted against one key do not verify on a daemon with another
key.**

Issue *narrow* grants: a dispatcher that only ingests sessions gets
`session:ingest` and nothing else. The geometry enforces what the grant
says, not what the principal "should" have.

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

- Prefer `revoke` + fresh `issue` over long-lived shared grants.
- Issuer-key rotation (replace the key file, restart) invalidates *all*
  grants at once — the emergency lever for key compromise, not routine
  rotation.
- GrantToken expiry is upstream-gated on Schubert 0.5 (`expires_at` +
  `nonce` in the signed blob); until then, revocation is the routine
  deprovisioning path.
