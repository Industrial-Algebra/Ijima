# ADR: Federation Control API — Scaffold

> **Status:** Accepted (2026-08-11). Scaffold behind the `federation` feature.
> Boundary enforcement is deferred (see §Deferred). Implements federation
> design seed step 4 + step 5 (ADR + feature gate) of
> [`../discovery/networked-instances-federation.md`](../discovery/networked-instances-federation.md).

## Context

Dominic's `FederationClient` contract ([`FederationState`] /
[`RoutedWrite`] / [`RoutedWriteReceipt`] / [`ConflictSignal`], in
`dominic-core`) needs a server to talk to. The federation seed sequences the
control API as a v0.2+ feature resting on a provenance foundation — and that
foundation (origin-instance + authority-scope on `Memory`, ADR
[`provenance-tier-model.md`](provenance-tier-model.md)) landed in v0.1.0. So
the control API is now buildable, and building the scaffold lets Dominic
develop and test its client against a real server rather than a mock.

## Decision

Implement the three federation control routes as a **scaffold** behind a
`federation` feature gate, with minimal-but-correct semantics:

| Route | Handler | Scaffold behavior |
|---|---|---|
| `GET /federation/state` | `federation_state` | Returns the instance's [`InstanceFederationConfig`] rendered as a [`FederationState`]. Default config = local / `Unifying` / local scope / no links. |
| `POST /federation/routed-write` | `routed_write` | Auth-gates on `memory:write`, deserializes the payload as a `Memory`, **stamps federation provenance** (origin = this instance, authority = the routed scope), stores it, returns a receipt with `accepted: true` + the commit id. **No egress filtering.** |
| `POST /federation/conflict-signal` | `conflict_signal` | Returns `404` (no active conflict). Conflict detection is not yet implemented. |

### DTO ownership — decoupled, JSON is the contract

The federation wire types live in **`ijima-core/src/federation.rs`** (behind the
feature). They mirror `dominic-core`'s topology/federation types **exactly**
(InstanceId as a plain string, AuthoritativeScope as `{namespace, project}`,
PascalCase enums) so the JSON is byte-compatible without either crate depending
on the other. This is the canonical home; `dominic-core` carries a duplicate set
it anticipates unifying here once it depends on `ijima-core` (see the doc comment
on dominic-core's `InstanceId`).

### Provenance stamping

`routed_write` reuses the v0.1.0 provenance foundation: the applied `Memory`
gets `origin = InstanceId::local()` and `authority = AuthorityScope("<ns>/<project>")`
derived from the routed scope. This exercises the provenance fields that
Phase 5 conflict resolution will eventually key on.

## Consequences

- Dominic's `FederationClient` has a real server for end-to-end development.
- The federation wire contract is concrete + tested (round-trip + route tests).
- The `federation` feature is opt-in; default builds + the pre-commit gate are
  unaffected. CI exercises it via `--all-features`.

## Deferred (the v0.2 proper — do NOT mistake the scaffold for secure federation)

The scaffold applies writes **without** the non-bypassable safety floor the seed
mandates (§1.1, §4). None of the following is implemented yet:

- **Boundary enforcement** — trust-tier egress filtering (doctrine crosses only
  as `PendingReview`, never auto-promoted), scope filters, airgap deny-lists at
  the instance edge. This is the hard, security-critical part.
- **Boundary transformation pipeline** — redact → downgrade → stamp → re-embed.
- **Conflict detection** — `conflict_signal` always returns `404` today; real
  multi-writer-same-scope detection + `source-authority` adjudication is future.
- **Real instance config** — `InstanceFederationConfig::default()` is hardcoded
  (local / `Unifying`); env/config-file driven role, scopes, and outbound links
  (incl. the `IJIMA_INSTANCE_ID` override) are a follow-on.
- **`capability_policy_ref` + `etag`** — left `None`; policy-hash population and
  cache validation land with the config surface.

A routed write today is therefore acknowledged + persisted locally with correct
provenance, but an operator must **not** deploy the scaffold as if it enforced
federation sovereignty. The `warnings` field on every scaffold receipt says so
explicitly.

## Cross-references

- Seed: [`../discovery/networked-instances-federation.md`](../discovery/networked-instances-federation.md)
  (§1.1 tiers, §4 policies, §8 resume checklist).
- Foundation: [`provenance-tier-model.md`](provenance-tier-model.md),
  [`../discovery/context-poisoning-protection.md`](../discovery/context-poisoning-protection.md).
- Consumer contract: `dominic-core/src/federation.rs` (`FederationClient`).
