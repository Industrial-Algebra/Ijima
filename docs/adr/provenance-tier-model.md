# ADR: Provenance-Tier Model (Schubert-leveraged)

> **Status:** Proposed (2026-07-11). Targets 0.1.0 as the shared foundation
> for Phase 4 (context-poisoning protection) and Phase 5 (federation).
> Design seed context:
> [`docs/discovery/context-poisoning-protection.md`](../discovery/context-poisoning-protection.md),
> [`docs/discovery/networked-instances-federation.md`](../discovery/networked-instances-federation.md).

## Context

Two future features both need `Memory` to carry explicit **provenance**
(where it came from) and **authority** (who is source-of-truth for its
domain), and both need to reason about **trust flow** quantitatively rather
than with ad-hoc conditionals:

- **Phase 4 (poisoning):** doctrine-health registry + stance-budget must
  key on trust tier and trace a pathological memory back to its origin.
- **Phase 5 (federation):** cross-talk egress filters accept content per
  trust tier; conflict resolution defers to the authoritative instance for
  a scope.

Today `Memory` has none of this. `MemorySource` (Explicit / AutoCapture /
Mined / Doctrine) is a flat trust-tier enum with no algebra. Trust-tier
*transitions* are ad-hoc: `promote_memory` (personal→shared, a
trust-increasing act) is gated by plain `memory:write` (codim 2) — the same
capability as an ordinary write at the current tier. That under-rates the
privilege of raising trust.

## Goal

A bounded 0.1.0 foundation, reusing Ijima's existing Schubert capability
algebra (Gr(4,8)) rather than inventing a parallel trust system:

1. **Data:** add `origin` + `authority` provenance to `Memory`.
2. **Trust grade:** map `MemorySource` → a Schubert codimension (the
   quantitative trust axis egress/poisoning will consume).
3. **Schubert leverage:** make trust-tier *transitions* (promotion,
   endorsement, override) into Schubert capabilities — so the same
   geometric policy governs *who may access* and *how trust may flow*,
   with codimension as the cost of raising trust.

The full federation/poisoning machinery (egress filters, detectors,
offline queues) stays in Phase 4/5. This ADR is the substrate they build on.

## Decisions

### P1 — Add `origin` provenance to `Memory`

New field `origin: InstanceId` (a typed newtype wrapping a string). For
0.1.0 (single instance) it defaults to the local instance id
(`IJIMA_INSTANCE_ID`, default `"local"`). Every memory records which
instance authored it. This is the stamp poisoning source-tracing and
federation provenance both need.

### P2 — Add `authority` scope to `Memory`

New field `authority: AuthorityScope` (newtype over namespace/project
string). For 0.1.0 it defaults to the local instance's scope. Drives
Phase 5 conflict resolution (the authoritative instance for a scope wins).
Forward-compatible default; not exercised until federation, but present so
the schema doesn't churn later.

### P3 — Trust grade = `MemorySource` codimension

`MemorySource::trust_grade() -> u64` maps each tier to a Schubert
codimension — *higher = more trusted*:

| Tier | `trust_grade` (codim) | Rationale |
|---|---|---|
| `AutoCapture` | 1 | Ambient, unverified — lowest. |
| `Mined` | 2 | Model-extracted, confidence-routed, review-eligible. |
| `Explicit` | 3 | A human/operator deliberately saved it. |
| `Doctrine` | 4 | Git-versioned, PR-reviewed, curated — highest. |

This is the quantitative axis. Phase 5 egress becomes intersection
arithmetic ("does this content's grade fit the link's trust budget?"),
not a special case. The grades fit comfortably inside Gr(4,8)'s 4×4 box.

### P4 — Trust-tier transitions are Schubert capabilities

Add transition capabilities to the vocabulary (`capabilities.rs` +
`policy/policy.toml`), each *more privileged than a plain write* because
raising trust is costlier than writing at the current tier. Per the design
review, `trust:promote` sits at codim 4 — a consequential write on par
with `mining:trigger` — with the rarer cross-tier and authority-override
actions above it:

| Capability | Partition | Codim | Grants |
|---|---|---|---|
| `trust:promote` | `[3, 1]` | 4 | Promote content to a higher tier / shared namespace. **Replaces** the `memory:write` check on `promote_memory`. |
| `trust:endorse` | `[3, 2]` | 5 | Endorse mined/auto content as Explicit (a cross-tier jump — rarer than promote). |
| `trust:override` | `[4, 2]` | 6 | Override local authority (accept conflicting content) — Phase 5. |

`promote_memory` is refined to require `trust:promote` (codim 4) instead of
`memory:write` (codim 2): the act of *raising* trust is deliberately more
privileged than the act of *writing at* a tier. Default-deny for
`trust:override` in policy (no principal seeded with it). All partitions
fit the Gr(4,8) 4×4 box.

This is the core "leverage Schubert again": trust flow becomes
capability-intersection checks with quantitative cost (codimension), not
ad-hoc booleans. The rate-limiter benefits too — trust-raising actions
get lower throughput capacity (higher codim = higher cost), matching their
risk.

### P5 — `InstanceId` / `AuthorityScope` are typed newtypes (IA convention)

Phantom-typed newtypes over `String`, with stable serde representation,
so instance/scope identifiers are type-distinct from each other and from
free strings (consistent with `MemoryId`, `NamespaceId`, `PrincipalId`).

## What this does NOT do (deferred)

- No egress/ingress filters, no federation links (Phase 5).
- No poisoning detectors / stance-budget (Phase 4).
- No multi-instance replication or conflict resolution (Phase 5).
- `trust:override` is *defined* in the vocabulary but not wired to an
  endpoint in 0.1.0 (Phase 5 wires it).

## TDD sketch

1. `MemorySource::trust_grade()` returns the table above (exhaustive test).
2. `Memory` gains `origin`/`authority` with serde defaults reproducing
   today's behavior (a legacy JSON without the fields deserializes to
   local/local) — migration-safe.
3. `InstanceId`/`AuthorityScope` newtypes: construct, serde, display.
4. `promote_memory` now requires `trust:promote` (a `memory:write`-only
   token gets 403 on promote; a `trust:promote` token succeeds).
5. New capabilities present in `ALL_CAPABILITIES`, have correct
   `intersection_number` (`trust:promote`=4, `trust:endorse`=5,
   `trust:override`=6), and are mirrored in `policy/policy.toml`.
6. Clippy `--all-features -D warnings`, fmt, doc green (CI parity).

## Open questions (for the design pass)

- ~~Codim for `trust:promote`~~ — **decided: 4** (consequential write,
  alongside `mining:trigger`); `endorse`=5, `override`=6.
- Should `trust_grade` participate in wake-up ranking (doctrinal memories
  rank above mined at equal importance/recency)? Likely yes; cheap to add.
- Does the doctrine-ingest path auto-stamp `source = Doctrine` with the
  local `origin`/`authority`? Yes (it's the curated tier).
