# Roadmap

**Shipped:**

- **v0.1.0** (2026-08-10) — the library: two-store model, Schubert
  capability auth, SurrealDB backend, mining pipeline, pi extension,
  crates.io publication of core/server/miner/client.
- **v0.2.0 "Central Brain"** (in progress) — the deployment release:
  config-file layer + deploy kit (WS1), GrantToken migration, token
  revocation, multi-source import over HTTP (WS2), namespace membership
  (WS3), Proserpina agent surface (WS0), dependency sweep incl.
  surrealdb 3.

**Next (0.3 horizon):**

- **Satellite sync** — full local instances with checkpoint export/push
  to the center (the WS6 design seed); the federation control API grows
  into enforcement.
- **Batch ingest (`turns:batch`)** for machine feeds (Minoru mining,
  Quantizon experiments) and **scheduled mining** (CLI + systemd timer)
  ride the 0.2.x line.
- **Schubert 0.5 adoption** — GrantToken expiry + nonce; reconciliation
  with instance-side revocation as defense in depth.
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
