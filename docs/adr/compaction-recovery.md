# ADR: compaction-context recovery

**Status:** Proposed (2026-07-10)
**Scope:** How Ijima helps an agent reconstruct working context after its
own context window is compacted (summarized), and what Ijima's
responsibility is vs. the harness's.

## Context

Agent contexts get compacted — the running conversation is summarized to
fit the model window (this very session opened from a compacted summary).
Compaction is **lossy**: detail, exact phrasing, and ordering degrade. But
Ijima holds the **lossless** record of the same work: raw session turns
(append-only, high-fidelity) and the curated, mined memory palace +
knowledge graph. Per DESIGN.md D11, *text + KG is the lossless source of
truth; the compacted context is the lossy working copy.*

The question: can Ijima reconstruct a compacted context — and how practical
is that?

## The key distinction: replay vs. recovery

**Full lossless replay is a trap.** Compaction happens *because* the turns
don't fit the window. Replaying all of them to rebuild context just
re-triggers the constraint that forced compaction. "Reconstruct the exact
pre-compaction state" is the wrong goal — it's self-defeating as the sole
method.

**Targeted recovery is practical**, and it is ~80% built. `search_memories`
+ `wakeup` (L0 identity + L1a personal essentials + L1b doctrine) already
pull the high-signal subset. The post-compaction move is a *composition*:
[latest compaction summary] + [turns since compaction] + [semantic search
for the current task].

## Decision

Ijima's role is scoped to **making compaction recovery a one-call,
high-signal operation** — not to reconstructing exact pre-compaction state.
The harness owns the trigger (detecting it was compacted and pulling);
Ijima owns the pull being cheap and relevant.

### C1 — Store compaction summaries as retrievable artifacts

Add a `MemorySource::Compaction` variant (or a session-range marker) so the
agent can fetch *"the state as of the last compaction"* — a cheap anchor
instead of replaying from zero. The summary is itself a memory: searchable,
ranked, provenance-tagged to the session + compaction point.

### C2 — A recovery operation tuned for the post-compaction moment

Distinct from generic `wakeup`. The contract:

```
recover(session_id, current_task) ->
  { compaction_summary, turns_since_compaction, search_hits_for_task }
```

It is a **composition of existing Store methods**, not new storage:
- latest `MemorySource::Compaction` for the session (C1)
- `session_turns` since that compaction's timestamp
- `search_memories(ns, embed(current_task), k)` across the project

### C3 — Identity via the Context Mapper

`recover` needs to know *which project* it is recovering for. A compacted
session's CWD resolves to a canonical project via the Context Mapper
(`resolve_path`), which then scopes the search to that project's memories.
The Context Mapper (ADR-separate) is the identity layer recovery depends
on — this is why it is 0.1.0 priority.

### C4 — The bottleneck is agent-side discipline, not Ijima storage

Ijima can serve a perfect recovery payload, but it only helps if the
*harness* (Pi/Wallace/OpenCode) detects compaction and calls `recover`.
That is the harness's responsibility. Ijima must not assume passive
reliance on the compacted summary; it makes the pull cheap and high-signal
so the harness *wants* to call it.

## What is NOT decided here

- The exact harness-integration contract (when/how Pi/Wallace/OpenCode call
  `recover`). That is a harness-side ADR per harness.
- Whether `MemorySource::Compaction` is a new variant or a session-range
  marker — deferred to the implementation PR (a variant is simpler; a
  marker preserves the Mined/Explicit taxonomy).
- Re-embedding the compaction summary vs. treating it as un-embedded text.
  Likely embedded (so it's searchable), but confirm at implementation.

## Practicality verdict

| Goal | Practical? | Mechanism |
|---|---|---|
| Exact pre-compaction replay | No (self-defeating) | — |
| Lossy-but-good re-inflation | **Yes** | `recover` composition (C2) |
| Knowing *what* to recover | Yes | Context Mapper (C3) |
| Triggering recovery | Harness's job (C4) | per-harness ADR |

## Open questions

- How does a harness reliably *detect* it was compacted? (Provider-specific
  signal — Claude compaction vs. Gemini context vs. custom.) May need a
  harness-reported flag on the session.
- Should `recover` be pull-only, or should Ijima *push* a recovery hint
  when a session with a recent compaction is resumed? Push adds a daemon
  concern; pull keeps Ijima stateless per-request.
