# Ijima — Handoff Document

> **Purpose**: This document hands off the Ijima design work to a future
> session. Ijima is the centralized agentic memory backend for the IA
> ecosystem — the single memory layer serving Wallace, Sakamoto, Tsume,
> Dominic, and eventually Proserpina, with extensions for other harnesses
> like pi and opencode.
>
> **Session scope**: A future pi session starting in
> `~/working/industrial-algebra/Ijima/` should read this document first,
> then `../Tsume/docs/HANDOFF.md` and `../Dominic/docs/HANDOFF.md` for
> the consumer-side context, then begin design.

---

## 1. What Ijima Is

Ijima is the **centralized agentic memory backend** for the IA ecosystem.
It replaces the fragmented, per-harness memory stores with a single
authoritative memory service that every agent reads from and writes to.

```
                      Dominic
                   (meta-orchestrator)
                  /    |      |       \        \
            Wallace  Sakamoto  Tsume   opencode  pi
           (multi-user (pipelines (gateways   (CLI)  (deep work
            TUI)      coding)    adapter)       sessions)
                  \    |      |       /        /
                   \   |      |      /        /
                    \  |      |     /        /
                     ▼ ▼      ▼   ▼        ▼
                   ┌─────────────────────────────┐
                   │            Ijima            │
                   │   (centralized memory +     │
                   │    session context mining)  │
                   └─────────────────────────────┘
```

**Ijima's job**: be the single source of truth for agentic memory across
the entire stack. Every harness stores and retrieves memories, knowledge
graph facts, and session context through Ijima. No harness keeps its own
private memory island.

**Ijima is NOT**: a pi extension, a ZeroClaw plugin, or a per-harness
adapter. It is a **standalone service** (likely a daemon with an API) that
harnesses connect to. It is a full Rust IA-standard project. It is
generalized — not specific to the IA ecosystem — and should offer
extensions/adapters for any agentic harness (pi, opencode, Claude Code,
etc.).

### The Two-Store Model

Ijima serves two related but distinct stores:

1. **The Memory Palace** — long-term semantic memory. Verbatim storage +
   semantic search + knowledge graph. "What did we decide about X?"
   "Who depends on Y?" This is the pi-mempalace model, production-proven.

2. **The Session Context Repository** — raw session transcripts/context
   from every harness. Every conversation, every agent run, every pi
   session, every Discord exchange. Not curated, not summarized — the raw
   context streams. This is Ijima's novel feature (see §4).

The novel capability: **Ijima mines the session context repository to
extract pertinent information into the memory palace.** Sessions are the
raw ore; the memory palace is the refined metal. Ijima does the refining.

---

## 2. Why Ijima Exists (The Memory Pain Points)

The current state is fragmented memory across multiple disconnected
stores. Every pain point below is a real problem encountered in this
project history.

### 2.1 Memory Fragmentation Across Harnesses

**Pain point**: Memory is siloed per harness. pi has its mempalace
(`~/.pi/agent/memory/memories.db`). ZeroClaw has its own brain.db
(`~/.zeroclaw/workspace/memory/brain.db`, 1,447 entries). OpenClaw had
its own JSONL session files (~500MB). Sakamoto, Wallace, Tsume will each
have their own if we don't centralize.

When Sara (ZeroClaw) learns something in Discord, pi sessions can't see
it. When pi research discovers something, Sara doesn't know. Decisions
made in one context are invisible to the others.

**Ijima fixes this**: one memory service. Every harness writes to and
reads from the same store. A decision Sara archives from Discord is
immediately searchable by pi's next research session.

### 2.2 The Bridge-Script Anti-Pattern

**Pain point**: We built a Python bridge (`~/.zeroclaw/mempalace-bridge.py`)
to let ZeroClaw (Rust) access pi-mempalace (Node.js/SQLite). It was
never wired in properly. We built a Rust module
(`~/working/ai/zeroclaw-fork/src/mempalace.rs`) that reads the mempalace
SQLite directly. Both are fragile workarounds for what should be a
service.

Every harness shouldn't have to implement its own bridge to the memory
store. Bridges duplicate logic, drift out of sync with the schema, and
break when the store evolves.

**Ijima fixes this**: one stable API. Harnesses speak the Ijima protocol
(HTTP/gRPC/SQLite-over-network), not the raw database schema. Schema
changes happen in one place.

### 2.3 Context Loss Across Restarts

**Pain point**: ZeroClaw lost Discord conversation context on every
binary restart. We built a context-injection hook that read recent
conversation entries from the database. It caused recursive database
bloat (the hook's own reads got logged as new entries). The hook approach
is fundamentally fragile.

**Ijima fixes this**: session context is stored centrally, not in
per-harness ad-hoc tables. A harness restarts and asks Ijima "give me the
last N entries for session X" — Ijima returns them without the harness
having to manage its own context store. No custom hooks, no bloat loops.

### 2.4 No Cross-Session Mining

**Pain point**: Raw session context (Discord conversations, pi
transcripts, agent runs) is valuable ore, but it's never refined. A
decision buried in a 200-message Discord thread is lost unless someone
manually saves it. The mempalace only has what was explicitly archived.

**Ijima fixes this**: the session context repository + the mining
capability (see §4). Ijima periodically or on-demand analyzes session
transcripts and extracts decisions, facts, references, and patterns into
the curated memory palace. Nothing valuable stays buried in raw logs.

### 2.5 Duplicate/Near-Duplicate Memories

**Pain point**: The same fact gets saved multiple times across sessions
because there's no shared dedup. The mempalace has a `checkDuplicate`
function but it's per-write, not cross-session-aware.

**Ijima fixes this**: centralized dedup. Every write goes through Ijima,
which checks content hashes and semantic similarity against the entire
store before accepting.

---

## 3. Reusable Assets

### pi-mempalace (FORK — primary reference)

`~/working/industrial-algebra/pi-mempalace/`

A fork of pi-mempalace already adapted toward centralization. This is
the most important reference for Ijima — it's the working implementation
of the memory palace model plus a remote backend server.

**Key assets:**

- **`server/pi-mempalace-server.ts`** — an HTTP server that exposes the
  memory store over `/rpc` (JSON-RPC dispatch) and `/health`. This is
  already a centralized backend prototype. Supports bearer-token auth,
  configurable host/port/dir via env vars. **Ijima is the Rust
  successor to this server.**
- **`extensions/pi-mempalace/memory_store.ts`** — the full memory store
  implementation. Schema, store/search/recall/wakeup, knowledge graph
  (entities + triples), palace graph, tunnels, diaries, dedup. This is
  the spec Ijima implements in Rust.
- **`extensions/pi-mempalace/backend.ts`** — local vs remote backend
  abstraction. The pattern for how a client switches between embedded
  and remote modes.

**The RPC method surface** (from the server dispatch) is Ijima's API
contract:

```
store, search, wakeup, status, recall, computeStats,
getPalaceGraph, traverseTunnel, addTriple, queryEntity,
knowledgeStats, listRooms, getTaxonomy, delete, checkDuplicate,
findTriple, invalidateTriple, kgTimeline, diaryWrite, diaryRead
```

**Remote backend mode** (from the README): the fork already runs as a
central memory service on a single host, with pi clients connecting over
HTTP on a private network (Tailscale). Env vars: `PI_MEMPALACE_DIR`,
`PI_MEMPALACE_HOST`, `PI_MEMPALACE_PORT`, `PI_MEMPALACE_TOKEN`. Ijima
should preserve this deployment model and env-var convention (or define
its own, deliberately).

### The Current Schema (from the live DB)

`~/.pi/agent/memory/memories.db` — 4,033 memories, 73 triples, 83
entities. This is real production data Ijima must be able to import.

```sql
CREATE TABLE memories (
  rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  content TEXT NOT NULL,
  content_hash TEXT NOT NULL UNIQUE,
  project TEXT NOT NULL DEFAULT 'general',
  topic TEXT NOT NULL DEFAULT 'general',
  source TEXT NOT NULL DEFAULT 'auto-capture',
  timestamp TEXT NOT NULL,
  session_id TEXT NOT NULL DEFAULT '',
  importance REAL DEFAULT 0.5,
  chunk_index INTEGER DEFAULT 0,
  parent_id TEXT DEFAULT NULL
);

CREATE TABLE entities (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  entity_type TEXT DEFAULT 'unknown',
  properties TEXT DEFAULT '{}',
  created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE triples (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  subject TEXT NOT NULL,
  predicate TEXT NOT NULL,
  object TEXT NOT NULL,
  valid_from TEXT,
  valid_to TEXT,
  confidence REAL DEFAULT 1.0,
  source_memory_id TEXT,
  project TEXT DEFAULT 'general',
  created_at TEXT DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (subject) REFERENCES entities(id),
  FOREIGN KEY (object) REFERENCES entities(id)
);

-- Vector index via sqlite-vec
CREATE VIRTUAL TABLE vec_memories USING vec0(embedding float[384]);
```

Ijima should: (a) be able to import this schema and data directly, (b)
define a superset schema that adds session-context storage, (c) keep the
memory palace tables compatible enough for a clean migration.

### Embeddings

pi-mempalace uses `all-MiniLM-L6-v2` (384 dims) locally via
`@huggingface/transformers`, with `sqlite-vec` for vector search. Ijima
(Rust) options: `fastembed-rs` (ONNX, same model family), `candle`
(local, no Python), or an external embedding API. Decision for the
design session. Keep 384-dim default for migration compatibility.

### ZeroClaw's Rust mempalace module

`~/working/ai/zeroclaw-fork/src/mempalace.rs` — a Rust bridge that reads
the pi-mempalace SQLite directly. Provides `search`, `save_memory`,
`query_knowledge`, `list_projects`. This is a reference for the Rust
side of the API, but it's a direct-DB bridge (the anti-pattern Ijima
replaces), not a service client.

### Data to Migrate

- **pi-mempalace DB**: `~/.pi/agent/memory/memories.db` (4,033 memories,
  73 triples, 83 entities) — the primary corpus
- **ZeroClaw brain.db**: `~/.zeroclaw/workspace/memory/brain.db` (1,447
  entries) — Sara's Discord-sourced memories, to be merged
- **OpenClaw JSONL** (if recoverable): raw Discord session logs from the
  pre-ZeroClaw era — candidate material for session-context mining

---

## 4. The Novel Feature: Session Context Mining

> This is the feature Ijima has that no other memory system does.
> Implementation is open — this section captures the intent and the
> design space.

### The Idea

Most memory systems are either:
- **Curated memory** (like the mempalace): explicitly saved facts and
  decisions. High signal, but only captures what someone bothered to save.
- **Raw logs** (chat transcripts, session recordings): captures everything,
  but is unsearchable noise.

Ijima unifies both and adds the missing link: **automated extraction of
curated memory from raw session context.**

Sessions are stored verbatim in the session context repository. Ijima
then mines those sessions to extract:
- **Decisions** — "we decided to use DeepSeek", "abandoned ZeroClaw"
- **Facts** — "Quantizon depends on candle", "Minuet is at v0.5.0"
- **References** — URLs, paper citations, file paths
- **Patterns** — recurring topics, open questions, unresolved threads
- **Knowledge graph triples** — entity relationships discovered in
  conversation

The extracted items are written to the memory palace (with provenance:
which session, which harness, when). The raw session remains in the
repository for full-fidelity recall.

### The Session Context Repository

A new store (separate table/schema from the memory palace) holding raw
session context:

```sql
-- Proposed (design session decides exact shape)
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  harness TEXT NOT NULL,        -- 'pi', 'tsume/discord', 'sakamoto', 'wallace', 'opencode'
  channel TEXT,                 -- gateway/channel/thread identifier
  started_at TEXT NOT NULL,
  ended_at TEXT,
  metadata TEXT                 -- JSON: model, provider, user, etc.
);

CREATE TABLE session_turns (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id),
  turn_index INTEGER NOT NULL,
  role TEXT NOT NULL,           -- 'user', 'assistant', 'system', 'tool'
  content TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  tool_calls TEXT,              -- JSON if applicable
  FOREIGN KEY (session_id) REFERENCES sessions(id)
);
```

Every harness streams its turns here. The repository is append-only and
high-fidelity — no curation, no summarization at write time.

### The Mining Process

How Ijima turns sessions into memory palace entries. Implementation open,
but the design space:

1. **On-demand mining** — a harness or the TUI operator triggers
   "mine session X for memory." Ijima runs an extraction pass (LLM-based
   or rule-based) over the session turns and proposes memory entries +
   triples for review or auto-accept.

2. **Scheduled mining** — Ijima periodically scans sessions that haven't
   been mined and extracts new memories in the background. New sessions
   since last mining pass.

3. **Streaming/real-time mining** — as turns arrive, Ijima incrementally
   extracts and archives. Lower latency but higher cost.

4. **Hybrid** — lightweight real-time extraction (references, obvious
   facts) + deeper scheduled passes (decisions, patterns, graph triples).

**Provenance is critical**: every mined memory records its source session,
turn range, and extraction confidence. The memory palace entry should link
back to the raw session context so a user can trace "where did we decide
this?" to the exact conversation.

**Review workflow**: mined entries can be auto-accepted (high confidence,
factual), queued for review (medium confidence, decisions), or discarded
(low confidence). The TUI (Tsume's control plane) should expose a review
queue. This connects to Tsume's mempalace-browser dashboard.

### Open Implementation Questions

- **LLM for extraction**: DeepSeek (consistent with the stack)? A local
  model (privacy)? Per-harness provider routing? Extraction is a
  classification/summarization task — doesn't need frontier reasoning.
- **Dedup against existing memory**: mined facts must check the existing
  palace before writing (avoid "we decided X" saved 50 times).
- **Session boundaries**: how does a harness signal "session ended" vs
  a long-running persistent session (like a Discord channel)? Minetime
  windows? Idle gaps?
- **Privacy**: session context may contain secrets (API keys in logs).
  Mining must sanitize or redact before archiving to the palace.
- **Cost control**: mining every session with a frontier model is
  expensive. Tiered extraction (cheap rules first, LLM only when
  uncertain)?

---

## 5. Ijima's Architecture (Proposed)

```
   Harnesses (pi, Tsume, Sakamoto, Wallace, opencode, ...)
        │           │           │           │
        └───────────┴─────┬─────┴───────────┘
                          ▼
                  ┌───────────────┐
                  │  Ijima API    │  (HTTP/gRPC + optional embedded lib)
                  │  (the spec)   │
                  └───────┬───────┘
                          │
           ┌──────────────┼──────────────┐
           ▼              ▼              ▼
    ┌────────────┐ ┌────────────┐ ┌─────────────────┐
    │  Memory    │ │ Knowledge  │ │  Session        │
    │  Palace    │ │  Graph     │ │  Context Repo   │
    │ (semantic) │ │ (triples)  │ │ (raw sessions)  │
    └─────┬──────┘ └────────────┘ └────────┬────────┘
          │                                │
          │         ┌──────────────────────┘
          │         │
          │         ▼
          │   ┌──────────────┐
          └───│    Miner     │  (extracts palace entries from sessions)
              └──────────────┘
```

### Core Components

1. **API Layer** — the stable contract harnesses speak. Superset of the
   pi-mempalace RPC surface (store/search/recall/wakeup/status/stats/
   graph/tunnel/triples/entity/diary/dedup) PLUS session-context
   endpoints (ingest turn, end session, query session, list sessions).
   Transport: HTTP/JSON (simplest, matches the fork's server) with an
   optional gRPC or native-lib fast path for high-throughput harnesses.

2. **Memory Palace Store** — the curated semantic memory. Compatible
   schema import from pi-mempalace. SQLite + sqlite-vec (or a Rust
   equivalent like `rusqlite` + a vector index). Verbatim storage +
   embedding search + keyword search (hybrid).

3. **Knowledge Graph** — temporal triples (entities + facts with
   valid_from/valid_to). Imported from pi-mempalace schema. Queryable
   by entity, time-travel queries, timeline.

4. **Session Context Repository** — raw session turns from every
   harness. Append-only, high-fidelity. The novel store (see §4).

5. **The Miner** — the extraction engine. Reads sessions, proposes
   memory palace entries + triples. LLM-backed with rule-based
   pre-filtering. Provenance-tracked. Review-queue aware.

### Deployment Model

Match the fork's remote-backend model: Ijima runs as a daemon on the host
(or a private Tailscale node), harnesses connect over HTTP. Env-var
convention preserved or deliberately superseded. systemd service (trust-
by-default, no sandbox — same principle as Tsume). Single SQLite database
file (or a small set) for easy backup/migration.

### Harness Adapters / Extensions

Ijima is generalized. Each harness gets a thin adapter that speaks the
Ijima API:

- **pi**: replaces the in-process pi-mempalace with an Ijima client (the
  fork already has a remote-backend mode — Ijima is a compatible remote
  backend, then a successor).
- **Tsume**: native Rust client (replaces the ZeroClaw mempalace.rs
  bridge).
- **Sakamoto / Wallace**: Rust clients via the Ijima crate.
- **opencode**: adapter (language TBD — opencode's extension model).
- **Proserpina** (future): critique transcripts stream to the session
  repo; extracted findings mined to the palace.

Adapters are thin — they translate the harness's native memory calls into
Ijima API calls. No harness re-implements the memory logic.

---

## 6. IA-Standard Project Requirements

Ijima is a full Rust IA-standard project. Follow IA conventions (see
the IA coding standards and licensing skills):

- **License**: Apache-2.0 + CLA (IA standard). Proserpina was
  relicensed to Apache-2.0 in v0.2.0 (commit `e261ccf`), so the whole IA
  ecosystem is now license-uniform. pi-mempalace is MIT-licensed upstream;
  the fork is IA's. Ijima is a clean-room Rust reimplementation inspired by
  the model, so licensing is clean.
- **TDD**: test-first, non-negotiable. Schema import, store/search,
  mining extraction — all driven by tests.
- **Phantom types / algebraic patterns** where they add safety (e.g.,
  typed `Harness` identifiers, exhaustive extraction-result enums).
- **Feature gates**: embedding backends (fastembed/candle/remote), mining
  engine (llm/rules/none), transport (http/grpc/embedded).
- **Workspace structure** if it grows: ijima-core, ijima-server,
  ijima-miner, ijima-client (the adapter crate harnesses depend on).
- **mdBook docs**, CI, CHANGELOG, CONTRIBUTING per IA release-polish
  standards.

---

## 7. Relationship to the Stack

| Component | How it uses Ijima |
|-----------|-------------------|
| **Dominic** | Dispatches; reads/writes coordination memory + knowledge graph. May trigger mining. |
| **Wallace** | Multi-user agents share memory via Ijima (no per-agent memory islands). |
| **Sakamoto** | Pipeline runs archive decisions/results; mining extracts from pipeline transcripts. |
| **Tsume** | Archives Discord/gateway links + decisions; reads context on session resume; TUI exposes mempalace browser + mining review queue. |
| **pi** | Primary heavy user today. Replaces in-process mempalace with Ijima client. Research sessions mine to the palace. |
| **opencode** | Extension adapter; coding sessions archived + mined. |
| **Proserpina** (future) | Critique transcripts stream to session repo; findings mined to palace. |

---

## 8. Open Questions for the Ijima Design Session

1. **API transport**: HTTP/JSON (matches fork, simple, language-agnostic)
   vs gRPC (typed, fast) vs a Rust-native library crate (fastest, but
   Rust-only harnesses)? Probably HTTP/JSON for the spec, with an
   optional Rust client crate for in-process use.

2. **Schema evolution**: how to version the schema and migrate? The
   pi-mempalace schema is the v0 baseline. Ijima adds session tables.
   Forward-compatible migrations.

3. **Embedding backend**: fastembed-rs (ONNX, local, 384-dim MiniLM)?
   candle? Remote embedding API? Keep 384-dim for migration parity, then
   allow configurable dimensions later (re-embed on dimension change).

4. **Vector index**: sqlite-vec (matches pi-mempalace, easy migration)?
   A Rust-native option (usearch, hora, lance)? sqlite-vec for v0,
   evaluate alternatives for scale.

5. **Mining extraction LLM**: DeepSeek (stack-consistent)? Local model
   (privacy, cost)? How to route? Extraction is a summarization task —
   likely a cheaper/smaller model than the main reasoning model.

6. **Mining triggers**: on-demand, scheduled, streaming, or hybrid?
   How does a harness signal "session ended, ready to mine"? Idle-gap
   detection for persistent sessions?

7. **Review workflow**: auto-accept thresholds? Who reviews (the TUI
   operator via Tsume's dashboard)? How are pending mined entries stored
   (a review queue table)?

8. **Provenance depth**: store full session-turn backreferences, or just
   session ID + turn range? How to render "this memory came from this
   Discord exchange" in the TUI?

9. **Privacy/redaction**: session logs may contain secrets. Sanitize at
   ingest, at mine time, or both? A redaction pass before mining?

10. **Multi-tenancy**: is Ijima single-user (Elliott's stack) or
    multi-user (Wallace serves multiple operators)? Affects schema
    (per-user memory namespaces) and auth.

11. **Consistency model**: synchronous writes (every harness write
    blocks until stored) vs async (fire-and-forget with ack)? For chat
    gateways, async is fine; for decisions, synchronous may matter.

12. **Backup/export**: single SQLite file is easy to back up. Should
    Ijima support export to the pi-mempalace JSONL format for
    interoperability? Import from other memory systems (Mem0, Zep)?

---

## 9. Next Steps for the Ijima Session

1. **Read this document, then `../Tsume/docs/HANDOFF.md` and
   `../Dominic/docs/HANDOFF.md`** for the consumer-side context.
2. **Study the pi-mempalace fork** — especially
   `server/pi-mempalace-server.ts` (the central-backend prototype Ijima
   succeeds) and `extensions/pi-mempalace/memory_store.ts` (the full
   store spec). These define Ijima's v0 API and schema.
3. **Design the schema** — import-compatible with pi-mempalace (memory
   palace + knowledge graph) PLUS the new session-context repository
   tables. TDD the schema import against the live
   `~/.pi/agent/memory/memories.db`.
4. **Design the API** — superset of the pi-mempalace RPC surface plus
   session-context endpoints. Decide transport (HTTP/JSON recommended
   for v0).
5. **Prototype the miner** — even a rule-based extractor (URL detection,
   "we decided" pattern matching) over a sample session is a start. LLM
   extraction comes after the plumbing exists.
6. **Stand up the server** — systemd daemon, trust-by-default, SQLite
   backend, the API. Get pi talking to it as a remote backend (the fork
   already supports this mode).
7. **Migrate** — import `~/.pi/agent/memory/memories.db` and
   `~/.zeroclaw/workspace/memory/brain.db` into Ijima. Verify counts and
   search parity.
8. **Wire the harnesses** — pi first (replaces in-process mempalace),
   then Tsume (replaces the Rust bridge), then others as they mature.

---

## 10. Key Paths Reference

| Item | Path |
|------|------|
| Ijima repo | `~/working/industrial-algebra/Ijima/` |
| This handoff doc | `~/working/industrial-algebra/Ijima/docs/HANDOFF.md` |
| pi-mempalace fork (primary reference) | `~/working/industrial-algebra/pi-mempalace/` |
| pi-mempalace server (central backend proto) | `~/working/industrial-algebra/pi-mempalace/server/pi-mempalace-server.ts` |
| pi-mempalace store spec | `~/working/industrial-algebra/pi-mempalace/extensions/pi-mempalace/memory_store.ts` |
| Live mempalace DB (to migrate) | `~/.pi/agent/memory/memories.db` |
| ZeroClaw brain.db (to merge) | `~/.zeroclaw/workspace/memory/brain.db` |
| ZeroClaw Rust mempalace bridge (reference) | `~/working/ai/zeroclaw-fork/src/mempalace.rs` |
| Tsume handoff (consumer) | `~/working/industrial-algebra/Tsume/docs/HANDOFF.md` |
| Dominic handoff (consumer) | `~/working/industrial-algebra/Dominic/docs/HANDOFF.md` |
| Dominic requirements (consumer) | `~/working/industrial-algebra/Dominic/docs/REQUIREMENTS.md` |
