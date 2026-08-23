# Changelog

All notable changes to Ijima are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **KG re-import was error-prone on both ends** (found during the fleet
  gap-import pass): (1) `invalidate_triple_in` URL-embedded the
  deterministic triple id unencoded — paragraph-long entity names
  (common in imported corpora) contain slashes that split the path
  (404s); the id is now percent-encoded. (2) `add_triple` propagated
  already-exists on re-add (500 + client skip for every existing
  triple); re-adding now reads back the existing record — the KG
  equivalent of memory content-hash dedup, making corpus re-imports
  true no-ops.


- **pi wake-up reminder injection was skipped when wake-up was empty**
  (npm 0.2.4): fresh principals — the ones that need the tool reminder
  most — never saw the `## Agent Memory (ACTIVE)` block. The reminder is
  now unconditional; wake-up context appends when present. Found by the
  first live 0.2.3 fleet session.
- **`pi.extensions` manifest entry was missing — tools and hooks never
  ran under pi** (npm 0.2.4): the package declared only the skills
  manifest, so pi file-scanned the skill and never executed `index.js`
  — silently, on every npm install since 0.2.1. The nine
  `memory_*`/`knowledge_*` tools, auto-capture, and wake-up injection
  were absent from every live session (the extension itself was
  innocent — loads cleanly under a mock API, which is also why our
  node-level E2E never caught a manifest bug). Found by a fleet session
  that patched it locally and verified the tools register. Manifest now
  declares `extensions: ["./index.js"]` alongside the skills.
- **The injected block now carries the memory-model cheatsheet**: the
  skill's critical guidance (namespaces, "empty is scoping", visible
  search first, wake-up self-priming) distilled into every system
  prompt — the pi-mempalace pattern. Skills are passive (agents must
  choose to consult them); the cheatsheet is present from turn one. The
  full skill remains the deep-dive reference.

## [0.2.3] — 2026-08-23

### Added

- **`ijima` skill ships with the pi package** (`skills/ijima/SKILL.md`,
  npm 0.2.2): the namespace mental model + diagnostics ladder for agents
  — why "empty" results are usually scoping (personal-namespace probes,
  nonexistent-namespace browses), why `memory_search` (`scope=visible`)
  is the real brain test, why the `/repos` table error is a cosmetic
  fingerprint on every store, and "never conclude wrong-data-dir without
  the admin census". Encodes every misdiagnosis observed in the field.

- **pi auto-capture + wake-up injection + token fallback** (npm 0.2.3):
  the extension now closes the memory loop without agent diligence —
  `turn_end` stores each exchange at the `AutoCapture` tier (length
  gates, 2000-char truncation, silent failure), `before_agent_start`
  appends an `## Agent Memory (ACTIVE)` block (tool reminder + wake-up
  essentials + doctrine, refreshed per session), and `IJIMA_TOKEN`
  falls back to `~/.config/ijima/token` / `$IJIMA_TOKEN_FILE` when the
  shell didn't export it. Verified E2E against a live daemon: the
  exchange captured on `turn_end` appears in the very next injected
  system prompt. Ported from pi-mempalace's three-prong design.

### Fixed

- **`repo_directory` missing from the open-time DDL**: `GET /repos` on a
  fresh store hard-errored (`table does not exist`) — surrealdb 3
  rejects `SELECT` from a never-written table, and the repo registry was
  the one table absent from the `DEFINE TABLE` set. The existing
  round-trip test masked it (its register-first upsert materializes the
  table implicitly); a list-first regression test now pins the
  fresh-store path to an empty 200.

## [0.2.2] — 2026-08-22

### Added

- **NixOS support**: root `flake.nix` — `packages.x86_64-linux.ijima`
  (built from the repo's own source on the pinned nightly toolchain the
  release was verified on; nixpkgs' stable rustc mis-selects diskann's
  AVX-512 VNNI intrinsic), `nixosModules.ijima` (hardened systemd service
  module: `services.ijima.{enable,package,dataDir,bindAddress,port,user,
  memoryMax}`), and a `module-eval` flake check that integrates the module
  into a real NixOS evaluation. Book: new "NixOS" guide chapter.


- **`HashEmbedder`** (`ijima-core`): deterministic, dependency-free
  embedder for tests/examples — consistent geometry without a model
  (model id `hash-embedder` so provenance detects it). Unblocked the
  first route-level search tests.

### Fixed

- **`scope=visible` now spans the principal's readable world**: own
  private + `global` commons + open `ns_import_*` staging + every org
  wall they hold membership in (pre-WS2 definition merged only private +
  global, so imported corpora and wall content were invisible to
  `scope=visible` searches — the "empty brain" first seen by a live pi
  session: extension installed, token valid, daemon full, every search
  empty). New `Store::list_namespaces_for_principal` (backed by a
  principal index on `namespace_members`) drives wall discovery;
  membership still gates — absent walls never appear.

