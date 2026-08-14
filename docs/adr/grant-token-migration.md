# ADR: GrantToken Migration (multi-capability tokens)

Date: 2026-08-10
Status: Accepted (executing)
Supersedes: option (a) of the Schubert 0.4 adoption (PR #44) — which
deliberately deferred GrantToken to v0.2; pulled forward per the
2026-08-07 PULSE's "Ijima is not yet really consuming Schubert" finding.

## Context

The 2026-08-07 PULSE of Schubert observed that Ijima — Schubert's first
real consumer — was "consuming v0.4 through a pre-v0.4 integration": it
pinned `schubert = "0.3"` (stale by the time of the audit — PR #44 bumped
to 0.4), **reimplemented the token wire format that v0.4 upstreamed
natively**, and kept the per-capability token model instead of the v0.4
`GrantToken`.

An audit against Schubert 0.4.0 confirmed three gaps:

1. **Wire-format duplication.** Ijima's `encode_token`/`decode_token` +
   `read_u16`/`read_str`/`read_bytes` + length consts (~60 lines in
   `ijima-server/src/auth.rs`) are a **byte-for-byte copy** of Schubert
   0.4's `CapabilityToken::to_bytes()`/`from_bytes()` — identical
   length-prefixed layout, identical signed message
   (`principal || capability || issuer_key`).
2. **No `GrantToken`.** Ijima issues one capability per token; the pi
   shim carries a 4-token env-bundle (one per capability). Schubert 0.4's
   `GrantToken` (`capabilities: Vec<GrantCapability>`, geometrically
   verified by `GrantVerifier::may()`) collapses that to one token.
3. **Geometric `check()` loaded but unused at runtime.** Runtime authz is
   proof-carrying string match (`capability == required || == admin`).
   `AccessController::check()` is wrapped, never called by a handler.

## Decision

Migrate Ijima to Schubert 0.4's `GrantToken` end-to-end, in one pass:

1. **Issue as `GrantToken` always** (a single-capability grant is a
   `GrantToken` with one entry). Use
   `CapabilityIssuer::issue_grant(principal, &[(CapabilityId, partition)])`,
   resolving each capability's partition from the `AccessController`
   (`controller.capability(id).partition`).
2. **Wire format = `GrantToken::to_bytes()`/`from_bytes()`.** Delete
   Ijima's `encode_token`/`decode_token` and the `read_*`/length helpers.
   This resolves gap #1 with **zero behavior change to the cryptography**
   (same Ed25519 signing primitive) but a **new bearer blob layout**
   (GrantToken, not CapabilityToken).
3. **Geometric verify via `GrantVerifier`.** `verify()` checks the
   signature; `may(grant, required_partition)` checks geometric
   containment (`cap_partition ≤ λ` component-wise over granted
   partitions λ). The partition is **signed into the token**, so the
   geometric check is self-contained — no capability registry needed for
   the authz decision, only for the static required-capability → partition
   lookup. This makes the `AccessController` runtime-load-bearing (resolves
   gap #3) for that lookup alone.
4. **`AuthenticatedPrincipal` carries the verified `GrantToken` + shared
   `Arc<AccessController>` + `Arc<GrantVerifier>`.** `may(required)` looks
   up `required`'s partition from the controller and delegates to
   `GrantVerifier::may`. The `admin` string short-circuit is **removed** —
   `[4,4,4,4]` (the point class) implies every partition by geometry.

### pi shim

Collapse the 4-token env-bundle to **one** grant token. The daemon mints
one `GrantToken` carrying all the shim's capabilities
(`memory:read`, `memory:write`, `knowledge:read`, `knowledge:write`, …);
the shim sends the single bearer for every op. `may()` admits each op by
geometry.

### CLI

`ijima token issue` gains `--capabilities a,b,c` (multi) issuing a grant;
`--capability X` (singular) remains as a single-entry convenience.

## Consequences

### Behavior changes (intentional)

- **New bearer wire format.** Tokens minted by v0.1.0 (CapabilityToken
  blobs) will **not** verify under the new verifier. This is a clean break
  — Ijima v0.1.0 has no external token consumers (the pi shim generates
  its own; the CLI mints them). Acceptable.
- **Write implies read.** Geometry gives `[2] ≥ [1]`, so a grant carrying
  `memory:write` now also satisfies `memory:read`. This is the **safe**
  least-privilege-with-implication direction (write is strictly higher
  privilege). Read still does **not** imply write (`[1] ≱ [2]`) — the
  existing `read_token_does_not_grant_write` invariant holds.
- **Admin via geometry, not string.** Observable behavior is unchanged
  (admin still grants everything); the mechanism is geometric containment
  of `[4,4,4,4]`, not a `== "admin"` check.

### What this closes

- PULSE finding "Ijima reimplements wire/auth/key-store logic that v0.4
  upstreamed" (the wire-format half; key_store was already delegated in
  PR #44).
- Gap #3 (controller loaded-but-unused): the controller becomes the
  required-capability → partition resolver at verify time.
- The pi shim's 4-token model → 1-token model (simpler, matches the
  upstream design intent).

### What this does NOT do

- Federation (Phase 5) — the cross-instance trust model still relies on
  the provenance-tier model, not multi-issuer grant delegation. A future
  `trust:endorse`-as-grant story is possible but out of scope.
- `issue_batch` / `verify_batch` — not needed; one grant subsumes batches.

## Verification

- `cargo test --all-features` (rewrite the auth.rs test suite for the
  grant model: issue/verify round-trip, tamper-rejection, write-implies-
  read, read-doesn't-imply-write, admin-implies-all-via-geometry,
  multi-cap grant, seed-based cross-instance verify).
- `cargo clippy --all-features --all-targets -- -D warnings`.
- pi shim: collapse token model; smoke-test that a single grant bearer
  admits read + write ops.
