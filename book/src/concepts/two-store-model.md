# The Two-Store Model

Ijima serves two related but distinct stores, unified by a miner that turns
one into the other.

## 1. The Memory Palace

Long-term, curated, semantic memory. Verbatim storage + candle embeddings +
cosine search + a temporal knowledge graph (entities and triples with
`valid_from`/`valid_to`). This is the pi-mempalace model, production-proven,
reimplemented in Rust and import-compatible with its schema.

Palace entries always carry **provenance**: source tier, harness, session,
origin instance, and authority scope — so any entry traces back to the
conversation that produced it.

## 2. The Session Context Repository

Raw session transcripts from every harness: every conversation, every agent
run, every gateway exchange. Append-only, high-fidelity, uncensored at
write time. Not curated, not summarized — the raw ore.

## The Miner: ore → metal

The novel capability. Ijima mines raw sessions to extract curated palace
entries:

- **Decisions** — "we decided to use DeepSeek."
- **Facts** — "Ijima depends on candle."
- **References** — URLs, citations, file paths.
- **Patterns** — recurring topics, open threads.

A **rules tier** runs unconditionally (no model); an optional **LLM tier**
(Proserpina) adds Fact + Pattern roles. Low-confidence extractions stage in
a per-namespace **review queue** rather than auto-archiving. See
[The Mining Pipeline](../guide/mining.md).

```
   raw sessions ──▶ [ rules + llm ] ──▶ Auto (palace) / PendingReview (queue)
```

The raw session always remains in the repository for full-fidelity recall,
even after mining.
