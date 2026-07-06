# Shared Kai Memory Service — Design (meeting-ready)

Status: **Design proposal for team brainstorm.** The 3a path: generalize the
MemPalace 4-layer scheme to a Postgres+pgvector service in the VPN, fronted by an
MCP so any harness can use it. This doc focuses on the **multi-tenancy/scoping
model** — the genuinely novel part with no prior art in pi-mempalace.

## 0. The core tension (read first)

MemPalace's retrieval-quality win (96.6% LongMemEval) comes from **"store
everything verbatim, make it findable — never let AI decide what to forget."**
That philosophy is *single-user*. Share one verbatim store across 8 people and
you immediately hit:

- **Privacy/consent** — does Christine consent to Justin reading her raw Claude chats?
- **Secrets/PII** — "store everything" ingests API keys and credentials verbatim.
- **Noise** — 8 people's streams drown each other's signal.
- **Currency** — Christine's worry: a doc written today is stale in 6 months.

**Resolution principle:** preserve verbatim storage *per individual*; make
**sharing a deliberate act.** This drives the entire model below.

## 1. Two orthogonal axes (minimal model)

A memory carries metadata on two axes. Keep it to two — defer finer-grained RBAC.

| Axis | Values | Meaning |
|---|---|---|
| **scope** (audience) | `personal` \| `team` | who can see it |
| **origin** (trust/provenance) | `auto` \| `explicit` \| `doctrine` | how it got here, how curated |

- **scope=personal** → default for auto-capture; only the author sees it. Each
  person keeps the full MemPalace "store everything" magic *individually*.
- **scope=team** → shared. Split by origin into two lived layers:
  - **team + explicit** = Christine's *live shared brain* (active collaboration,
    "here's what we decided"). Higher write rate, moderate trust.
  - **team + doctrine** = curated, PR-reviewed, mirrors the **3b seed pack**
    from the repo. Read-mostly. Examples: Organic Modular Design encoding,
    kai-shiki usage patterns, Kai vocabulary, architecture decisions. Low write
    rate, high trust.

So the **3b seed pack is not replaced by 3a — it becomes doctrine's ingest
source.** Doctrine is authored in Git (reviewed) and mirrored into the service;
it is never written directly to the service by an agent. Conflicts resolve in
Git, not in the memory store.

## 2. Promotion policy (the social/cultural lever)

Movement between scopes is the lever that shapes the shared brain:

- **personal → team (explicit):** an author deliberately shares (`memory_promote`).
  Runs a **redaction/scrub filter** at this boundary (secrets, PII). This is the
  one place filtering happens — never at auto-capture (which would violate
  "store everything").
- **team (explicit) → doctrine:** promotion via **PR against the repo seed pack**.
  Rhymes with the kai-shiki pattern (personal exploration → PR-reviewed shared
  surface). Doctrine is stable and curated.
- **doctrine → personal:** automatic at install/wake-up — everyone inherits it.

This means **doctrine is versioned, reviewable, and reversible** (Git history),
while the live team brain is fluid. That split directly answers Christine's
staleness fear: doctrine can be *invalidated* (`knowledge_invalidate` already
exists in pi-mempalace) or superseded via PR; live team memory just ages and gets
recency-ranked downward.

## 3. Multi-party content (meetings) — needs a decision

A meeting transcript is inherently multi-party; "whose scope?" is ambiguous.
Three options; **lean is the hybrid**:

- **(a) Per-attendee** — each gets their own auto-captured copy in personal scope.
  Simplest; preserves individual retrieval; duplicates content.
- **(b) Shared-once** — stored once in team scope attributed to the meeting.
  Efficient; but ownership/attribution is fuzzy.
- **(c) Hybrid (lean)** — auto-capture per-attendee into *personal*; a **curated
  summary** gets explicitly promoted to *team*. This already matches how the
  2026-06-26 meeting notes work: raw transcript per person (≈personal), Gemini
  summary = the promoted artifact (≈team). Low conceptual cost to the team.

## 4. Wake-up composition (L1) — where "shared brain" becomes real

pi-mempalace injects L0 identity + L1 essential story at session start. In a
multi-tenant service this composes:

```
L0  identity           (per-user, ~100 tokens)
L1a personal essentials (per-user, top-N by importance+recency)  ← individual brain
L1b team doctrine       (shared, the curated baseline)           ← shared baseline
L2  on-demand by project/topic
L3  deep semantic search (your visible scopes: personal + team)
```

**Christine's "shared Kai brain" = L1b (team doctrine), shared identically across
the team.** Justin's 6k-memory individuality = L1a, stays personal. The two
compose, they don't compete. This is the cleanest answer to "shared vs
individual" — *both, layered.*

## 5. Service surface (MCP) — harness-agnostic

Tools exposed to any MCP-aware harness (Pi, Claude Code, Codex, Cursor):

| Tool | Notes |
|---|---|
| `memory_save` | gains a `scope` param (default personal) |
| `memory_search` | searches caller's visible scopes (personal + team) |
| `memory_promote` | personal→team, runs redaction filter |
| `memory_recall` / `memory_graph` / `memory_tunnel` | carry over, scope-aware |
| `knowledge_add` / `knowledge_invalidate` | temporal facts; doctrine prefers these over free text |
| admin/ingest endpoint | doctrine from repo seed (CI-driven on merge) |

**Embedding:** the service owns the model and embeds centrally; clients send
text. Guarantees vector compatibility. In-VPN latency is negligible. **Decision:
pick ONE embedder for the service and forbid local embedding against the shared
DB.** (MiniLM-L6-v2 is the safe default to match pi-mempalace; an upgrade is a
one-time re-embed migration.)

## 6. The Claude Design/Desktop gap (flag, don't solve)

Quinn/Christine/Tove live in Claude Design + Desktop, which are **walled gardens
that don't run custom MCPs.** They cannot read/write the shared memory from
inside those tools directly. Options to raise (not decide now):

- A **lightweight web UI** for the service (browse/promote/search without an
  MCP-aware harness) — gives designers a seat.
- Designers consume **doctrine read-only** via a generated doc (Confluence page
  or a synced file) until/unless their harness opens up.
- Accept the gap; designers collaborate via the *live team brain* that
  engineers/members write into on their behalf.

This gap is a direct consequence of Shape D's "no Anthropic-only path" rule and
is worth naming explicitly in the meeting so no one assumes designers are
included by default.

## 7. Engineering shape (for sizing, not committing)

- **Store:** Postgres + pgvector (replaces SQLite+sqlite-vec). Schema/logic port
  from pi-mempalace is mechanical; adding `author`/`scope`/`origin` columns is
  the new schema work.
- **Embedder:** central, served once (ONNX or a small GPU box in-VPN).
- **Front:** MCP server (TypeScript) + optional minimal web UI.
- **Auth:** in-VPN, tie to Kai SSO if one exists; per-user credentials.
- **Doctrine ingest:** CI job — on merge to the seed pack, upsert doctrine
  memories into the service.

Natural ownership: **Greg** (data engineer) for the Postgres/store, **Mikhail**
(prompt-integration engineer) for the MCP surface, **Justin** for the
pi-mempalace lineage and doctrine mapping. A real three-person build.

## 8. Decisions to bring to the meeting

1. **Multi-party handling (§3):** per-attendee / shared-once / **hybrid (lean)**?
2. **Scope model (§1):** is two axes (scope + origin) the right minimal model,
   or does the team want finer visibility (e.g., per-project)?
3. **Redaction at promotion (§2):** confirm the boundary — scrub at
   personal→team, never at auto-capture. (Security sign-off needed.)
4. **Embedder (§5):** MiniLM-L6-v2 default vs upgrade now; central-only.
5. **Claude Design gap (§6):** web UI vs read-only doctrine doc vs accept.
6. **Ownership (§7):** Greg/Mikhail/Justin split — confirm and staff.
7. **Cadence:** doctrine review cadence (stale-doctrine flagging) — Christine's
   currency concern needs a social process, not just a feature.

## 9. What I'm explicitly deferring (Organic Modular Design)

- Fine-grained RBAC / per-project visibility rules (YAGNI until pain).
- Automated summarization/compression (MemPalace skips AAAA compression for
  accuracy — same call).
- Conflict resolution beyond Git (doctrine resolves in Git; live team memory
  doesn't conflict, it just recency-decays).
- Public/beyond-VPN access.
