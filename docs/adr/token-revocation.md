# ADR: Token Revocation (the grant kill-switch)

Date: 2026-08-16
Status: Accepted (WS1b of `docs/plans/2026-08-10-v0.2.0-central-brain.md`)

## Context

Schubert `GrantToken`s are stateless Ed25519 signatures: a bearer is
cryptographically valid for as long as the issuer key exists. On a
single-workstation test daemon that is acceptable. On **laniakea** — a
long-lived, multi-principal, tailnet-exposed central instance — it is
not: a bearer leaked into a CI log, a dotfile, or a backup must be
killable *now*, not at the next issuer-key rotation (which invalidates
every grant and requires re-minting the whole fleet).

## Decision

A store-backed **revocation list** checked at verify time:

1. **Key = SHA-256 of the bearer** (`bearer_hash`: trim, strip an
   optional `Bearer ` prefix, hash). Hashes — never raw bearers — are
   persisted, so store dumps, backups, and the admin listing never
   contain a live credential.
2. **Store surface** (`Store` trait + SurrealStore): `revoke_token`
   (idempotent upsert keyed by hash) + `list_revocations` (oldest
   first). Global table `token_revocations` — revocations are
   instance-wide, not namespace-scoped.
3. **Live check in `IjimaAuth`**: an in-memory `HashSet<String>` behind
   a `Mutex` (sub-microsecond critical section). `verify_bearer`
   rejects a revoked bearer *after* the crypto check — a revoked token
   is exactly as dead as a bad signature. Hydrated from the store at
   daemon boot; appended by the admin route.
4. **Routes**: `POST /tokens/revoke` (admin; body `{token, reason?}` —
   accepts raw or `Bearer ...` form) persists first, then arms the
   in-memory set. `GET /tokens/revocations` (admin) lists the ledger.
   Order matters: persist-then-arm means a crash between the two
   re-arms at boot (store is the source of truth).
5. **CLI**: `ijima token revoke --token <bearer> --auth <admin>` /
   `ijima token revocations --auth <admin>` (HTTP against a running
   daemon — the store may be locked by the daemon, so direct store
   writes from a second process are not an option).

## Alternatives considered

- **Expiry carried in the token** (JWT-style `exp`): belongs in Schubert
  (`GrantToken` field + verify-time check) — requested for Schubert 0.5.
  Expiry and revocation are complementary, not substitutes: expiry
  handles routine deprovisioning; revocation handles incidents. This ADR
  does not wait for it.
- **Short-lived tokens only**: chases the same property through TTL
  plumbing and re-mint churn; still can't kill a specific leaked token
  before its TTL.
- **Issuer-key rotation as the kill-switch**: the nuclear option — kills
  *every* grant at once. Retained as the documented emergency lever for
  compromise-of-the-key (as opposed to leak-of-a-token), per the laniakea
  runbook.
- ** denying at the extractor only (not `verify_bearer`)**: would leave
  non-HTTP verifiers (future satellite sync paths) unguarded.

## Consequences

- Every request pays one hash + one set lookup (nanoseconds; the Ed25519
  verify already dominates by orders of magnitude).
- Revocation survives restarts (boot hydration) — pinned by test.
- Un-revoking = delete the row + restart (no route yet; acceptable for
  the incident-driven usage this serves).
- If the same logical grant is re-issued after revocation (same seed,
  same principal+caps ⇒ byte-identical bearer), the revocation kills it
  too. Re-issue after revocation requires changing the grant content
  (e.g. adding a capability) — acceptable; noted for Schubert 0.5
  (nonce/jti would make re-issue distinct).
