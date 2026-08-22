# NixOS

Ijima ships a Nix flake: a package (the `ijima` binary built from the
repo's own source, with the exact nightly toolchain the release was
verified on) and a NixOS service module.

## Quickstart

```bash
nix run github:Industrial-Algebra/Ijima -- --help   # try the CLI
nix build github:Industrial-Algebra/Ijima           # the package
```

## As a flake input

In your NixOS flake:

```nix
# flake.nix
inputs.ijima.url = "github:Industrial-Algebra/Ijima";  # or a tag: .../v0.2.1

# host config
{
  inputs,
  pkgs,
  ...
}:
{
  imports = [ inputs.ijima.nixosModules.ijima ];

  services.ijima = {
    enable = true;
    package = inputs.ijima.packages.x86_64-linux.ijima;
    # dataDir = "/var/lib/ijima";   # default; keep on fast storage
    # port = 7373;                  # default
    # user = "ijima";               # default: isolated system user
  };
}
```

`nixos-rebuild switch` and the daemon is up on `0.0.0.0:7373` (adjust
`bindAddress`). First boot downloads the embedding model (~90 MB) into
`dataDir/hf`; every boot after is offline.

## Options

| Option | Default | Notes |
|---|---|---|
| `enable` | `false` | |
| `package` | — | `packages.x86_64-linux.ijima` from this flake |
| `dataDir` | `/var/lib/ijima` | Store, issuer key, policy overlay, model cache. SurrealKv does random IO — SSD/NVMe. |
| `bindAddress` | `0.0.0.0` | The daemon trusts the network boundary: firewall it, or put a TLS terminator in front |
| `port` | `7373` | |
| `user` | `ijima` (auto-created) | Set to your own username to let the local CLI share the daemon's issuer key — token minting and imports without sudo |
| `memoryMax` | `8G` | Raise if you run mining |

The unit is hardened ( ProtectSystem=strict, NoNewPrivileges, empty
capability set, … — mirrors `deploy/ijima.service`) with write access
only to `dataDir`.

## After the first boot

The state directory starts with a fresh issuer key. Mint operator grants
from the same host so the CLI shares the daemon's key:

```bash
IJIMA_DIR=/var/lib/ijima sudo -u ijima ijima token issue \
  --principal <name> --capabilities memory:read,memory:write \
  --policy /var/lib/ijima/policy.toml
```

(If `user` is your own account, drop the `sudo -u`.) Then continue with
the [daemon guide](daemon.md) and the [runbook](deploy.md).
