# ADR: ijima-miner architecture

**Status:** Accepted (2026-07-08)
**Scope:** Phase 4 — the mining engine (rules tier → Proserpina llm tier →
review-queue/promote pipeline).

Ijima's novel capability: raw session transcripts (the ore) are mined into
curated memory-palace entries with full provenance (the refined metal). This
ADR fixes the architecture before pipeline code, because the review-queue and
provenance choices are expensive to reverse once the Store trait surface and
capabilities are committed.

## M1 — Extraction is pure; storage is a separate ingest step

`mine(turns) -> Vec<Extraction>` is **synchronous and side-effect-free**: it
only extracts. A separate async step (`ingest_extractions`) writes results to
the store. Rationale: the extraction tiers (rules, llm) are pure functions of
the turn text; storage is an async store concern. This keeps the miner crate
free of async/store coupling and makes tiers independently testable with
`EchoAgent`-style deterministic inputs.

## M2 — Review queue is per-namespace

Mining reads a principal's session turns (personal namespace) and produces
extractions. **PendingReview extractions stage in a per-namespace review
queue** (`mining_queue` table), not as Memory records. Rationale: keeps the
palace (`store_memory`) clean of unreviewed entries, and matches Ijima's
multi-tenant isolation (you review what was mined from your own sessions).
`mining:review` grants review rights within the reviewer's effective namespace.
A future "team review" mode may promote cross-namespace; out of scope for v0.

## M3 — Auto extractions go straight to the palace; PendingReview to the queue

- `Extraction::Auto(memory)` → `store_memory(ns, memory)` immediately
  (MemorySource::Mined). Content-hash dedup applies (a re-mine of the same
  fact is a no-op).
- `Extraction::PendingReview(memory)` → insert into `mining_queue`.
- `Extraction::Nothing` → discarded.
- Review: `list_pending(ns)`, `accept(ns, id)` → store_memory + delete from
  queue, `reject(ns, id)` → delete from queue.

## M4 — v0 is a single synchronous mining pass; no re-entrant locking

A `mining:trigger` request mines a session's new turns and returns. No
concurrent miners, no session-range locking in v0. The re-entrant locking
described in DESIGN.md ("a mining pass locks a session-range, not the whole
store; two miners run on disjoint sessions") is a **v1+ concern** that lands
when mining is long-running enough to need concurrency. Don't over-engineer v0.

## M5 — Single-shot extraction per role; no panel cross-examination

Each extraction role (decision-extractor, fact-extractor, reference-extractor)
is **one Proserpina persona doing one `respond` pass** (single-shot per session
window, per DESIGN.md). Proserpina's panel/cross-examination model is deferred
until review queues need it. The rules tier needs no persona at all.

## M6 — Provenance via existing Memory fields; confidence is queue-routing only

Provenance is the existing `Memory.session_id` + `MemorySource::Mined` (no new
fields). Confidence (0.0–1.0) **determines Auto vs PendingReview routing** but
is not stored as a Memory field in v0 — it is captured on the queued record for
the reviewer's context. A future `mining_pass` provenance table can add
(turn-range, pass-id, model) if audit needs grow.

## M7 — Tiers compose: rules always runs; llm runs when feature-enabled

`mine()` runs the **rules tier unconditionally** (cheap, no model), then the
**llm tier when the `llm` feature is on**, and merges results. Merged
extractions are content-deduplicated before ingest (reuse the content-hash
concept) so the two tiers don't double-report the same fact.

## M8 — Extraction roles (what each tier looks for)

| Role | Signal | Default tier | Default route |
|---|---|---|---|
| Decision | "we decided X", "let's go with Y", "agreed on Z" | rules | Auto |
| Fact | a stated technical fact (heavier; needs LLM judgment) | llm | PendingReview |
| Reference | URL / link / citation | rules | Auto |
| Pattern | a repeated behaviour (needs cross-turn LLM analysis) | llm | PendingReview |

The rules tier ships Decision + Reference (deterministic). The llm tier adds
Fact + Pattern (model-judged). Both populate `project`/`topic` heuristically
(best-effort; the reviewer corrects on accept).

## Open questions deferred to v1+

- Re-entrant session-range locking (M4).
- Panel cross-examination for low-confidence extractions (M5).
- Team/global review queue across namespaces (M2).
- A `mining_pass` provenance table for full audit (M6).
- Importance auto-calibration from acceptance/rejection feedback.
