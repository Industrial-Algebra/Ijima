# Context-Poisoning / Doctrine-Health Protection (design seed)

> **Status:** Under investigation. **Blocked on an incident report** (the
> triggering GPT-5.6 session occurred on another machine; the operator is
> assembling a report). This document captures the brainstorming analysis so
> far so a future session can resume without re-deriving it.

## Origin

An incident in which a **trusted, front-loaded system/doctrine prompt**
caused a GPT-5.6 session to **deteriorate to uselessness** — but *only* on a
specific task class. The model performed fine on every task until it was
asked to do **AI research**, at which point it became "very overbearing and
sloppy."

This is the most dangerous poisoning class: the malicious/degraded content
**is** the instructions, and it is **trusted** (doctrine / system prompt the
agent is told to obey unconditionally). The standard RAG defense — *"treat
retrieved content as data, not instructions"* — is useless here, because the
poison is trusted-tier instructions, not retrieved data.

## Mechanism hypothesis (to confirm against the incident report)

Operator's hypothesis: a combination of over-constraint, a specific bad
instruction, and gradual drift — with the **specific bad instruction** the
likely primary cause.

The task-specific signature ("fine everywhere, overbearing + sloppy on AI
research") points most strongly to a **dose-dependent stance/persona
directive** — e.g. "be rigorously critical / adversarial on research" —
that is healthy in moderation but pathological when over-applied, and that
only *activates* on a given task domain. Under that model:

- **"Overbearing"** = the stance directive firing maximally.
- **"Sloppy"** = the resulting over-constraint cannibalizing the model's
  capacity for everything else.
- **Drift** = no single revision looks wrong in isolation; the directive
  accreted intensity across versions.

*Confirm against the report:* is there a stance/persona directive (critical,
adversarial, skeptical, rigorous, dominant) scoped to research or reasoning
tasks? Did its intensity grow across revisions? Is it conditional on a task
trigger?

## Why this is hard

- The poison is **trusted** — data/instruction delimiting does not apply.
- The failure is **task-domain-conditional** — a doctrine that is benign on
  95% of tasks only reveals itself on one class, so naive "does this
  doctrine look bad?" checks will pass it.
- The failure is **dose-dependent** — the directive isn't wrong at lower
  intensity; the *amount* is the problem.
- "Overbearing and sloppy" is **hard to measure objectively**, which rules
  out cheap automated pass/fail gating without behavioral probes.

## Candidate approaches (from brainstorm)

- **A — Doctrine Gatekeeper** (ingest + serve validation): stance-directive
  audit + contradiction detection + budget caps + behavioral regression
  probes across task domains (incl. research) before accepting a doctrine
  revision. *Strongest prevention; assumes Ijima owns all doctrine;
  objective badness is hard to measure; LLM-probe cost at ingest is high.*
- **B — Doctrine-Health Registry + Outcome Correlation**: version all
  doctrine, correlate doctrine-version ↔ session outcome, detect
  degradation from turn-pattern signals (loops/refusals/repetition) or
  operator reports, enable bisect + one-command rollback. *Uses real
  outcomes rather than judging badness; great for catching drift;
  reactive (after damage); needs a degradation signal.*
- **C — Stance-Budget + Task-Domain Profiles**: tag doctrine fragments by
  type (stance/persona vs factual/procedural) and task-domain affinity;
  at serve, cap the stance-directive dose and serve a curated, tested
  doctrine *profile* per task domain so research gets a bounded subset,
  not the full load. *Directly targets the dose-dependent mechanism;
  serve-time only; requires doctrine to be structured/tagged.*

**Tentative recommendation:** **B + C as the spine, plus a targeted slice
of A** (cheap static ingest-time stance-directive validation, *not* full
behavioral regression). B catches drift and enables rollback; C contains
the dose-dependent directive and makes it task-conditional; the A-slice
catches obvious pathological directives before they ever serve. Defer full
A (LLM behavioral probes at ingest) until B+C prove out — "overbearing and
sloppy" is too hard to validate objectively at ingest to justify the cost
first.

## Open architectural question (gates the design)

**Is Ijima the authoritative doctrine store?** Ijima already has a doctrine
ingest pipeline (Git → CI → service, `POST /doctrine`) and serves wake-up
composition, so it is *positioned* to own doctrine. But if the poisoned
prompt was assembled at the harness level (pi's own system prompt / skills),
Ijima only sees pieces, and its role becomes a **doctrine-health contract
the harness opts into** rather than a hard gatekeeper. The report should
establish where the poisoned text actually lived.

## Ijima building blocks that already apply

- Doctrine ingest pipeline (the natural ingest-time validation attach point).
- `MemorySource` provenance tiers (Doctrine / Explicit / Mined / AutoCapture)
  — a foundation for trust tiers.
- Redaction filter — an existing content-transformation pass at ingest; a
  stance-audit pass could share this seam.
- Versioned doctrine (partial) — the spine B needs.
- Context Mapper — task-domain/CWD context that could drive task-domain
  profiles (C).

## Resume checklist (when the incident report lands)

1. Confirm/refute the dose-dependent stance-directive mechanism against the
   actual poisoned text.
2. Establish the architectural fact: Ijima-owns-doctrine vs harness-level.
3. Re-evaluate the B+C+A-slice recommendation in light of (1) and (2).
4. Write a full design (`docs/plans/`) and ADR; then implement.
