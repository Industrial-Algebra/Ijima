# Namespaces & Multi-Tenancy

Every request to Ijima is scoped to a **namespace** — the isolation unit.
No request spans namespaces; there is no implicit cross-namespace read.

## Namespace classes

| Class | Shape | Who sees it |
|---|---|---|
| Personal | `ns_<principal>_private` | The principal only |
| Shared | `ns_<org>_shared` (e.g. `ns_ia_shared`) | Members (membership-gated) |
| Import | `ns_import_<source>` | Imported corpus staging, per source |
| Global commons | `global` | Legacy migration baseline; readable |

## Routing

- Omit the namespace → your personal namespace.
- Pass `?namespace=<ns>` → that namespace, subject to checks:
  another principal's `*_private` namespace is always forbidden, and
  **shared namespaces require membership** (granted by an admin;
  admins bypass).
- Open namespaces — `global`, `ns_doctrine`, `ns_import_*` staging —
  are readable/writable by any authenticated principal.
- Writes require `memory:write` (or `knowledge:write`) *and* namespace
  eligibility; reads are personal-by-default and explicit otherwise.
- Promotion targets go through the same rule (`trust:promote` cannot
  tunnel through an org wall; import staging is not a promotion target).

Shared-namespace **membership** (org walls like `ns_ia_shared`,
`ns_kellas_shared`) is runtime-managed store data, not static policy —
grants take effect immediately for principals holding valid grants:

```bash
ijima namespace grant ns_ia_shared elliott --auth <admin-bearer>
ijima namespace members ns_ia_shared --auth <admin-bearer>
ijima namespace revoke ns_ia_shared elliott --auth <admin-bearer>
```

Admins bypass membership checks (operator access). See the
namespace-membership ADR.

## Isolation mechanics

Isolation is enforced at two layers:

1. **API layer** — `resolve_ns` rejects foreign private namespaces before
   the store is touched.
2. **Store layer** — SurrealDB record keys are namespaced composites
   (`<namespace>:<memory-id>`), so the same logical id can exist in two
   namespaces without collision. This matters for imports: the same
   pi-mempalace row imported from two workstations coexists per-source.

## Import namespaces

`ijima import` defaults each source to `ns_import_<sanitized-source>` —
never the global commons. A source name like `Laptop 01` becomes
`ns_import_laptop_01`. Import namespaces are staging areas: content lands
at the `AutoCapture` trust tier and is promoted into personal/shared
namespaces only after review (see [Provenance](./provenance.md)).

## Namespace-aware surfaces

The palace features mirror the memory store's scoping: rooms and taxonomy
are computed per namespace; the knowledge graph is namespaced per
triple/entity; sessions and diaries carry their namespace. The
`/palace/graph` and `/tunnel` traversals operate within one namespace's
view by design.

## What a search "sees"

`POST /memories/search` takes a `scope`:

- `personal` (default): the resolved namespace only — the caller's
  private namespace, or the `?namespace=` override.
- `visible`: **the principal's readable world** — own private + the
  `global` commons + open `ns_import_*` staging + every org wall they
  hold membership in, merged and ranked by similarity across all of
  them. The pi extension searches `visible` so a fresh session finds
  the whole brain, not just its own (initially empty) namespace.

Membership still gates: a wall the principal is absent from never
  appears in `visible` results.

## Agent homes

Personal-by-default is right for private notes — and exactly wrong for
fleet-institutional knowledge, which fragments across per-host silos
where sibling agents can't recall it. **Agent homes** (0.2.5) point the
default the other way: clients configured with a home namespace
(`IJIMA_NAMESPACE`, typically an org wall like `ns_ia_shared`) capture,
save, dedup-check, *and wake up* in that namespace.

The rules compose cleanly:

- Captures/saves in the home follow the wall's membership — every
  member agent writes to and reads from one shared pool.
- Wake-up essentials come from the home: a memory saved on host A
  greets the next session on host B.
- Another org's agents set their own wall as home; membership gating
  applies as always. Homes never widen access — they relocate the
  default.
- Unset home = personal namespace: private-by-default stays available
  for anything that genuinely is per-principal.
