# ADR: Namespace Membership (WS3 Org Walls)

**Status:** Accepted (2026-08-20)
**Context:** The v0.2.0 Central Brain deployment puts four principals
classes (IA / Kellas Cat Games / Shiroyama Electric / personal) on one
instance. Namespaces are the isolation unit, but pre-WS3 any
authenticated principal could read/write any non-private namespace —
fine for one operator, wrong for an org.

## Decision

**Shared namespaces are membership-gated; membership lives in the
store, not policy TOML.**

### Classification (resolve_ns, check order)

1. own `ns_<principal>_private` → allowed;
2. any other `*_private` → 403 (unchanged);
3. **open** namespaces — `global`, `ns_doctrine`, `ns_import_*`
   (staging) → any authenticated principal;
4. everything else (shared org namespaces, e.g. `ns_ia_shared`,
   `ns_kellas_shared`) → store membership **or** `admin` bypass.

### Why membership-in-store over policy-TOML grants

The issuance policy (Schubert 0.5 #20.3) is embed-at-build + file
overlay — changing it is an operator file edit, but it gates *minting*,
not *reading*: already-issued proof-carrying grants keep verifying.
Membership is a runtime fact that must take effect immediately for
principals holding valid grants, survive restarts, and be auditable
(principal × namespace × granted-by × when). A SurrealDB table
(`namespace_members`, natural key `<ns>:<principal>`, upsert-grant /
idempotent-revoke) does all three; a policy file does none.

### Promotion target is also gated (closed tunnel)

Pre-WS3, `POST /memories/:id/promote` wrote the redacted shared copy
directly to the store — the target namespace never passed `resolve_ns`,
so promotion could tunnel through any org wall. WS3 runs promotion
targets through the same rule (member or admin; foreign-private
forbidden; import staging rejected as a target).

### Admin surface

`POST /namespaces/grant`, `POST /namespaces/revoke`,
`GET /namespaces/members?namespace=` (admin capability — kept;
`namespace:admin` vocabulary churn is not worth a policy rebuild), plus
`ijima namespace grant|revoke|members` CLI (remote, `--auth <admin>`).

## Consequences

- Deploy-time provisioning = grant memberships for the four org walls
  (`ns_ia_shared`, `ns_kellas_shared`, `ns_shiroyama_shared`,
  `ns_writing_shared`) — documented in the runbook.
- Import staging stays open by design: staging content is AutoCapture
  (unverified) and the wall that matters is promotion.
- Membership checks are one indexed record lookup per request
  (`is_namespace_member` natural-key select) — no measurable hot-path
  cost.
- 0.3 satellites will need membership in the checkpoint export (a
  satellite's local walls must round-trip); noted for the WS6 design.

References: v0.2.0 plan WS3, `docs/adr/provenance-tier-model.md`
(promotion as the trust boundary), Schubert `#20.3` (issuance-vs-
access distinction).
