# ADR: Schubert 0.5 Adoption (expiry, policy-constrained issuance, revocation reconciliation)

**Status:** Accepted (2026-08-20)
**Context:** Schubert 0.5.0 (published 2026-08-20) ships GrantToken expiry
+ nonce (#20.1/ADR-0001), grant-aware CRDT revocation (#20.2/ADR-0002), and
policy→issuance linkage (#20.3). Ijima v0.1.0→develop consumes Schubert 0.4.

## Decision

Adopt Schubert 0.5 **before the v0.2.0 tag** (full adoption), in the
zero-re-mint window: no production daemon exists yet, so the GrantToken
wire-format break (`nonce(16) | tag(1) | expires_at`) costs nothing today
and would cost a fleet-wide re-mint after laniakea deploys.

### 1. Expiry — adopted, opt-in per grant

- `ijima token issue --expires-in <SECONDS>` → `issue_grant_with_expiry`
  (boundary inclusive per ADR-0001: dead when `now >= expires_at`).
  Omitted = never (pre-0.5 behavior).
- Verification needs no daemon change: `GrantVerifier::verify` checks
  signature-then-expiry; a distinct `GrantExpired` error now surfaces in
  the 401 body (e.g. `grant verify: grant expired: expires_at …, now …`),
  satisfying the "which check fired is telemetry" composition rule.
- **Practice** (runbook/book): machine-feed and service principals always
  carry an expiry; human/operator grants may run long or never. Renewal =
  re-issue (fresh nonce), never mutation.

### 2. Policy-constrained issuance — CLI-enforced, overlay files

- `ijima token issue` now resolves a policy and signs only what it
  entitles (`issue_grant_under_policy` semantics; fails closed: unknown
  principal or over-entitled request denies, no geometry smuggling).
- Resolution: `--policy PATH` > `$IJIMA_POLICY` > `$IJIMA_DIR/policy.toml`
  > embedded default. An explicit pointer at a missing file is a hard
  error (config-layer convention).
- **Principals-only overlay**: the operator file may contain only
  `[principals.<name>] grants = [...]`, merged onto the embedded policy's
  partitions. Partitions always derive from embedded — the overlay can
  assign capabilities but never redefine geometry (the #20.3
  anti-smuggling invariant). A file carrying `[capabilities]` must be a
  complete, valid policy.
- **Bootstrap**: the embedded policy seeds no principals — a fresh
  install mints nothing until the operator provisions a policy file.
  This preserves (and strengthens) the "fresh install starts with no
  access" posture: issuer-key possession alone no longer mints.
- The daemon is unchanged: verification is proof-carrying (grant + public
  key), so principals matter only at issuance.

### 3. Revocation reconciliation — keep the instance-side hash list

Ijima's WS1b revocation (store-backed SHA-256 bearer-hash list, checked
after signature verification) is **kept as defense-in-depth**; Schubert's
CRDT nonce-tombstones are **deferred to v0.3 satellites** where merge
semantics matter:

- The hash list kills any bearer — leaked, forged-shape, or pre-expiry —
  without issuer involvement, including for grants issued before expiry
  existed.
- CRDT tombstones are issuer-side and merge-replicated; valuable when
  multiple instances must agree on revocation, which is the 0.3
  federation/satellite problem, not the 0.2 single-instance problem.
- Composition (ADR-0001 rule 5, extended): valid signature AND not
  expired AND not hash-revoked. Order in `verify_bearer`: revocation →
  decode → signature → expiry; which check fired is telemetry.

## Consequences

- Wire break: 0.1.0-published bearers do not verify against 0.2.0+ (the
  v0.2.0 tag closes the published-artifact gap the PULSE flagged).
- `ijima token issue` UX change: minting requires a provisioned policy
  file (bootstrap step added to the deploy runbook/book).
- 401 bodies now carry failure detail — clients must treat the body as
  informational, not parse it for control flow.
- Re-adopting CRDT tombstones in 0.3 will need a bridge decision (hash
  list ↔ tombstone-set); noted for the WS6 satellite design.

References: Schubert `docs/adr/0001-grant-expiry-semantics.md`,
`docs/adr/0002-grant-crdt-revocation.md`, Schubert ROADMAP #20 addendum
(expiry↔revocation interaction rules), Ijima
`docs/adr/token-revocation.md`, `docs/adr/grant-token-migration.md`.
