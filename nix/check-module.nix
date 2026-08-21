# Copyright (C) 2026 Industrial Algebra
# SPDX-License-Identifier: Apache-2.0

# Eval-only check: the module must integrate into a real NixOS evaluation
# with the flake-built package, and produce a sane unit. Returned as a
# derivation whose text is the generated ExecStart line + environment, so
# `nix flake check` (which builds checks) fails if evaluation breaks.
{
  writeText,
  ijima,
  module,
  nixpkgs,
}:
let
  eval = import (nixpkgs + "/nixos/lib/eval-config.nix") {
    system = "x86_64-linux";
    modules = [
      module
      (
        { ... }:
        {
          services.ijima = {
            enable = true;
            package = ijima;
            dataDir = "/var/lib/ijima";
          };
        }
      )
    ];
  };
  unit = eval.config.systemd.services.ijima;
in
writeText "ijima-module-check" ''
  ExecStart: ${unit.serviceConfig.ExecStart}
  IJIMA_DIR: ${unit.environment.IJIMA_DIR}
  user: ${unit.serviceConfig.User}
''
