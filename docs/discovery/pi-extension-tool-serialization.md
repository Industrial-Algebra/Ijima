# pi extension tool-serialization across provider adapters

> **Status:** Investigation **closed** (worked around). Refocused as a design
> footnote for cross-stack **tool-call-loop protection** — the constructive
> takeaway for Ijima, Wallace, and Dominic.
>
> **Why this matters for Ijima:** the `ijima-pi` crate registers its pi-facing
> tools the **same way `pi-mempalace` does** (it is the successor adapter — a
> WASM serde shim mapping Ijima's REST API to pi's tool surface). Anything that
> makes pi-mempalace's tools unreachable under a provider adapter will make
> `ijima-pi`'s tools unreachable the same way — and `ijima-pi` is the natural
> place to build preflight protection (see §Engineering protection).

## Symptom

After switching the default provider (`settings.json`: `defaultProvider: "zai"`,
`defaultModel: "glm-5.2"`) from Z.ai's OpenAI endpoint to its
Anthropic-compatible endpoint, **tool-calling broke** — but selectively:

| Tool class | openai-completions | anthropic-messages (Z.ai facade) |
|---|---|---|
| **Core / built-in** (`bash`, `read`, `edit`, `write`) | ✅ works | ✅ works |
| **Extension-registered** (`memory_save`, `memory_search`, `knowledge_add`, … from `npm:pi-mempalace`) | ✅ works | ❌ **fails** — model emits repeated `bash echo` instead of the intended extension-tool call (a tight loop) |

The failure is reproducible across a full pi restart (fresh model context), so
it is config/serialization-driven, not stale-session state. The extension tool
schemas are **clean, valid JSON Schema** (plain `Type.Object` of string/number
properties — nothing Anthropic-incompatible).

## Attribution — uncertain (NOT confirmed as a pi bug)

> Earlier drafts of this doc asserted "it's a pi bug." That was **overstated**.
> The attribution is genuinely unresolved.

Two hypotheses fit the evidence equally, and were never disambiguated (no
outgoing-request inspection was performed):

1. **pi-side** — pi's extension-tool registration path doesn't (correctly)
   include extension tools in the `tools` array sent to the `anthropic-messages`
   endpoint, while built-ins are included.
2. **facade-side** — Z.ai's facade imposes a limit/quirk (e.g. a tool-count or
   payload threshold) that drops/mishandles tools registered *after* the
   built-ins, so built-ins survive and extensions vanish from the model's menu.

The tell that leans toward **facade-side**: other pi users report clean tool
calling on **real Anthropic**, which means pi's extension-tool serialization is
presumably correct for real Anthropic — making the facade the new variable. But
real Anthropic ≠ Z.ai facade, so this is not conclusive.

**The decider (never run):** inspect the outgoing `anthropic-messages` request
body — are the extension tools present in the `tools` array? Present-but-uncalled
→ facade/model-side; absent → pi-side.

## Resolution

The issue is **academic** — worked around by switching Z.ai back to its
**`openai-completions`** endpoint (`api.z.ai/api/coding/paas/v4`), which
serializes extension tools correctly *and* provides transparent server-side
prefix caching (~98% discount on stable prefixes ≥1024 tokens; short TTL). The
`anthropic-messages` detour was unnecessary; the compat-flag experiments below
are reverted. **Not filed upstream** — filing responsibly would require the
request inspection above, and the workaround removes the motivation. (If
revisited: inspect the request first, then file to `earendil-works/pi` if
pi-side, or to Z.ai if facade-side.)

### Investigation log (for the record)

All `anthropic-messages` compat flags (pi `docs/models.md` → "Anthropic Messages
Compatibility") were tried; none resolved the loop. The `zai` block had been
half-migrated (`api`/`baseUrl` switched to Anthropic, but the `compat` block
still held stale OpenAI-completions fields).

| Option | Flag(s) | Result |
|---|---|---|
| Cleanup | drop the 3 stale OpenAI-compat fields | done (housekeeping) |
| **A** | `supportsEagerToolInputStreaming: false` | ❌ did not fix |
| **C** | `supportsCacheControlOnTools: false` | ❌ did not fix |
| **B** | `allowEmptySignature: true` + `forceAdaptiveThinking: true` | ❌ did not fix |

Versions: reproduced on pi **0.80.8** and **0.80.9**.

---

## Engineering protection — catching tool-call loops across the stack

> This is the constructive takeaway. A tool-call loop is a **detectable
> signal**: the model repeatedly invokes a fallback tool (`bash`) when an
> intended tool is unreachable, making no progress. Detection can be built at
> several layers — no single layer catches everything, and the robust design
> uses multiple. This footnote records where each layer fits, so Ijima, Wallace,
> and Dominic can engineer defenses in as they're built.

### The signal

A tool-call loop has a recognizable signature:

- **Repeated fallback-tool calls** — N consecutive invocations of the same tool
  (e.g. `bash echo`) with no intervening model progress.
- **Intent ≠ action** — a stated intent to use tool X followed by repeated calls
  to tool Y ≠ X without success.
- **Negative write signal** — a long session that produces *zero* writes to the
  intended subsystem (e.g. no Ijima `memory_save`/`knowledge_add` calls when the
  session clearly intended to record things). Absence-of-expected-writes is a
  strong, cheap signal.

### Layer-by-layer defenses

**1. Ijima — session-context miner (strongest fit).** Ijima already ingests raw
session turns from every harness (the session-context repository). The miner can
detect loop signatures as an extraction pattern: repeated tool calls, intent≠action,
or the negative-write signal above. Action: flag the session, alert the operator,
and write the *insight* into the memory palace — e.g. "adapter X / facade Y makes
Ijima tools unreachable; use openai-completions." This turns a silent failure
into refined, searchable knowledge — exactly the session→palace mining loop Ijima
exists to perform, and it directly serves `ijima-pi` (it would catch `ijima-pi`
tools being unreachable).

**2. `ijima-pi` adapter — tool-readiness preflight (catch *before* the loop).**
`ijima-pi` registers tools; it can run a readiness probe at session start that
confirms its registered tools are actually present in the outgoing request's
`tools` array (or issues a probe call). If `ijima-pi`'s tools aren't reachable
under the configured adapter, **fail loudly at session start** rather than
silently degrading into a model loop. This is shift-left protection — detect the
broken adapter config before the model ever loops.

**3. Dominic — dispatch telemetry + loop detection (the protocol angle).**
Dominic can't see intra-session tool calls by default, but the **dispatch
contract can be extended to carry tool-call telemetry** — a `ToolCallEvent`
stream or summary metrics (turn count, repeated-tool-call count) in
`DispatchOutcome`. Dominic then detects loop signatures across dispatches and can
**retry with a different provider/adapter** (the actual fix — e.g. route to
`openai-completions`), alert, or abort cleanly.

This dovetails with the **Tsume dispatch-inspector observability gap** (Tsume's
TUI wants to see Dominic's routing decisions in real time): the *same* telemetry
stream that powers the inspector also powers loop detection — one protocol
addition serves both. Candidate `dominic-core` extension: a `DispatchTelemetry`
type + a loop detector behind the dispatch contract, alongside the existing
async `Dispatcher` trait. **This is where the debugging connects back to the
protocol design.**

**4. Wallace (when built) — policy / budget enforcement.** Wallace manages
agents; it can enforce per-agent/session **tool-call budgets** (max total calls,
max repeats of the same tool) and terminate or alert on loop exhaustion. It can
also maintain a **known-good provider/adapter × tool-set compatibility matrix**
and route agents away from broken combinations proactively — protecting the
multi-user environment from a loop-consuming agent.

**5. Tsume — operator alerting.** Tsume's TUI control plane (dispatch inspector,
session browser, status panel) surfaces loop detection to the operator in real
time — e.g. flash the status panel when an agent repeats a tool N times. Tsume
sees the message stream and is the operator's seat; it's the right place for
human-visible loop alerts.

### Cross-cutting theme

Each layer sees a **different slice** of a loop: pi sees raw tool calls, Ijima
sees session context, Dominic sees dispatch outcomes, Wallace sees agent
budgets, Tsume sees the message stream. Robust defense is **distributed**:
shift-left (preflight/readiness at `ijima-pi` and Wallace's compatibility
matrix) catches before the loop; telemetry/mining (Ijima miner, Dominic
detector, Tsume alerting) catches during/after. No single layer is sufficient;
together they make a silent, session-destroying loop into a loud, recoverable
event.

---

## Action items for `ijima-pi`

1. **Test the full `ijima-pi` tool set against the production adapter**
   (`anthropic-messages` facades included), not just `openai-completions`. Do
   not assume a tool definition that works under one adapter round-trips under
   another — the extension path is the fragile one.
2. **Build the tool-readiness preflight** (§Engineering protection §2) — confirm
   `ijima-pi`'s tools are in the outgoing `tools` array at session start; fail
   loudly if not.
3. **Wire loop-signature detection into the miner** (§1) — repeated-tool-call and
   negative-write-signal extraction, flagging sessions and mining the "adapter X
   breaks Ijima tools" insight into the palace.

## Cross-references

- pi `docs/models.md` → "Anthropic Messages Compatibility" (the compat-flag reference).
- [`docs/plans/2026-07-12-pi-integration.md`](../plans/2026-07-12-pi-integration.md) — the `ijima-pi` integration plan.
- [`ijima-pi`](../../ijima-pi) — the successor adapter (WASM serde shim; same tool surface as pi-mempalace).
- **Dominic** [`dominic-core`](../../Dominic/dominic-core) — the dispatch contract (`Dispatcher`/`DispatchOutcome`) that the telemetry + loop-detection extension (§3) would extend. Full-circle: this debugging informs Dominic's protocol design.
- **Wallace** (`../Wallace`) — when built, inherits the budget/compatibility-matrix defense (§4).
