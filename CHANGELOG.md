# Changelog

All notable changes to Ijima are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] — 2026-10-09 — "The Trust Machinery"

The 0.4 arc, scoped by operator decision (2026-09-22) from the 09-22
dive's synthesis: *prose about behavior is interpreted-tier; behavior
itself is observed-tier — only observed-tier claims should compound.*
The trust machinery gives that thesis its substrate. The dreamer and
the rank fork (decay vs derived) go to 0.5 by the same decision.

### Evidence grades (direction D)

- Every `Memory` carries `EvidenceGrade` (`Observed`/`Interpreted`,
  default `Interpreted` — legacy rows and ungraded saves are the weaker
  claim, never masquerading as observed fact) plus typed `Citation`s
  (Commit/Report/Session/File/Url).
- **Observed claims cite or they do not ship**: saving `Observed`
  without at least one citation is rejected 400.
- Auto-capture stamps `Observed` with a Session citation (it witnessed
  the session); promotion carries the source's grade and citations.
- The two axes: provenance tier = *who wrote it*; evidence grade =
  *how they know it*. The grade crosses tier lines.

### Supersede — the ritual becomes a mechanism

- A save declaring `supersedes: <id>` writes the inverse link onto its
  target atomically; superseded rows are excluded from wake-up, search,
  and browse regardless of tier arithmetic, while recall keeps them
  visible (fossils, not deletions).
- RABBIT_HOLE_2026-09-22 §1 found corrections working only as
  choreography — a manual 0.8 save tying the wrong memory and winning
  the recency tiebreak, while any correction arriving by any other path
  lost forever to `ORDER BY importance DESC, created_at DESC`. The
  archaeology (RABBIT_HOLE_2026-10-08): the multiplicative ranking was
  the founding spec; the SQL was the simplification; which layer is
  authoritative was the 0.4 question. 0.4.0 answers by making
  corrections structural.
- Chain-through-successor semantics (re-superseding a superseded row is
  rejected); the complete insert/validate/claim transition is atomic
  (conditional claim + compensation + store-level serialization), so
  concurrent successors, reused ids, and mutually-referencing saves all
  see the serial outcome.

### Doctrine versioning, rollback, and ingest validation (directions B + A)

- Ingest archives the outgoing body before overwriting — the
  delete-then-store history destruction ends.
- **Revision numbers are stable body identities**: a returning body
  restores the number it first went live with; a new body takes
  max-ever + 1; no number ever identifies two different bodies.
- One-command admin rollback (`POST /doctrine/rollback {id, to}`)
  rewrites the live row from any archived version; active flags are
  truthful by construction (the archived version matching the live
  body, if any).
- The whole ingest transition — authoritative live-dedup (retired rows
  are never canonical; losers are retired via supersede links, never
  deleted), archive, allocation, replacement, active-recompute — runs
  under one lock: concurrent ingests, rollbacks, dedups, and
  restorations serialize with stable identities.
- A-slice stance validation: advisory warnings (density, accretion,
  long-dense) surface in ingest responses for the PR reviewer —
  doctrine stays PR-reviewed, never auto-blocked.
- Doctrine bodies ingest as `Observed` with a Report citation.
- `/memories/check` is live-only, matching the save path it preflights.

### Stratified wake-up — the starvation fix

- Personal essentials compose two strata: the recency stratum (8
  freshest non-superseded rows, admitted first — the guarantee) plus
  lexicographic fill to 20. A wall saturated by high-importance rows
  can no longer starve fresh default-tier memories out of wake-up;
  underfull walls return every eligible row.

### Extension and docs

- `memory_save` gains `evidence`/`citations`/`supersedes` parameters
  (absent params produce absent body keys — server defaults apply);
  offline builder tests and live e2e round-trips to the 0.4 contract.
- Book: the evidence-axis section (two-axes table), corrections,
  doctrine versioning/rollback routes, stratified wake-up.
- ROADMAP status header caught up (was "unreleased — toward 0.1.0",
  four releases stale); README Status section diffed per the new
  release-checklist line.

### Dependencies

- surrealdb 3.2.4 → 3.3.0 (#130): the active rkyv moves to 0.8.18,
  closing RUSTSEC-2026-0235 in the build graph. A residual
  `rkyv 0.7.46` lock entry remains via `rust_decimal`'s optional edge —
  not in the compiled Linux build path (documented, not shipped).
  rsa Marvin-attack timing (RUSTSEC-2023-0071) remains
  documented-unfixed upstream.

### Process

- The arc was built via ia-moment generation dispatch (five units
  against plan contracts) and reviewed through seven astra-class rounds
  to convergence: 19 findings, all verified-then-fixed with named
  regression tests; four fix-attempt defects disclosed and caught by
  gates. The regression suite pins every repro.

## [0.3.1] — 2026-09-21 — extension: declarative per-project namespaces

The pi extension gains project-local configuration: a repo declares which
memory wall it belongs to, and an org's repo cloned anywhere lands in that
wall on first launch — no shell env, no direnv, no launch wrappers.

- **`.pi/ijima.json`** (nearest, walking up from the session root):
  `{ "namespace": "ns_orthant_shared", "url": …, "token_file": … }` —
  all keys optional. Precedence: environment variable → project file →
  default, for all three knobs.
- **`IJIMA_HOME`** overrides home-directory expansion (`~` paths) — for
  containers, sandboxes, and tests.
- **Fix: the well-known token-file fallback (`~/.config/ijima/token`) never
  expanded `~`** (`replace("^~")` matched a literal caret) — silently
  unreadable since 0.2.x. Now expanded correctly.
- Offline unit tests for the resolution chain (`config.test.mjs`, 15
  checks) plus a live E2E: a bare directory with a two-line config captured
  into its declared namespace through a production daemon.
- Rust crates are version-companions to the npm package (no server changes
  in this release); the whole workspace bumps so the tag publishes cleanly.

## [0.3.0] — 2026-09-10 — "The Curated Brain"

The curated half of the two-store model goes live: markdown corpora
ingest into org-scoped walls, ambient chatter learns to age out, and
the operator gains cleanup + extract tools that work against a live
daemon.

### Added

- **Namespace-scoped doctrine** — `POST /doctrine?namespace=<wall>`
  upserts curated entries into an org-scoped wall instead of the
  instance-global namespace (visibility fix for internal corpora);
  admin gate unchanged.
- **`doctrine:write` capability** — the least-privilege write for
  unattended ingest timers: explicitly-targeted non-private walls
  only; the global default namespace stays admin-only.
- **Tree-mode ingest** — `ijima doctrine ingest --root <TREE>` walks a
  markdown corpus (fnmatch include/exclude over posix relpaths,
  `*` crosses `/`), synthesizes stable `doct_<hash12(relpath)>` ids so
  re-runs upsert edits and append additions without duplication, and
  passes id-carrying files through verbatim. `--dry-run` prints the
  plan without a daemon. Ingest rides 429/503 backoff.
- **AutoCapture TTL sweeper** — daily daemon task deleting
  `source = AutoCapture` rows older than the TTL (default 30 days;
  `autocapture_ttl_days` / `IJIMA_AUTOCAPTURE_TTL_DAYS`, `0`
  disables). Tier-gated by construction: Explicit, Doctrine, Mined,
  and imported rows never age out.
- **KG hard-delete** — `DELETE /kg/triples/{id}?namespace=<ns>`
  (admin, namespace required, private walls valid targets) plus
  client `delete_triple_in` and `ijima kg-delete`: the misplaced-data
  cleanup tool.
- **Logical JSONL export** — `GET /export` (admin) and a rewritten
  `ijima export --url --token [--namespace] [--out]`: one
  `{"namespace": …, "memory": {…}}` object per line, count header.
  Works against the running daemon; the runbook documents the
  tiering (mirror remains the primary restore; logical restore is a
  re-ingestion).
- **npm publishing via trusted publishing (OIDC)** in the tag
  workflow — build-from-source, tag/version match guard, idempotent
  re-tags, provenance automatic. No token in the common path.

### Changed

- Doctrine ingest collapses byte-identical content to the existing id
  (upsert-compatible dedup; corpora with duplicate files re-ingest
  idempotently instead of 409ing forever).
- `ijima export` now requires `--url`/`--token` (daemon API path);
  the direct-to-store SQL dump is removed — it lost the store LOCK
  race to the running daemon.

### Fixed

- Dependency advisories (final read-over sweep): ammonia 4.1.3→4.1.4
  (RUSTSEC-2026-0213, XSS via SVG animation tags — transitive via
  surrealdb), h2 0.4.15→0.4.19 (RUSTSEC-2026-0258, unbounded empty
  DATA frames — on the HTTP listening path via axum/hyper), and
  crossbeam-epoch 0.9.18→0.9.21 (RUSTSEC-2026-0204). Remaining,
  documented: rkyv 0.7 OOB reads (RUSTSEC-2026-0235 — fix requires
  surrealdb to move to rkyv 0.8; 3.2.4 is the latest stable) and
  rsa Marvin-attack timing (RUSTSEC-2023-0071 — no upstream fix; not
  in the Linux build path).
- Soak-log split-brain class of issue documented in ops (canonical on
  the primary, never inside a mirror target) — see the deploy guide.

## [Unreleased]

_Nothing yet — 0.3.0 development begins.

## [0.2.5] — 2026-08-29

### Added

- **npm publishing wired into CI** (trusted publishing / OIDC, the Amari
  pattern): the tag workflow builds the extension from source (wasm-pack +
  tsc via new devDependencies), verifies the package version matches the
  tag, checks npm for the version, and publishes with automatic provenance —
  no token in the common path, idempotent on re-tags. One-time setup: link
  `@industrialalgebra/ijima-pi` on npmjs.com to this repo + `publish.yml`;
  fallback remains a granular `NPM_TOKEN` secret. The 0.2.5 publish was the
  last manual 2FA step.

### Added

- **Agent homes** (0.2.5): `IJIMA_NAMESPACE` selects a shared home
  namespace for pi captures, saves, dedup checks, and wake-up
  (`?namespace=` on `GET /wakeup`, the last route without it) — shared
  knowledge stays shared, org walls keep other orgs out. Field origin:
  fleet-institutional memory landed in per-host private namespaces and
  sibling agents could not recall it across hosts.
- **`IJIMA_URL` file fallback** (`~/.config/ijima/url`): hosts work with
  no shell env at all (token file + url file).
- **Content-derived memory ids** in the pi shim (`mem_<hash16>`):
  deterministic, meaningful in transcripts, idempotent.

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

