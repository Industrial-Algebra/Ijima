# Security Considerations

Ijima's access model is [Schubert](https://github.com/Industrial-Algebra/Schubert)
capability algebra on the Grassmannian **Gr(4,8)** — quantitative,
geometry-based authorization, not ad-hoc role checks.

## Identity & authentication

- **Principals** are operators or harnesses; a request is always
  `(principal, harness, action)`.
- **GrantTokens** are proof-carrying, ed25519-signed bearer credentials
  bundling one or more capabilities; the capability list is inside the
  signed blob. Verified by the daemon's `GrantVerifier`.
- **Write implies read** in the geometry (`memory:write` ⊇
  `memory:read`); the converse never holds.
- **Revocation**: store-backed SHA-256 hash list checked after signature
  verification; survives daemon restarts; raw bearer values never touch
  the store, logs, or backups. Issuer-key rotation remains the
  emergency lever (invalidates every grant at once).

## Authorization

Eleven capabilities as Schubert partitions; the **intersection number**
(codimension) of a capability is both its authorization weight and its
rate-limit capacity. `memory:read` (codim 1) → 1× throughput;
`memory:write` (2) → 2×; `admin` (the point class σ₄₄₄₄, codim 16) → 16×.
*The geometry of access maps to the geometry of throughput.*

## Trust tiers & trust-flow

Trust-tier **transitions** are themselves capabilities (the
provenance-tier model): `trust:promote` gates tier promotion — raising
trust is costlier than writing at a tier. `trust:endorse` and
`trust:override` (default-deny) cover cross-tier endorsement and
authority override. The same geometric policy therefore governs *who may
access* and *how trust may flow*. Imported content lands `AutoCapture`
regardless of origin claims — an import is a claim, not a credential.

## Boundaries

- **Namespace isolation** is enforced at two layers: `resolve_ns` at the
  API (foreign `*_private` is always forbidden) and namespaced record
  keys at the store (same logical id in two namespaces cannot collide or
  cross-read).
- **Promotion is the single redaction boundary** — the one place content
  filtering (secret stripping) happens. Personal storage is always
  verbatim.
- **Provenance** (origin instance + authority scope + source tier) on
  every memory is the foundation for federation cross-talk policies and
  context-poisoning protection.

## Known limitations (honest)

- v0.2.0 is **single-instance**: federation routes exist as a scaffold;
  cross-instance enforcement is 0.3+.
- Rate limiting is per-principal token-bucket; it is not a DoS shield —
  deploy behind a real proxy on untrusted networks.
- TLS is opt-in (`tls` feature); on a private Tailscale network, plain
  HTTP behind `tailscale serve` is the documented default.
- GrantToken expiry is adopted (Schubert 0.5); the instance-side
  revocation list remains as defense-in-depth, and CRDT tombstones are
  deferred to 0.3 satellites.
- Context-poisoning protection is designed but not yet implemented.

See `docs/DESIGN.md` and `docs/adr/` (grant-token-migration,
token-revocation, provenance-tier-model).
