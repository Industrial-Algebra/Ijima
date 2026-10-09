# Corrections: supersede

*Since 0.4.0.*

The memory palace treats correction as a first-class structural
operation, not a ranking fight. Before 0.4, a wrong high-importance
memory outranked every lower-tier correction forever — wake-up orders
lexicographically (`importance DESC, created_at DESC`), so recency
never closed an importance gap, and corrections only *worked* when a
human re-saved them at matching importance to win the recency
tiebreak. The 2026-09-22 research dive called this "a ritual, not a
mechanism." Supersede is the mechanism.

## The link

Saving a memory that declares `supersedes: <target_id>`:

- writes the inverse link (`superseded_by`, `superseded_at_unix`) onto
  the target as part of the save — serialized and
  failure-compensated (see below);
- **excludes the target** from wake-up, search, and browse —
  regardless of either row's importance, tier, or age;
- **keeps the target recallable by id**, with its links intact —
  superseded rows are visible fossils, not deletions.

```json
POST /memories
{
  "id": "mem_correction",
  "content": "The wire shape was fixed in commit abc123 — earlier notes were wrong.",
  "supersedes": "mem_wrong_note",
  "evidence": "Observed",
  "citations": [{ "kind": "Commit", "locator": "abc123" }],
  ...full Memory fields...
}
```

## Semantics

- **Chains, not re-targeting**: a memory that is already superseded
  cannot be superseded again — supersede the *successor*. `A ← B ← C`
  is a valid chain; `A ← B` then `A ← C` is rejected (409).
- **Self-supersede** is rejected (409); a target absent from the
  namespace is 404.
- **Namespace-scoped**: the link resolution never crosses walls.
- **Concurrency-safe (the precise guarantee)**: a supersede save is
  *insert → conditional claim → compensate*, serialized per process —
  not a single transaction. The claim on the target is one conditional
  atomic update (`UPDATE … WHERE superseded_by = NONE`), so concurrent
  successors of one target yield exactly one winner; a claim that
  fails (absent, claimed, or self target) compensates by deleting the
  just-inserted successor, leaving the pre-attempt graph. Mutually
  referencing saves see the serial outcome — both rejected and
  compensated away — instead of forming a correction cycle. The
  correction graph is acyclic by construction under single-process
  operation; cross-process serialization is federation's problem.

## Where exclusion applies

| Path | Superseded rows |
|---|---|
| Wake-up (personal essentials) | excluded |
| Semantic search | excluded |
| Browse / project-topic listing | excluded |
| Recall by id | **included** (with links) |
| Content dedup (`/memories/check`, save-path) | excluded — a retired row's content does not block a fresh save |
| Doctrine cross-id dedup | never canonical; stale duplicates retire via supersede links |

## Corrections and evidence grades

A correction that merely asserts a different claim is an
*interpretation*. A correction that cites what it observed
(`"Observed"` + citations — see [provenance](provenance.md)) carries
the evidence axis with it. The two mechanisms compose: the supersede
link decides *what is visible*; the evidence grade declares *how the
replacement knows*.

## Doctrine is versioned, not superseded

The doctrine tier (Git-reviewed, ingested from corpus) corrects itself
through [versioning and rollback](../api/overview.md), not supersede
links — its rows carry stable revision identities instead. The two
correction systems are tier-appropriate: agent memory displaces;
doctrine re-versions.
