---
name: ijima
description: "Mental model and diagnostics for the Ijima central memory service (the memory_search / memory_save / knowledge_* tools). Use BEFORE concluding the memory store is empty, misconfigured, or pointing at the wrong data — most 'empty brain' reports are namespace scoping, not missing data. Also use when saving memories (provenance, dedup, project/topic fields) or when ijima tools error on env, tokens, or capabilities. Triggers: ijima, memory search empty, brain is empty, no memories, memory palace, knowledge graph, namespace, scope visible, IJIMA_TOKEN, IJIMA_URL, memory_save, token capability."
---

# Ijima — the central memory brain

The `memory_*` / `knowledge_*` tools in your toolset are a thin client over
an Ijima daemon running elsewhere. All memories live **server-side**, in
**namespaces**. Your session contributes almost nothing at startup — so an
"empty" first result is usually correct scoping, not a broken store.

## The namespace model (read this before diagnosing)

| Namespace | What lives there | Who can read it |
|---|---|---|
| `ns_<your-principal>_private` | Your saves (auto-created on first write) | Only you |
| `global` | The reviewed commons | Everyone |
| `ns_doctrine` | Curated doctrine entries | Everyone |
| `ns_import_<source>` | **Bulk-imported legacy corpora** — this is where most memories usually live | Everyone (staging) |
| `ns_<org>_shared` | Org/team walls (e.g. company projects) | **Members only** |

`GET /wakeup` tells you your principal identity. There is **no global
"list everything" for agents** — that census is admin-only.

## Why your probes look empty (in observed order of likelihood)

1. **Personal-scope probes are genuinely empty.** `/memories/stats` with no
   namespace = your private namespace. Fresh principals have zero. Check
   `global` and — more importantly — actually **run `memory_search`**.
2. **`memory_search` uses `scope=visible`**: your private + `global` +
   every `ns_import_*` staging + org walls you hold membership in. That is
   the real test of "is the brain there". One empty search ≠ empty brain —
   try several different queries first.
3. **Browsing a namespace that never existed returns an empty list**,
   indistinguishable from an empty one. Verify names from the table above;
   imports are named after their *source host/system*, not generic words.
4. **`/repos` failing with "table 'repo_directory' does not exist"** is a
   known cosmetic bug on **every** store, populated or not. It is NOT a
   fresh-store fingerprint. Ignore it.
5. **Empty `scope=visible` searches on a populated daemon** → the daemon is
   older than v0.2.2 (visible used to span private + global only). Ask the
   operator to check the daemon version; do not repoint data dirs.

**Never conclude "wrong data directory / virgin store" from agent-side
probes alone.** That determination requires the admin census
(`GET /status`, admin token) on the daemon host.

## Diagnostics ladder (cheapest first)

1. `curl $IJIMA_URL/health` → `{"status":"ok"}` means the daemon is up.
2. Missing `IJIMA_URL`/`IJIMA_TOKEN` in env? Tools fail fast with setup
   text — the error names the exact fix. (Common cause: spawned from a
   shell that didn't source the env — interactive shell, not login.)
3. Run `memory_search` with 2–3 distinct, plausible queries.
4. If still empty: report "visible-scope search returns empty" and stop —
   an operator with an admin token can run the census. Don't speculate
   about server-side state you cannot see.

## Saving memories

- `memory_save` is content-addressed: identical content dedups (a
  duplicate check tool exists — use it to test, not to gate saves).
- Fill `project` and `topic` — they drive palace rooms and taxonomy.
- Agent saves land at the **AutoCapture trust tier** (unverified). The
  path to higher trust is explicit promotion by an operator, not re-saving.
- Structured facts (`X depends_on Y`, temporal ranges) belong in the
  knowledge graph (`knowledge_add`), not free-text memories.

## Token capabilities

One grant carries all four capabilities the tools need:
`memory:read,memory:write,knowledge:read,knowledge:write`. A tool error
naming a missing capability includes the exact `ijima token issue`
invocation that fixes it. 429s are retried with backoff automatically —
you never need to retry yourself.
