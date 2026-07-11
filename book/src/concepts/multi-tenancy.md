# Multi-Tenancy & Provenance

## Namespaces

Every request is scoped to a namespace — the isolation unit:

- **Private** — an operator's personal memory (`ns_<principal>_private`).
- **Shared** — project/team memory visible to a group.
- **Global** — the legacy pi-mempalace "everyone sees everything" commons.

Cross-principal personal isolation is enforced at the API layer: a
principal can read their own private namespace and shared/global, never
another operator's private store.

## Promotion — the single redaction boundary

Personal → shared promotion runs a [redaction filter](https://github.com/Industrial-Algebra/Ijima/blob/develop/ijima-server/src/redaction.rs)
at the boundary — the *one* place content filtering happens. Personal
storage is always verbatim. Promotion is gated by the `trust:promote`
capability: raising trust is deliberately more privileged than writing at a
tier.

## Provenance & trust tiers

Every `Memory` carries provenance fields:

| Field | Meaning |
|---|---|
| `source` | Trust tier: `Explicit` / `AutoCapture` / `Mined` / `Doctrine`. |
| `harness` | Which harness wrote it. |
| `session_id` | The originating session. |
| `origin` | The authoring instance (for federation). |
| `authority` | The source-of-truth scope for the record's domain. |

`MemorySource::trust_grade()` maps each tier to a Schubert codimension —
the quantitative trust axis. `Doctrine` is the highest grade (Git-versioned,
PR-reviewed, curated); `AutoCapture` the lowest. This is the foundation for
federation cross-talk policies and context-poisoning protection — see the
[provenance-tier ADR](https://github.com/Industrial-Algebra/Ijima/blob/develop/docs/adr/provenance-tier-model.md).
