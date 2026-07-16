# pi extension tool-serialization across provider adapters

> **Status:** In-progress investigation (2026-07). Triggered by a tool-call
> loop after switching pi's default provider to Z.ai's **Anthropic-compatible**
> endpoint (`api.z.ai/api/anthropic`, `api: "anthropic-messages"`). This doc
> will be refined once the working compat-flag combination is confirmed.
>
> **Why this matters for Ijima:** the `ijima-pi` crate registers its pi-facing
> tools the **same way `pi-mempalace` does** — it is the successor adapter (a
> WASM serde shim mapping Ijima's REST API to pi's tool surface). Anything that
> breaks pi-mempalace's tools under a given provider adapter will break
> `ijima-pi`'s tools the same way. This is direct design input for the
> [`ijima-pi`](../../ijima-pi) integration.

## Symptom

After switching the default provider (`settings.json`: `defaultProvider: "zai"`,
`defaultModel: "glm-5.2"`) from Z.ai's OpenAI endpoint to its
Anthropic-compatible endpoint, **tool-calling broke** — but selectively:

| Tool class | openai-completions | anthropic-messages (Z.ai facade) |
|---|---|---|
| **Core / built-in** (`bash`, `read`, `edit`, `write`) | ✅ works | ✅ works |
| **Extension-registered** (`memory_save`, `memory_search`, `knowledge_add`, … from `npm:pi-mempalace`) | ✅ works | ❌ **fails** — model emits repeated `bash echo` instead of the intended extension-tool call (a tight loop) |

The failure is reproducible across a full pi restart (fresh model context), so
it is config/serialization-driven, not stale-session state.

## Hypothesis

The **extension tool registration/serialization path differs from the built-in
tool path**, and is sensitive to the adapter × facade combination. Built-in
tools are hardened across adapters; extension-registered tools use a different
schema-registration path that Z.ai's Anthropic facade mishandles — producing a
model-visible tool set where extension tools are unreachable, so the model
falls back to the most reliable tool it *can* reach (`bash`), in a loop.

This is **not** broad context corruption (which would degrade `bash` too); the
selectivity (extension tools only) points at tool-*definition* handling.

## Investigation log — compat flags

The levers are pi's `anthropic-messages` compat flags (see pi
`docs/models.md` → "Anthropic Messages Compatibility"), which all default to
`true` (i.e., assume real Anthropic). The `zai` provider block in
`~/.pi/agent/models.json` was half-migrated: `api`/`baseUrl` switched to
Anthropic, but the `compat` block still held stale OpenAI-completions fields
(`supportsDeveloperRole`, `thinkingFormat: "zai"`, `supportsReasoningEffort`)
and **none** of the Anthropic-messages flags.

| Option | Flag(s) | Result |
|---|---|---|
| Cleanup | drop the 3 stale OpenAI-compat fields | done (housekeeping) |
| **A** | `supportsEagerToolInputStreaming: false` | ❌ **did not fix** the loop |
| **C** | `supportsCacheControlOnTools: false` (drop `cache_control` from tool defs) | ⏳ **testing** — best match for the tool-specific symptom |
| **B** (reserve) | `allowEmptySignature: true` + `forceAdaptiveThinking: true` (thinking-replay) | held; less precise match (would affect all tool state, not just extension tools) |

**Caching note:** `supportsCacheControlOnTools: false` only drops `cache_control`
from *tool definitions*. The system-prompt caching (the bulk of the cache win
that motivated the Anthropic switch) is preserved — so this flag is safe from a
quota perspective.

## If C and B both fail

We are past the documented flags. Options narrow to:

- Tool-heavy sessions on the OpenAI endpoint while keeping Anthropic for
  caching-sensitive work (a per-session tradeoff).
- Investigate pi's extension-tool wire-format for `anthropic-messages` directly
  (how `npm:`-registered tools are serialized vs built-ins).

## Action items for `ijima-pi`

These follow from the hypothesis regardless of which flag turns out to fix it:

1. **Test the full `ijima-pi` tool set against the production adapter**
   (`anthropic-messages` / Z.ai facade), not just `openai-completions`. Do not
   assume a tool definition that works under one adapter round-trips under the
   other — the extension path is the fragile one.
2. **Record the required compat-flag combination** (once confirmed) in the
   [`ijima-pi` integration plan](../plans/2026-07-12-pi-integration.md) and in
   any pi-session `models.json` that consumes Ijima. Treat it as a hard
   requirement for Ijima-on-Anthropic deployments.
3. **Consider shaping `ijima-pi` tool schemas to avoid fragile features** —
   e.g., if `cache_control` on tools is the breakage, prefer definitions that do
   not require it, so the Ijima tools are robust across adapters by construction.
4. **Cross-link this finding** from the integration plan so it isn't missed when
   `ijima-pi` tool schemas are finalized.

## Cross-references

- pi `docs/models.md` → "Anthropic Messages Compatibility" (the compat-flag reference).
- [`docs/plans/2026-07-12-pi-integration.md`](../plans/2026-07-12-pi-integration.md) — the `ijima-pi` integration plan.
- [`ijima-pi`](../../ijima-pi) — the successor adapter (WASM serde shim; same tool surface as pi-mempalace).
- `pi-configs` repo (`justinelliottcobb/pi-configs`) — the live `models.json` under test; history shows the cleanup + A + C changes.
