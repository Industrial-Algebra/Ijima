# Introduction

**Ijima** is the centralized memory service for the Anima ecosystem — the
single source of truth for agentic memory across every harness (pi, Tsume,
Sakamoto, Wallace, Dominic, opencode, …). It replaces fragmented,
per-harness memory stores with one service every agent reads from and
writes to.

## What it does

Ijima holds three things:

1. **The Memory Palace** — curated long-term semantic memory + a temporal
   knowledge graph. Local candle embeddings, cosine search, content-hash +
   semantic dedup.
2. **The Session Context Repository** — raw session transcripts from every
   harness, append-only and high-fidelity.
3. **The Miner** — refines raw sessions into curated palace entries
   (decisions, facts, references, patterns) with full provenance: *"this
   memory came from that conversation."*

## Why it exists

Memory was siloed per harness — pi had its mempalace, ZeroClaw had
`brain.db`, OpenClaw had JSONL dumps. A decision archived in one context
was invisible to the others, and valuable ore in raw session transcripts
was never refined. Ijima centralizes the store *and* adds the missing link:
automated extraction of curated memory from raw sessions. See
[`docs/DESIGN.md`](https://github.com/Industrial-Algebra/Ijima/blob/develop/docs/DESIGN.md)
for the full decision log (D1–D11).

## Status

Unreleased, in active development toward 0.1.0. The features above are
merged to `develop` and tested; nothing is "shipped" until the crates are
published to crates.io and tagged. See the [Roadmap](./design/roadmap.md).

## Key features

- Multi-tenant namespaces (personal / shared / global) with isolation.
- Schubert capability auth on the Grassmannian Gr(4,8).
- A rules + LLM mining pipeline with a per-namespace review queue.
- Provenance on every memory (origin instance, authority scope, trust tier).
