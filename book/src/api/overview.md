# HTTP API Overview

Ijima's stable contract is a REST/JSON surface; every route is guarded by a
Schubert capability check and scoped to a namespace. The typed client is
`ijima-client`.

## Route map (selected)

| Method | Path | Capability |
|---|---|---|
| `GET` | `/health` | (none) |
| `POST` | `/memories` | `memory:write` |
| `GET` | `/memories/:id` | `memory:read` |
| `POST` | `/memories/search` | `memory:read` |
| `POST` | `/memories/check` | `memory:read` |
| `POST` | `/memories/:id/promote` | `trust:promote` |
| `GET` | `/wakeup` | `memory:read` |
| `POST` | `/kg/triples` | `knowledge:write` |
| `GET` | `/kg/entities/:id` | `knowledge:read` |
| `POST` | `/sessions/:id/turns` | `session:ingest` |
| `POST` | `/sessions/:id/mine` | `mining:trigger` |
| `GET` | `/mining/queue` | `mining:review` |
| `GET` | `/repos/resolve?path=` | `memory:read` |
| `GET` | `/status` | `memory:read` |

The full table (with the store method each route maps to) lives in the
daemon crate's `api` module doc comment.

## Capabilities

Eleven capabilities, each a Schubert partition on **Gr(4,8)** (dimension
16): `memory:read`/`write`, `knowledge:read`/`write`, `mining:review`/
`trigger`, `session:ingest`, `trust:promote`/`endorse`/`override`, and
`admin` (the point class σ₄₄₄₄). Tokens are proof-carrying (one capability
each). The vocabulary is declarative — see
[`policy/policy.toml`](https://github.com/Industrial-Algebra/Ijima/blob/develop/policy/policy.toml).

## Client

For harnesses, depend on `ijima-client` (typed async HTTP) rather than
hand-rolling requests.
