---
id: doctrine-ijima-architecture
project: ijima
topic: architecture
---

Ijima is the centralized agentic memory backend for the IA ecosystem. It
replaces fragmented per-harness memory stores with a single service every
agent reads from and writes to.

## Two-store model

1. **Memory Palace** — long-term semantic memory + knowledge graph.
2. **Session Context Repository** — raw session transcripts from every
   harness. Mined into the palace by `ijima-miner`.

## Key decisions

- **Embeddings**: candle (all-MiniLM-L6-v2, 384-dim) for pi-mempalace
  migration parity.
- **Storage**: SurrealDB (native multi-tenancy + graph + embedded deploy).
- **Auth**: Schubert proof-carrying capability tokens (authn + authz in
  one open crate). Policy on Gr(4,8), derived from `schubert recommend`.
- **Doctrine** (this tier): curated, Git-versioned, PR-reviewed. Never
  written directly by agents.
