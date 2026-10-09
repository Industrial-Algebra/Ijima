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
- **`Doctrine`** — curated, Git-versioned, PR-reviewed memory mirrored; since 0.3.0 it can live in org-scoped walls (`POST /doctrine?namespace=`), keeping internal corpora out of the instance-global namespace.
  from the repository seed pack. Never written directly by agents.

Trust *transitions* are themselves capabilities: `trust:promote` raises an
entry's tier, and cross-tier endorsement/override are progressively more
expensive in the capability algebra (see
[Capabilities](./capabilities.md)). Raising trust costs more than writing
at a tier — by construction.

## The evidence axis (0.4)

Provenance answers **who wrote it**; evidence grade answers **how they
know it**. They are independent axes — the grade crosses tier lines. An
`Explicit` save by an operator can still be an interpretation, and a
low-tier `AutoCapture` row can carry a directly observed fact.

| Axis | Question | Values |
|---|---|---|
| Provenance tier (`source`) | Who (or what) wrote this? | `Explicit`, `AutoCapture`, `Mined`, `Doctrine` |
| Evidence grade (`evidence`) | How does the author know it? | `Observed`, `Interpreted` |

- **`Interpreted`** is the default. Inference, judgment, summary, and any
  ungraded save land here — including every legacy row written before
  0.4.0 and every save that never set a grade.
- **`Observed`** means the authoring process directly witnessed the thing
  itself — a session transcript event, command output, or a git artifact.
  An observed claim **must cite** (`citations` >= 1): no citation, no
  observed claim. The server rejects `Observed` without citations at save
  time (`POST /memories` → `400`).

Citations are typed pointers (`kind` ∈ `Commit`, `Report`, `Session`,
`File`, `Url`; `locator` opaque) to the artifact that grounds the claim.
A grade is a claim about knowledge, not a credential — raising the
provenance tier does not by itself make an entry observed.

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
