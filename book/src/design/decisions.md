# Decision Log (ADRs)

Architecture decisions live as ADRs in `docs/adr/` in the repository.
Index with one-line outcomes:

| ADR | Decision |
|---|---|
| `grant-token-migration.md` | Adopt Schubert 0.4 GrantTokens (multi-capability, partition-signed) as the sole bearer format; delete the duplicated wire codec; admin via geometry (point class), not string equality |
| `token-revocation.md` | Store-backed SHA-256 bearer-hash revocation list (no raw bearers at rest); checked after signature verification; expires are upstream (Schubert 0.5) — revocation and expiry are complementary |
| `provenance-tier-model.md` | MemorySource trust grades map to Schubert codimensions; trust transitions are capabilities; imports land AutoCapture |
| `miner-architecture.md` | Two-tier extraction (deterministic rules + optional LLM via proserpina-agent); confidence-routed to auto-file or review queue |
| `compaction-recovery.md` | Session compaction keeps recoverable turn history for re-mining |
| `federation-control-api.md` | `/federation/*` control scaffold with instance identity (`IJIMA_INSTANCE_*`); boundary enforcement staged for 0.3 |

## Standing decisions recorded elsewhere

- **Thin clients in 0.2.0, satellites in 0.3** — all workstations point
  at the central instance; local instances with checkpoint sync are the
  next design (`docs/plans/`).
- **Membership-in-store over policy-TOML grants** for shared namespaces —
  mutable at runtime without redeploying.
- **Config precedence defaults < file < env < CLI**, and an explicit
  `$IJIMA_CONFIG` pointing at a missing/malformed file is a hard error.
- **Announcements stay manual** — the reusable auto-announce workflow
  will be proven on another project first.
