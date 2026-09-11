# Roadmap

**Shipped:**

- **v0.1.0** (2026-08-10) — the library: two-store model, Schubert
  capability auth, SurrealDB backend, mining pipeline, pi extension,
  crates.io publication of core/server/miner/client.
- **v0.2.0 "Central Brain"** (2026-08-21) — the deployment release:
  GrantToken migration + signed expiry + policy-constrained issuance
  (Schubert 0.4 → 0.5), token revocation, config-file layer + deploy
  kit, multi-source import over HTTP (WS2), membership-gated org walls
  (WS3), Proserpina agent surface (WS0), surrealdb 3, the book you are
  reading.
- **v0.2.5 "Agent Homes"** (2026-08-29) — shared home namespaces for
  agents (`IJIMA_NAMESPACE` + namespaced wake-up): fleet knowledge lives
  in the org wall, not per-host silos; org walls keep other orgs out.
  Plus URL file fallback and content-derived memory ids.
- **v0.2.3 "Loop-Closers"** (2026-08-23) — the pi extension completes
  the memory loop: auto-capture per turn, wake-up injection per session,
  token-file fallback; the bundled agent skill; `repo_directory` DDL fix
  (fresh-store `/repos` was a 500).
- **v0.2.2 "Visible World"** (2026-08-22) — the "empty brain" fix:
  `scope=visible` search now spans the principal's readable world
  (private + global + import staging + member org walls); first-party
  NixOS flake (package + service module); `HashEmbedder` test utility.
- **v0.2.1 "Field Lessons"** (2026-08-21) — first production
  deployment hardening: knowledge-graph import (entities + triples
  alongside memories, id-is-name re-addressing), client 429 backoff
  (rate-limited daemons slow imports down, never drop rows),
  reproducible pi-extension build. Found on the fleet's first production
deployment.

**Shipped (0.3.0 — "The Curated Brain", 2026-09-10):**

- **Markdown corpora into walls** — namespace-scoped doctrine ingest
  (`?namespace=`), the `doctrine:write` capability for unattended
  timers, tree-mode CLI (`--root`, stable path-derived ids,
  dry-run), and duplicate-collapse semantics for idempotent re-runs.
  The first corpus: the research-report corpus, two walls
  (reports+docs / arxiv), a daily morning ingest ahead of the day's
  sessions.
- **AutoCapture TTL sweeper** — tier-gated lifecycle: ambient chatter
  ages out (30d default), deliberate saves and doctrine never do.
- **KG hard-delete** + **logical JSONL export** through the daemon —
  operator hygiene that works against a live instance.
- **npm publishing via trusted publishing (OIDC)** in CI.

**Next (0.4 horizon):**

- **Satellite sync** — full local instances with checkpoint export/push
  to the center (the WS6 design seed); the federation control API grows
  into enforcement.
- **Batch ingest (`turns:batch`)** for machine feeds (Minoru mining,
  Quantizon experiments) and **scheduled mining** (CLI + systemd timer).
- **Contemplative loop plumbing** — the daily reflection organ reads
  wake-up + deltas + a randomness seed and writes back into the corpus
  (IA-research hosts the prompt; Ijima already carries the loop).
- **Block↔memory promotion boundary** — the doctrine note (Lonis Block
  kinds × promotability × trust tiers) that Wallace and Ijima will
  implement against.

**Long game:**

- Context-poisoning protection (defending trusted-tier doctrine going
  pathological) — designed, gated on a real incident report.
- Networked instances / federation cross-talk policies.
- The doctrine-authority question: is Ijima the authoritative doctrine
  store, or an opt-in doctrine-health contract?

The authoritative, continuously-updated version lives in the repository:
[`docs/ROADMAP.md`](https://github.com/Industrial-Algebra/Ijima/blob/develop/docs/ROADMAP.md).
