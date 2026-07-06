---
id: doctrine-ijima-capabilities
project: ijima
topic: auth
---

Ijima's capability vocabulary (on Schubert Gr(4,8)):

| Capability | Kind | Grants |
|---|---|---|
| memory:read | ReadLike | recall, search, session turns |
| memory:write | WriteLike | store, delete, promote |
| knowledge:read | ReadLike | query entities/triples/timeline |
| knowledge:write | WriteLike | add/invalidate triples |
| session:ingest | WriteLike | append session-context turns |
| mining:trigger | WriteLike | trigger an extraction pass |
| mining:review | ReadLike | review the mining queue |
| admin | AdminLike (point) | full control; implies all above |

Tokens are Ed25519-signed (proof-carrying). The issuer key lives at
`$IJIMA_DIR/issuer.key`, shared by the CLI and daemon. Verify by
signature; the point class (admin) implies every capability.
