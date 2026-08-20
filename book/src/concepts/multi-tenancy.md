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
  another principal's `*_private` namespace is always forbidden.
- Writes require `memory:write` (or `knowledge:write`) *and* namespace
  eligibility; reads are personal-by-default and explicit otherwise.

Shared-namespace **membership** (org walls like `ns_ia_shared`,
`ns_kellas_shared`) is runtime-managed data, not static policy — see the
design decision log.

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
