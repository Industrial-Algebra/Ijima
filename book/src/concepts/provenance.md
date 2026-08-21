# Provenance & Trust Tiers

Every memory in Ijima carries a provenance block. Provenance is not
metadata garnish — it is the basis for trust decisions, promotion, and
(eventually) federation conflict resolution.

## The provenance fields

| Field | Meaning |
|---|---|
| `source` | Trust tier: `Explicit`, `AutoCapture`, `Mined`, or `Doctrine` |
| `harness` | Which harness wrote it (`Pi`, `Dominic`, `Wallace`, …) |
| `origin` | The instance that authored the entry |
| `authority` | Source-of-truth scope for the entry's domain |
| `session_id` | Originating session, when known |

## Trust tiers

- **`Explicit`** — an operator or harness deliberately saved it. Highest
  routine trust.
- **`AutoCapture`** — an automatic hook wrote it. Unverified.
- **`Mined`** — extracted from a session transcript by the miner,
  carrying a confidence score until reviewed.
- **`Doctrine`** — curated, Git-versioned, PR-reviewed memory mirrored
  from the repository seed pack. Never written directly by agents.

Trust *transitions* are themselves capabilities: `trust:promote` raises an
entry's tier, and cross-tier endorsement/override are progressively more
expensive in the capability algebra (see
[Capabilities](./capabilities.md)). Raising trust costs more than writing
at a tier — by construction.

## Imports land unverified

`ijima import` stamps every imported memory `origin = <source>` and drops
the tier to **`AutoCapture` regardless of its original classification** —
a `manual-save` row from a workstation's pi-mempalace arrives as
AutoCapture. Imported content is unverified until promoted through the
review path. This is deliberate: an import is a claim, not a credential.

## Why authority matters

`authority` records *whose* fact this is — the local instance, or a
remote instance's scope. In the single-instance present it is uniformly
`local`; when federation lands, per-domain authority scopes drive
cross-instance conflict resolution (the instance whose authority scope
matches a domain wins that domain's writes).
