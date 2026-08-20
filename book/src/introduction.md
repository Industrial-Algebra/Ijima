# Introduction

**Ijima** is the centralized memory service for the Anima ecosystem — the
single source of truth for agentic memory across every harness (pi, Tsume,
Sakamoto, Wallace, Dominic, opencode, …). It replaces fragmented,
per-harness memory stores with one service every agent reads from and
writes to, with per-principal isolation, quantitative access control, and
full provenance on every stored fact.

## What it does

Ijima serves three related capabilities:

1. **A memory palace** — curated memories organized as project → topic
   rooms, with importance-weighted recall, semantic search, and a diary
   per agent.
2. **A knowledge graph** — subject/predicate/object triples with entity
   resolution, timelines, and cross-project tunnels.
3. **A session repository + mining pipeline** — raw agent session turns
   flow in, an extraction engine (rules tier, optional LLM tier) proposes
   candidate memories with confidence scores, and a human/agent review
   queue promotes the good ones into the palace with provenance intact.

## Why it exists

Before Ijima, each harness carried its own memory: pi-mempalace SQLite
databases per workstation, ZeroClaw's Discord brain, ad-hoc files. Memory
was fragmented (the same insight saved five times, never reconciled),
unauditable (no notion of *where* a fact came from or *how much to trust*
it), and unsecurable (one all-or-nothing bearer token). Ijima makes memory
a *service*: one instance, many principals, capability-scoped access, and
a trust tier on every entry.

## Ecosystem position

Ijima is the memory plane under Dominic's orchestration plane:

- **[Schubert](https://github.com/Industrial-Algebra/Schubert)** provides
  the capability algebra (Grassmannian Gr(4,8)) that Ijima uses for
  authorization, proof-carrying GrantTokens, and rate limiting. Ijima is
  Schubert's first and most complete consumer.
- **Dominic** (meta-orchestrator) dispatches work and federates through
  Ijima's control plane; Ijima enforces trust boundaries locally even when
  Dominic is unreachable.
- **pi** connects as a thin client (`IJIMA_URL` / `IJIMA_TOKEN`), replacing
  the pi-mempalace extension.
- **Proserpina** supplies the LLM agent surface for the mining tier
  (`proserpina-agent`), cross-repo contract-tested.

## The workspace

| Crate | Role |
|---|---|
| `ijima-core` | Domain types, `Store`/`KnowledgeGraph` traits, capability vocabulary |
| `ijima-server` | HTTP daemon (axum), SurrealDB backend, auth, CLI |
| `ijima-client` | Typed async HTTP client for harnesses |
| `ijima-miner` | Extraction engine (rules + LLM tiers) |
| `ijima-pi` / `integrations/pi` | pi extension (WASM/npm) |

Ijima is v0.x: interfaces evolve, breaking changes are announced in the
[CHANGELOG](https://github.com/Industrial-Algebra/Ijima/blob/develop/CHANGELOG.md),
and the 0.2.0 "Central Brain" release targets a real single-instance
deployment with all workstations as thin clients.
