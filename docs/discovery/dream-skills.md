# Dream-Skills: Replay-Based Skill Discovery from the Memory Substrate

**Status:** discovery note — 0.4 design input. Not scheduled work.
**Date:** 2026-09-21
**Sources:** Dream-RSI (Zheng et al., Google/DeepMind/UMD/UVa, 2026 —
[dream-rsi.com](https://dream-rsi.com), local copy `~/working/papers/dream-rsi.pdf`);
the report corpus' three documented organic-learning events; Lonis `v0.2.0`
(`Block<P>` seed corpus, replay provenance); Dominic `0.5` (pinned federation
wire fixtures, the Ijima HTTP FederationClient — PR #22).

---

## 1. The source idea

Dream-RSI improves *exploration policies* — executable code that decides where
a coding agent branches, what runs in parallel, and when to stop — through a
three-stage loop:

1. **Online explore** — the current policy drives discovery, recording every
   attempt and its outcome in a **discovery tree** (workspace snapshot,
   artifact, diagnostics, score `s_v` per node).
2. **Construct replay simulator** — completed trees become replay worlds:
   alternative policies can re-traverse recorded branches, revealing stored
   outcomes, at **zero execution cost**.
3. **Dreaming-based policy improvement** — a policy-development agent drafts
   many candidate policy revisions, *replays* each against every recorded
   tree, scores them (best-discovery − execution-cost + parallelism), and
   deploys the winner. The loop recurses: better policy → richer trees →
   better simulator → better policy.

Two properties matter for us: the policy is **code** (prompts, skills,
orchestration — not weights), and the meta-level improvement bottleneck is
**delayed, expensive feedback** — the paper's diagnosis is that policy
improvement cycles gate on long online rollouts.

We lived that bottleneck manually: the contemplation-prompt tone doctrine took
three operator-review cycles over four days to converge. The prompts are our
exploration policy; the operator was the expensive meta-feedback channel;
nobody was dreaming.

## 2. What the IA stack already covers

| Dream-RSI component | IA analogue | State |
|---|---|---|
| Discovery tree with recorded outcomes | report corpus + memory walls + git history + prediction-scoring events | have, as **prose with provenance** — not structured attempts→scores |
| Dreaming pass over accumulated history | the 08:45 contemplation (retrieve, reflect, schedule falsifiable self-tests) | have, daily |
| Policy-as-code improving from experience | prompts + skills + AGENTS.md | have the artifact; the improvement loop is operator-in-the-loop only |
| Replay simulator (cheap off-policy evaluation) | `memory_search` + corpus greps | partial — retrieval is cheap; no replay semantics |
| Policy-development agent | agent sessions + operator review | **the gap** |

The corpus' three organic-learning events are prose-scored precedents: the
arXiv cadence model (prediction scheduled, blinded, vindicated in the
recovered backlog), the solver-factory seam (transfer priced at one grep),
the Thatch cross-repo sprint (probe → founding doc → executed → reported, 27
days, full provenance). Each was a skill that should have been *recorded as
one* at the moment it was learned. None was.

## 3. The three-repo convergence (verified 2026-09-21)

The structured-outcome gap has a partial answer already built:

- **Lonis `Block<P>`** carries the missing `s_v`. The seed corpus includes
  `Result` ("what happened when work ran"), `Outcome` ("a structured domain
  outcome or error"), `Evidence` ("an observed fact"), plus `Decision`,
  `Assumption`, `Plan` ("an ordered replayable plan"). Every Block carries
  `ReplayProvenance`: tool version, compatibility, content hashing,
  `verify_replay → ReplayStatus`. Replay is in the type system, not the prose.
- **Dominic `0.5`** landed the delivery seam: pinned federation wire
  fixtures ("the wire shape is not invented here") and the Ijima HTTP
  FederationClient — the orchestrator speaks to the memory plane.
- **Ijima** holds the walls the outcomes land in — and 0.3.1's declarative
  `.pi/ijima.json` namespaces answer the routing question this loop didn't
  know it would be asked (an org session's Blocks belong in that org's wall).

```
Lonis tools emit typed Result/Outcome/Evidence Blocks
  → Dominic routes work + carries the control wire
    → Block↔memory promotion boundary (0.4 roadmap item)
      → Ijima walls hold scored, content-hashed, replayable outcome trails
        → the dreaming pass reads attempts-with-outcomes, not prose
          → dreamed skills, evidence-cited, PR'd to ia-toolkit
            → operator merges; fleet sessions deploy them; new work
              generates new Blocks — the recursion persists across sessions
```

The property the paper's simulator pool has only inside one experiment, ours
has across a company: **persistence**.

## 4. The adaptation: dreaming skills, not exploration policies

The thing improved is not "where does the coding agent branch next" — it is
**what the agents know how to do**: new `SKILL.md`s for ia-toolkit, doctrine
entries, prompt refinements. The loop:

1. **Mine** — a periodic dreaming pass reads the memory graph + corpus for
   recurring patterns of work or failure (skill-seeds: the env/token
   debugging saga, the switch-tree rollback lesson, the "three stores"
   thesis, duty-cycle governance).
2. **Draft** — candidate skills, each **evidence-cited** (memory ids, report
   titles, dates). No citation, no candidate.
3. **Replay** — the corpus *is* the replay simulator: "would this skill have
   applied?" is answerable read-only (grep for the failure pattern, count
   recurrences across reports, check overlap with existing skills, look for
   the counter-case). Zero execution; thousands of probes.
4. **Score** — applicability × recurrence × severity − overlap. Every term
   countable from the record; no invented outcomes.
5. **Propose** — best candidates land as PRs (ia-toolkit / doctrine).
6. **Deploy, gated** — **the operator merges.** A dreaming layer that writes
   its own skills unattended is the context-poisoning threat model wearing a
   friendly hat; the human gate is the defense. This is the circadian
   adjudication pattern (agent proposes overnight, operator disposes by
   morning) made structural.

### Block-kind → provenance-tier mapping (direction D convergence)

The promotion boundary ships evidence grading almost for free:

| Block kind | Evidence class | Rationale |
|---|---|---|
| `Evidence` | `observed` | an observed fact, content-hashed |
| `Result` (green `ReplayStatus`) | `observed` | verified execution outcome |
| `Outcome` | `observed` (success) / `interpreted` (error taxonomy) | domain outcome |
| `Decision` | `observed` (that it was made) | the *wisdom* stays `interpreted` |
| `Assumption`, `Summary` | `interpreted` | claims, not facts |

The two 0.4 design inputs (this doc and context-poisoning protection's
direction D) were always one item.

## 5. Honest gaps and constraints

1. **Adoption is the real "partial."** Blocks score outcomes only for work
   flowing through Lonis-hosted tools under Dominic routing; fleet pi
   sessions do not yet. The dreamer runs **hybrid** for a long time: Block
   trails where they exist, corpus-grep recurrence everywhere else. The
   prose corpus earned its three scored events without any Blocks — hybrid
   is proven ground, not a compromise.
2. **The promotion boundary is designed, not built** — which kinds earn
   promotion, tier mapping, namespace routing, retention.
3. **Skill deployment feedback is slow** — a merged skill proves itself over
   weeks of sessions, so the recursive loop runs at monthly cadence, not the
   paper's per-rollout cadence. Acceptable: so does evolution.
4. **Replay vs reminiscence** — without Block trails, "replay" over prose is
   recurrence analysis, not counterfactual traversal. The hybrid design
   should say so, always.

## 6. 0.4 unit candidates this doc contributes

- **Block↔memory promotion boundary** — deliver structured outcomes; ship
  the tier mapping as its first citizen (closes direction D's mechanism).
- **Skill-mining pass** — `ijima-miner`'s evolution: sessions → memories
  → recurring patterns → evidence-cited skill candidates → scored → PR'd.
  Scheduled (the report apparatus' eighth genre or a miner timer).
- **Evidence-grade doctrine** — the `observed`/`interpreted` markers, cite
  requirements, retrieval-side weighting (direction D proper).

These three are one arc: **the memory substrate starts returning capability,
not just context.**

## 7. References

- Dream-RSI: *Recursive Self-Improvement through Evolving Worlds* —
  https://dream-rsi.com (paper PDF on file).
- `docs/discovery/context-poisoning-protection.md` — direction D, elevated
  to active 0.4 work (2026-09-13).
- Report corpus: `CONTEMPLATION_2026-09-1[1–21]` (organic events, tone
  doctrine convergence, duty-cycle governance), `RABBIT_HOLE_2026-09-14_Thatch`
  (cross-repo probe lifecycle).
- Lonis `v0.2.0`: `lonis-schema` Block kinds, `ReplayProvenance`,
  `verify_replay`. Dominic `0.5`: `dominic-ijima` FederationClient (PR #22),
  pinned wire fixtures (PR #21).
- Kagome `docs/research/solver-factory-seam.md` — the physics-domain cousin
  of this pattern (search architecture as the transferable).
