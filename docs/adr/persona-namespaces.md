# ADR: Persona Namespaces (`persona:{id}`)

**Status:** Proposed (2026-08-29)
**Context:** the persona arc's three legs are now all shipped — **Dominic
defines** (static TOML registry, D-09), **Tsume wears** (PersonaSessionHost,
Unit 09) and **writes** (IjimaEmitter with `?namespace=`, Plan 10/PR #17),
**Ijima remembers**. This ADR names the convention so the upcoming persona
imports (Sara), drift diagnostics, and 0.3 satellites all target one shape
instead of inventing three.

## Decision

### 1. Naming and meaning

A persona's lived experience lives in a **shared namespace named
`persona:{id}`** (`persona:sara`, `persona:mirror`), where `{id}` matches the
Dominic registry's persona id. The namespace holds **persona-as-lived**:
conversations had, decisions made, memories captured. It does **not** hold
**persona-as-authored** — the definition (name, voice, constraints) stays in
the Dominic static registry, version-controlled in TOML.

> The authored form is the *contract*; the lived form is the *record*.
> Confusing the two is how personas drift silently.

### 2. Classification: shared, membership-gated (no new machinery)

`persona:*` namespaces are ordinary **shared namespaces** under the
namespace-membership ADR's resolve_ns check order (class 4): reads and writes
require store membership (or admin). Consequences, all reusing existing
machinery:

- the **Tsume service principal** is granted membership per persona at deploy
  time (`POST /namespaces/grant`) — persona grants join the runbook's
  provisioning list;
- **promotion into a persona namespace** flows the closed-tunnel rule
  (member or admin; staging rejected as a target) — a promoted memory is
  curated lived-experience, and only members curate;
- grants are auditable (principal × namespace × granted-by × when) — persona
  custody is answerable.

### 3. Provenance and the promotion ladder

Entries arrive at the bottom of the provenance tier: Tsume's IjimaEmitter
writes `source: AutoCapture`, `harness: tsume-{network}`,
`session_id: {identity}@{conversation}` — every lived entry traceable to the
conversation that produced it (the mandatory-provenance rule). Promotion
(`POST /memories/{id}/promote`) is the ratchet from lived to curated, and
composes with the provenance-tier model's origin work. This is the same
three-state ladder Lonis PR #21 proposes for discovery catalogs — evidence
based promotion is an ecosystem pattern, not a coincidence.

### 4. Imports enter through staging

Bulk persona imports (Sara: OpenClaw/ZeroClaw stores) land in
`ns_import_*` staging (open by design), then promote into `persona:sara`
through the gated target. Foreign provenance is preserved in content/source;
staging content is unverified until promoted — the wall that matters is
promotion, per the membership ADR.

### 5. Drift diagnostics compare the two forms

Persona drift = divergence between as-authored (Dominic registry) and
as-lived (`persona:{id}`). The diagnostics loop: periodic comparison of lived
entries against the registry's constraints/voice, flagging drift, feeding the
context-poisoning protection discovery's doctrine-health machinery. The
static registry is the **audit baseline**; the namespace is the field under
audit.

## Consequences

- Deploy provisioning gains: Tsume principal membership on every
  `persona:{id}` it serves (one grant per persona, not per conversation).
- Tsume daemon config: `mirror_path` era config carries the emitter's
  `namespace = "persona:{id}"` per persona served (Plan 10's builder).
- `persona:{id}` names must be valid namespace slugs (URL-safe; the emitter
  appends the namespace raw to the query string — Plan 10's documented
  constraint).
- 0.3 satellites export persona namespaces in checkpoints like any shared
  namespace; no special-casing.

## Cross-references

- [Namespace membership](namespace-membership.md) — the gate this rides
- [Provenance-tier model](provenance-tier-model.md) — the ladder this climbs
- Dominic D-09 (Persona + PersonaRegistry::load_dir) — the authored form
- Tsume plans 09/10 — the wearer and the writer
- `docs/discovery/context-poisoning-protection.md` — drift diagnostics' home
- Lonis PR #21 (discovery-substrate recommendations) — the parallel ladder
