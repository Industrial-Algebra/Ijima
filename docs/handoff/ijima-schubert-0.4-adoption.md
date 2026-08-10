# Handoff: Schubert v0.3 → v0.4.0 adoption

**Status:** ready to execute. **Branch:** new `feature/schubert-0.4` off `develop`
(after PR #42 merges — see [the plan](../plans/2026-07-12-pi-integration.md) §9a).
**Driver:** Complete v0.1.0 scope (Phase C). PULSE-killer item #3 ("Ijima Schubert
authz wiring", flagged 4×). Schubert v0.4.0 was built *from Ijima's friction* —
this upgrade deletes the custom code Ijima wrote before v0.4 existed.

## Why

Ijima pins `schubert = "0.3"` and reimplements wire/auth/key-store logic that
v0.4.0 upstreamed. PULSE estimate: ~1 hr for the basic swap; the GrantToken
migration (decision below) adds scope. Schubert v0.4.0 verified healthy
(159 lib + 18 CLI + 16 doc tests pass).

## The current Ijima auth surface (0.3)

| File | Lines | Does | Schubert 0.4.0 replacement |
|---|---|---|---|
| `ijima-server/src/key_store.rs` | 185 | file-based Ed25519 seed persistence | `schubert::KeyStore` (`src/crypto.rs:582`) + `CapabilityIssuer::from_seed(seed)` (`crypto.rs:141`) |
| `ijima-server/src/extractor.rs` | 145 | custom axum `AuthPrincipal(AuthenticatedPrincipal)` extractor | `schubert::axum::AuthPrincipal` (`src/axum.rs:99`, `FromRequestParts`) |
| `ijima-server/src/auth.rs` | 387 | `IjimaAuth { controller, issuer, verifier }`; `verify_bearer`; single-cap `AuthenticatedPrincipal::may()` | `GrantVerifier::may(&self, grant: &GrantToken, cap_partition: &[usize])` (`crypto.rs:529`) + `AccessController::check_single` (`controller.rs:257`) |

Current API used (`auth.rs:36`):
```rust
use schubert::{AccessController, AccessDecision, PrincipalId,
    crypto::{CapabilityIssuer, CapabilityToken, CapabilityVerifier}};
```
`AuthenticatedPrincipal.capability: String` holds a **single** capability; `may()`
checks `self.capability == required || admin`. CLI: `ijima token issue --capability <cap>`
mints **per-capability** tokens; the pi shim uses an env-bundle of 4
(`IJIMA_TOKEN_{MEMORY,KNOWLEDGE}_{READ,WRITE}`).

## The swap

1. **Bump** `ijima-server/Cargo.toml`: `schubert = "0.3"` → `"0.4"`, add the `axum`
   feature (for the upstream extractor). Re-check ed25519-dalek stays 2.x (Schubert
   0.4 uses 2.x — compatible; the 3.0 bump is still blocked).
2. **key_store.rs** → delete Ijima's impl, use `schubert::KeyStore` for seed load/store
   + `CapabilityIssuer::from_seed()`. Keep the `ijima token issue` seed-loading path.
3. **extractor.rs** → swap to `schubert::axum::AuthPrincipal`. **Naming collision**:
   Ijima's handlers use `principal.0.may(...)` (tuple struct). Resolve: either re-export
   Schubert's type or adapt handlers to its field shape. Read `Schubert/src/axum.rs:99`
   for the exact wrapper before adapting.
4. **auth.rs** → `verify_bearer` returns a verified principal whose `may()` now goes
   through `GrantVerifier::may(grant, cap_partition)` instead of the string-equality
   single-cap check. Keep `personal_namespace()`.

## Decision: GrantToken (single multi-cap) vs keep per-capability tokens

**The plan's stated end-state** ([plan §10 risk](../plans/2026-07-12-pi-integration.md)):
"Capability-token bundle — env-bundle for 0.1.0; collapses to one token once Schubert
ships multi-cap grants." Schubert 0.4.0 now ships them (`GrantToken`/`GrantCapability`).
The Schubert PULSE rec #1: "move from per-capability tokens toward GrantToken."

- **Option (a) — mechanics only, keep per-cap tokens:** adopt KeyStore + axum extractor +
  GrantVerifier, but keep issuing per-capability tokens (4-token env-bundle unchanged).
  Smallest blast radius; **does not touch PR #42's pi shim**. Ships the 0.4 upgrade.
- **Option (b) — full GrantToken migration:** one token grants memory:read+write +
  knowledge:read+write. Reworks `ijima token issue` (drop `--capability`, add grant
  minting) **and** the pi shim (`ijimaFetch` collapses to one `IJIMA_TOKEN`). This is the
  plan's end-state and the PULSE recommendation, but it reopens the "done" pi integration.

**Recommendation: (b) for v0.1.0** — it's the coherent end-state, and doing it now
avoids a v0.2 auth-token-format break (token format changes are a published-API risk;
do it before any external consumer). If the shim rework looks large mid-flight, (a) is
a safe fallback that still ships the 0.4 upgrade.

## TDD

The **contract is the capability map** (plan §3.6 + `principal.0.may()` checks). Every
existing auth test in `auth.rs`/`api.rs` must stay green; the live-daemon e2e capability
checks (`/memories/search` needs memory:read, `POST /memories` needs memory:write, etc.)
are the behavioral spec. Add a test for the GrantToken path (one token, multiple `may()`
checks pass) before/with the migration. `ijima token issue` round-trip (mint → verify →
may) must work for both the old and new shapes during the swap.

## Risks

- **`AuthPrincipal` naming collision** — Ijima's tuple-struct vs Schubert's wrapper.
  Resolve deliberately; don't shadow.
- **Token format change** — GrantToken ≠ CapabilityToken wire format. Since Ijima is
  pre-publish (private, v0.1.0 not out), there are no external tokens to invalidate —
  do the format change now, before publish.
- **policy.toml** — confirm Schubert 0.4 still loads Ijima's `policy/policy.toml` shape
  (the policy vocabulary is Ijima's: `ijima_core::capabilities::*`).
- **CLI break** — `token issue --capability` changes shape under option (b); update
  `ijima-server/src/main.rs` + the docs that show mint commands.

## Sources of truth (read during execution)
- Ijima: `ijima-server/src/{auth,extractor,key_store}.rs`, `policy/policy.toml`, `ijima-core/src/capabilities.rs`.
- Schubert 0.4.0: `~/working/industrial-algebra/Schubert/src/{crypto,axum,controller}.rs`.
- Decisions: [plan §10 risk note](../plans/2026-07-12-pi-integration.md) (token bundle → single token).
