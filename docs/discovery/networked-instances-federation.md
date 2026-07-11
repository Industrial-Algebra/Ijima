# Networked Instances & Cross-Talk Policies (design seed)

> **Status:** Proposed future feature (v0.2+). Not blocked — pure forward
> design. This document captures the exploration so the design can resume
> without re-deriving it. **Composes with** the context-poisoning work
> ([`context-poisoning-protection.md`](context-poisoning-protection.md)):
> both rest on a shared trust/provenance-tier foundation (see §5).
>
> **Architecture (2026-07-11 refinement):** the federation control plane
> lives in **Dominic** (`../Dominic`), the Anima meta-orchestrator, not in
> Ijima. Ijima instances are the memory plane; Dominic brokers cross-talk.
> See §1.1.

## 1. Where this sits

D2 settled **single-daemon, multi-tenant, Tailscale-private**, with
namespaces isolating operators *within one daemon*. "Networked instances"
is the layer above: **multiple Ijima daemons** that federate. It must
justify itself over "just add namespaces to the existing daemon" — and it

## 1. Where this sits

D2 settled **single-daemon, multi-tenant, Tailscale-private**, with
namespaces isolating operators *within one daemon*. "Networked instances"
is the layer above: **multiple Ijima daemons** that federate. It must
justify itself over "just add namespaces to the existing daemon" — and it
does, for use cases a shared daemon cannot serve (§2).

## 1.1 Three tiers: Dominic orchestrates, Ijima enforces locally

The federation is **not peer-to-peer between Ijima instances**. It is a
three-tier architecture:

```
   harnesses (pi / Wallace / Sakamoto / Tsume)
        │  read/write memory
        ▼
   Ijima instances  (memory plane + LOCAL boundary enforcement)
        ▲  control API (state, accept-routed, policy guard)
        │
   Dominic  (orchestration plane: topology, routing, conflict,
             offline coordination, domain delegation)
```

- **Dominic-mediated cross-talk (the smart path).** Dominic decides what
  crosses between instances, when — routing, domain delegation to
  `domain-authority` instances, edge-queue replay on reconnect, conflict
  adjudication. This is orchestration intelligence, which is Dominic's
  defined role (it already coordinates the harness plane).
- **Ijima-local hard enforcement (the safety floor).** Each instance
  non-bypassably enforces trust-tier egress, airgap deny-lists, and scope
  filters at its own boundary — so sovereignty holds even when Dominic is
  unreachable, compromised, or the instance is airgapped/offline.

**Net effect for Ijima:** it does **not** need a peer-to-peer consensus
protocol. It needs (a) boundary policy enforcement and (b) a
Dominic-facing control API (expose state; accept routed writes). The
heavy federation logic (routing, conflict, multi-instance offline
coordination) is Dominic's job. This materially *shrinks* Ijima's
federation surface and is the natural split, since Dominic already
coordinates pi/harness instances.

Note: Dominic is currently a **greenfield stub** (designed, not yet built).
The orchestration-side work described here is net-new in Dominic.

## 2. Motivating use cases (all in scope)

- **Airgap / sovereignty** — memory that must *never* cross a network
  boundary (client-confidential, classified work). A shared daemon can't
  enforce a hard egress wall; separate instances with default-deny links can.
- **Offline / edge + central** — a travel/edge machine works disconnected
  and syncs on reconnect. A server-only daemon doesn't serve offline work.
- **Multi-org / multi-trust-domain federation** — Wallace serving operators
  from different trust domains; each org's instance shares *selectively*
  (scoped projects), not via one shared namespace.
- **Resilience / redundancy** — no single point of failure (replica + failover).
- **Hub-spoke with role specialization** — a *unifying* (authoritative)
  instance talking to *limited/specialized* instances: an **archive**
  instance (backup / larger cold storage), and **domain-authority**
  instances the unifying instance *defers to* for their domain knowledge.

The unifying signal: the design must be a **general, policy-driven
federation layer** parameterized by per-link policy — not a single-purpose
sync. Each use case is a *policy combination* over the same primitives.

## 3. Core model — instances as federation nodes

- **Instance identity.** Each instance has a stable id (extends the Context
  Mapper / `Principal` model). Every record crossing a boundary is stamped
  with its origin instance.
- **Roles.** An instance declares a role:
  - `unifying` — aggregator / authoritative hub.
  - `archive` — cold storage / backup / larger-storage tier.
  - `domain-authority` — source-of-truth for a specific namespace/project
    domain; others defer to it there.
  - `edge` — offline-capable replica that syncs to a central instance.
  - `airgapped` — sovereign; default-deny egress.
- **Authoritative scopes.** Each instance declares which
  namespaces/projects/topics it is source-of-truth for. This drives
  conflict resolution (§4): the authoritative instance wins in its scope —
  no CRDTs needed for the common case.

## 4. Cross-talk policies (the real design)

Per §1.1, policies are **evaluated in two places**: Dominic reasons about
them to decide routing (mediated), and each Ijima instance **enforces** a
non-bypassable subset locally (the safety floor). A link between instance
A and B carries a policy with five axes:

| Axis | Values | Enforced where |
|---|---|---|
| **Direction** | `replica` (A→B read-only) · `sync` (bidirectional) · `subscribe` (B pulls) · `airgap` (hard-deny) | Dominic routes; Ijima enforces `airgap` locally. |
| **Scope filter** | namespace/project/topic allow/deny lists | **Both** — Dominic honors scope when routing; Ijima rejects out-of-scope writes at the boundary (default-deny for sovereignty). |
| **Trust-tier egress** | which `MemorySource` tiers cross, and how | **Ijima-local, always** (see §5): mined/auto cross as-is; **doctrine/trusted-tier crosses only as `PendingReview`, never auto-promoted** on the receiver. |
| **Conflict resolution** | `source-authority` (default) · `last-write-wins` · `CRDT-merge` | Primarily **Dominic** adjudicates; instances apply the result. |
| **Freshness** | `realtime` (streaming) · `batched` (scheduled + offline queue) | Dominic schedules; edge instances hold the offline write-queue. |

The asymmetry is deliberate: the **trust-tier egress and scope filters are
Ijima-local and non-bypassable**, so Dominic cannot — even if buggy or
compromised — push doctrine into a sovereign instance or violate a
scope boundary. Direction/conflict/freshness are Dominic's to manage.
This is defense in depth: Dominic is the brain, the instances are the
guards.

Plus a **boundary transformation** pass at the wire: redact secrets (reuses
the existing redaction seam), downgrade promotion level, stamp origin
provenance, re-embed if dimensions differ.

## 5. The shared foundation: an explicit trust/provenance-tier model

Both this feature and context-poisoning protection need the *same* thing:
Memory carrying an explicit, first-class trust/provenance tier —
`Doctrine` / `Explicit` / `Mined` / `AutoCapture` (already in
`MemorySource`), **plus two new dimensions**:

- **Origin instance** — which instance authored a record (provenance for
  federation, and for poisoning source-tracing).
- **Authority scope** — which instance/namespace is authoritative for the
  record's scope (drives conflict resolution *and* "is this doctrine
  authoritative here").

With that model in place:
- **Federation cross-talk** = trust-tier egress filtering + authority-scoped
  conflict resolution at the link boundary.
- **Poisoning protection** = doctrine-health registry + stance-budget, all
  keyed on the same tier/provenance.

**Roadmap implication:** design the unified trust-tier model *early* (0.1.x)
— it's small and cheap, and it unlocks both future features. Build the
federation layer itself in 0.2+ (the big lift: sync, conflict, offline
queues, replication, failover).

## 6. Composability with existing capabilities

- **D2 namespaces + capabilities** — namespaces are the natural scope unit
  for policy filters; an inter-instance link is modeled as a scoped
  principal (a peer instance with scoped capabilities). The capability
  model already exists.
- **`MemorySource` provenance** — the foundation tier already exists; §5
  extends it.
- **Redaction filter** — boundary transformation reuses this seam.
- **Promotion (personal→shared)** — the *downgrade* direction at egress.
- **Knowledge-graph triples** — global-vs-provenance decision (D2 open
  question) sharpens here: which facts may cross an org boundary.
- **Context Mapper** — instance identity + scope mapping.

## 7. Topology patterns (use cases → policy combos)

| Use case | Role | Link policy |
|---|---|---|
| Airgap/sovereignty | `airgapped` | `airgap` / default-deny egress |
| Offline/edge + central | `edge` | `batched sync` + offline write-queue; central `source-authority` on its scopes |
| Multi-org federation | per-org `unifying` | `sync` scoped to shared projects + trust-tier egress |
| Resilience | peer | `replica` + authority failover |
| Hub-spoke specialization | `unifying` hub | `replica`→`archive`; defer to `domain-authority` via `source-authority` |

## 8. Resume checklist

**Ijima-side (memory plane):**

1. Decide the v0.1.x trust-tier extension (origin-instance + authority-scope
   on `MemorySource`/`Memory`) — shared with poisoning protection; do first.
2. Boundary policy enforcement: non-bypassable trust-tier egress + scope
   filter + airgap deny-list at the instance edge.
3. Boundary transformation pipeline (redact → downgrade → stamp → re-embed).
4. A Dominic-facing control API (expose instance state/role/scope; accept
   routed writes; report conflict signals).
5. ADR; implement behind a `federation` feature gate.

**Dominic-side (orchestration plane — net-new, Dominic is a stub today):**

6. Federation topology model: instances as nodes with roles + authoritative
   scopes (§3); links with policies (§4).
7. Routing + delegation logic: where a write/query goes, including
   domain-authority deferral and hub→archive replication.
8. Conflict adjudication: source-authority default; CRDT only if a real
   multi-writer-same-scope case appears.
9. Offline/edge coordination: schedule batched syncs, replay edge
   write-queues on reconnect.
10. Failover/resilience: authority transfer when an authoritative instance
    is lost.

> **Sequencing:** step 1 (trust-tier foundation) is shared with the
> context-poisoning feature and should land first in 0.1.x. Steps 2–4 are
> the minimal Ijima federation surface for v0.2. The Dominic-side work
> (6–10) proceeds in parallel once Dominic is built out beyond its stub.
