# Multi-Source Import

Consolidating two workstations' pi-mempalace corpora into one central
daemon — the WS2 workflow end to end.

## Scenario

- `elliotthall-laptop` and `kaiizen` each have a pi-mempalace
  `memories.db`; the same insight was sometimes saved on both.
- The central daemon runs at `ijima.tailnet:7373`.

## 1. Mint an import grant (once)

```bash
ijima token issue --principal importer \
    --capabilities memory:read,memory:write --json
export IJIMA_URL="http://ijima.tailnet:7373"
export IJIMA_TOKEN="<grant>"
```

## 2. Import each source

On each workstation (scp the db to the server, or run locally with
`IJIMA_URL` pointed over the tailnet):

```bash
ijima import mempalace --db ~/.pi/agent/mempalace/memories.db \
    --source "elliotthall-laptop"
# ijima: import `elliotthall-laptop` complete — 1190 added, 91 deduped, 3 skipped

ijima import mempalace --db /srv/kaiizen/memories.db --source "kaiizen"
# ijima: import `kaiizen` complete — 940 added, 150 deduped, 0 skipped
```

Each lands in its own staging namespace — `ns_import_elliotthall_laptop`
and `ns_import_kaiizen` — with `origin` stamped per source and every
memory at the `AutoCapture` tier, even rows the source called
`manual-save`.

The `deduped` counts are per-source content-hash collisions (the same
memory saved twice on one machine). Cross-source overlap is *preserved*
deliberately: both namespaces keep their copy, each tagged with its
origin, until you review.

## 3. Inspect per-source overlap

```bash
curl -s -H "Authorization: Bearer $TOKEN" \
     "localhost:7373/memories?namespace=ns_import_kaiizen&limit=50" \
  | jq '.[] | select(.origin=="kaiizen") | .content'
```

## 4. Promote what you trust

Review in the staging namespaces, then promote winners into personal or
shared namespaces — `trust:promote` grant required:

```bash
curl -s -X POST -H "Authorization: Bearer $TRUST_TOKEN" \
     "localhost:7373/memories/mem_9812/promote?namespace=ns_import_kaiizen"
```

The origin stamp travels with the promoted memory — the workstation
trail survives promotion.

## 5. Re-run safety

Imports are idempotent: re-running the same command reports the previous
`added` count as `deduped` and adds nothing. Schedule freely.
