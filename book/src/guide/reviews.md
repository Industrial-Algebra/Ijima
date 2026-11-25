# Ingesting review threads

> Since 0.4.3. PR inline review comments land on the org walls.

The review loop treats the PR thread as the durable record: finding,
verification, disposition. Ingesting the thread makes that record
searchable memory — and the corpus gains the exact knowledge the
evidence machinery was built to hold: findings are *interpreted*
until dispositioned; dispositions are *observed*.

## The shape

Each inline comment becomes one memory on the org wall
(`ns_<org>_shared` — cross-org search comes free through
`scope=visible`):

| Memory field | Value |
|---|---|
| `content` | the comment body |
| `project` | the repository name |
| `topic` | `review` |
| `session_id` | the PR number |
| `source` | `Explicit` |
| `citations` | the comment's permalink (`Url` kind) |
| `evidence` | `Interpreted` |
| `created_at` | the comment's timestamp |

The permalink citation is the observed-tier anchor: whatever the
body claims, the Url points at the immutable thread.

## The wiring

A reusable workflow lives at
`Industrial-Algebra/IA-home/.github/workflows/review-ingest.yml`.
Per repo, add a caller on `pull_request: [closed]` (see this
repository's `.github/workflows/review-ingest.yml` for the pilot
wiring), with:

- `repo_name`: the repository
- `wall`: the org wall to land on
- `pr_number`: the PR (the caller passes `${{ github.event.pull_request.number }}`)
- three forwarded secrets (reusable workflows inherit nothing from
  their source repo — the caller carries all three):
  `IJIMA_REVIEW_INGEST_TOKEN` (a principal holding `memory:write`
  and membership in that wall), `IJIMA_INGEST_URL` (the daemon base
  URL — topology-private, a secret never a literal), and
  `IJIMA_TS_AUTH_KEY` (the runner joins the tailnet for the job's
  duration via an ephemeral auth key).

Re-runs are idempotent by construction: identical comment content
hits the content-hash dedup (a `409 duplicate` is success to the
workflow), and comment ids are stable across fetches.

## Boundaries (v1)

- `source` stays `Explicit`: a new enum variant would make rows
  written by 0.4.3 unreadable to a rolled-back 0.4.2 daemon — the
  version-skew lesson from 0.4.1.
- Finding-vs-disposition linkage (supersedes between a finding and
  its disposition) is a later refinement; v1 ingests the flat
  thread.
- Ingestion happens at PR close — edits during a review land with
  the final sweep's content.
