# Security Considerations

Ijima's access model is [Schubert](https://github.com/Industrial-Algebra/Schubert)
capability algebra on the Grassmannian **Gr(4,8)** — quantitative,
geometry-based authorization, not ad-hoc role checks.

## Identity & authentication

- **Principals** are operators or harnesses; a request is always
  `(principal, harness, action)`.
- **Proof-carrying tokens**: each token carries exactly one capability and
  a proof the daemon verifies. No shared all-or-nothing bearer secret (the
  pi-mempalace model Ijima replaces — see DESIGN D2).
- Tokens are issued by the `ijima token` CLI using an issuer key created on
  first run under `IJIMA_DIR`.

## Authorization

Eleven capabilities as Schubert partitions; the **intersection number**
(codimension) of a capability is both its authorization weight and its
rate-limit capacity. `memory:read` (codim 1) → 1× throughput; `memory:write`
(2) → 2×; `admin` (the point class σ₄₄₄₄, codim 16) → 16×. *The geometry of
access maps to the geometry of throughput.*

## Trust tiers & trust-flow

Trust-tier **transitions** are themselves capabilities (the provenance-tier
model): `trust:promote` (codim 4) gates personal→shared promotion — raising
trust is costlier than writing at a tier. `trust:endorse` (5) and
`trust:override` (6, default-deny) cover cross-tier endorsement and
authority override. The same geometric policy therefore governs *who may
access* and *how trust may flow*.

## Boundaries

- **Namespace isolation** is enforced at the API layer; a principal never
  sees another operator's private namespace.
- **Promotion is the single redaction boundary** — the one place content
  filtering (secret stripping) happens. Personal storage is always
  verbatim.
- **Provenance** (origin instance + authority scope + source tier) on every
  memory is the foundation for the planned federation cross-talk policies
  and context-poisoning protection.

## Known limitations (honest)

- v0.1.0 is **single-instance**: federation (multi-instance, cross-talk
  policies) is designed but not yet implemented.
- Rate limiting is per-principal token-bucket; it is not a DoS shield —
  deploy behind a real proxy on untrusted networks.
- TLS is opt-in (`tls` feature); on a private Tailscale network, plain HTTP
  is the documented default.
- Context-poisoning protection (defending against trusted-tier doctrine
  going pathological) is designed but not yet implemented.

See `docs/DESIGN.md` (D2, D4, D5) and `docs/adr/provenance-tier-model.md`.
