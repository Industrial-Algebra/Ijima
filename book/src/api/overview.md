# HTTP API Overview

Ijima's stable contract is a REST/JSON surface; every route is guarded by
a Schubert capability check and scoped to a namespace. The typed client
is `ijima-client`. Namespace-sensitive routes accept `?namespace=<ns>`
(omit for the caller's personal namespace; foreign `*_private`
namespaces are always rejected).

## Route map (selected)

| Method | Path | Capability |
|---|---|---|
| `GET` | `/health` | (none) |
| `GET` | `/status` | `admin` |
| `POST` | `/memories[?namespace=]` | `memory:write` |
| `GET` | `/memories?namespace=&limit=` | `memory:read` (browse) |
| `GET` | `/memories/:id?namespace=` | `memory:read` |
| `DELETE` | `/memories/:id?namespace=` | `memory:write` |
| `POST` | `/memories/check?namespace=` | `memory:read` (dedup pre-check) |
| `POST` | `/memories/search` | `memory:read` |
| `POST` | `/memories/:id/promote` | `trust:promote` |
| `GET` | `/wakeup` | `memory:read` (top-N wake-up context) |
| `GET` | `/rooms`, `/taxonomy`, `/palace/graph`, `/tunnel` | `memory:read` |
| `POST` | `/kg/triples` | `knowledge:write` |
| `GET` | `/kg/entities/:id`, `/kg/timeline/:entity` | `knowledge:read` |
| `POST` | `/sessions`, `/sessions/:id/turns`, `/sessions/:id/end` | `session:ingest` |
| `POST` | `/sessions/:id/mine` | `mining:trigger` |
| `GET` | `/mining/queue`, decisions (accept/reject) | `mining:review` |
| `POST` | `/diaries` | `memory:write` |
| `GET` | `/repos/resolve?path=` | `memory:read` (RepoDirectory) |
| `POST` | `/tokens/revoke` | `admin` |
| `GET` | `/tokens/revocations` | `admin` |
| `GET` | `/federation/state`, routed-write, conflict-signal | `federation` feature |

The full table (with the store method each route maps to) lives in the
daemon crate's `api` module doc comment.

## Authentication

One header: `Authorization: Bearer <GrantToken>`. The grant bundles the
principal's capabilities; the daemon verifies the signature, checks
revocation, and evaluates each request's capability geometrically (see
[Capabilities](../concepts/capabilities.md)).

## Conventions

- Errors are JSON with a stable shape; `403` means the grant lacks the
  capability (or the namespace is off-limits), `401` means the bearer
  failed verification/revocation, `409` duplicate content on store.
- Provenance fields are accepted on write and echoed on read — the
  daemon stamps defaults (`created_at`, instance identity) where absent.
- All list endpoints are `limit`-bounded.

## Client

For harnesses, depend on `ijima-client` (typed async HTTP) rather than
hand-rolling requests — see [The Client Crate](./client.md).
