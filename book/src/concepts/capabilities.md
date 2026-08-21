# Capabilities & GrantTokens

Ijima's access model is [Schubert](https://github.com/Industrial-Algebra/Schubert)
capability algebra on the Grassmannian **Gr(4,8)**. Capabilities are not
role strings — they are Schubert conditions (subspaces), and *access
decisions are geometry*: a grant authorizes an action when the grant's
subspace intersects the capability's.

## The vocabulary

Eleven capabilities, each a partition on Gr(4,8):

| Capability | Codimension | Governs |
|---|---|---|
| `memory:read` | 1 | Read/recall/search memories |
| `knowledge:read` | 1 | Query the knowledge graph |
| `mining:review` | 2 | Read the review queue |
| `memory:write` | 2 | Store/delete memories, diary |
| `knowledge:write` | 2 | Write triples |
| `session:ingest` | 3 | Ingest session turns |
| `mining:trigger` | 3+1 | Trigger mining runs |
| `trust:promote` | 3+1 | Raise a memory's trust tier |
| `trust:endorse` | 4+1 | Cross-tier endorsement |
| `trust:override` | 4+2 | Authority override (rare) |
| `admin` | 4,4,4,4 (point class) | Token admin, status, repos |

**Write implies read** in the geometry: `memory:write`'s partition
contains `memory:read`'s, so a write grant can also read. A read grant
can never write.

## GrantTokens

A **GrantToken** is a compact, proof-carrying, ed25519-signed token that
bundles one or more capabilities for one principal:

```
ijima token issue --principal sara \
    --capabilities memory:read,memory:write,knowledge:read
```

- **Multi-capability** — one grant covers a whole job description; no
  more per-capability token bundles.
- **Verified geometrically** — the daemon decodes the grant, verifies the
  signature, and checks each request's capability against the grant's
  partition set.
- **Partition-signed** — the capability list is inside the signed blob;
  clients cannot edit grants.
- **Revocable** — see [Token Management](../guide/tokens.md).

## Why geometry

The codimension of a capability is a *quantitative* authorization weight —
and it doubles as the rate-limit capacity (intersection-number-scaled
token buckets). Low-stakes capabilities (`memory:read`, codim 1) are
cheap; trust-flow capabilities (`trust:override`) are expensive; `admin`
is the point class. The geometry of access is also the geometry of
throughput, and trust-flow costs more than access — deliberately.

The vocabulary is declarative (`policy.toml` in the daemon crate); adding
a capability is a policy edit plus a partition assignment, not a code
change.
