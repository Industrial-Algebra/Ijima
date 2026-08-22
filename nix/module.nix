# Copyright (C) 2026 Industrial Algebra
# SPDX-License-Identifier: Apache-2.0

# NixOS module: Ijima memory daemon — the Anima "company brain".
#
# Runs a single central instance; harnesses (pi on workstations, other
# agents, machine feeds) connect over HTTP with Schubert GrantTokens.
# Multi-tenant: per-principal private namespaces + membership-gated
# shared org walls (see the book, "Multi-tenancy").
#
# State layout under dataDir:
#   ijima.db/      SurrealDB (surrealkv) — the memory palace + KG + sessions
#   issuer.key     Schubert GrantToken issuer seed — SECRET, never commit
#   policy.toml    issuance overlay ([principals.<name>] grants) — operator state
#   hf/            candle model cache (all-MiniLM-L6-v2, ~90 MB, fetched at
#                  first boot — needs network on the first start only)
#
# User choice: with the default `ijima` system user the daemon is fully
# isolated. Set `user` to your own username if the `ijima` CLI on the
# same host should share the daemon's issuer key (token minting and
# imports without sudo choreography) — that user must already exist.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.ijima;
in
{
  options.services.ijima = {
    enable = lib.mkEnableOption "Ijima memory daemon (Anima company brain)";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The ijima package to run.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/ijima";
      description = "State directory (store, issuer key, policy overlay, model cache). Keep on fast storage — SurrealKv does random IO.";
    };

    bindAddress = lib.mkOption {
      type = lib.types.str;
      default = "0.0.0.0";
      description = ''
        Bind address. The daemon has no transport auth boundary of its own
        beyond GrantTokens; the NixOS firewall (or a private network / TLS
        terminator in front) is the access control.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 7373;
      description = "Listen port.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "ijima";
      description = ''
        Service user. The default creates an isolated `ijima` system user.
        Set to an existing user to let the local `ijima` CLI share the
        daemon's issuer key (see the module header).
      '';
    };

    memoryMax = lib.mkOption {
      type = lib.types.str;
      default = "8G";
      description = "MemoryMax for the unit (candle mmaps ~90 MB; mining raises this).";
    };
  };

  config = lib.mkIf cfg.enable {
    # The default service user is created for you; a custom user is
    # expected to exist already (it belongs to the operator's account).
    users.users.${cfg.user} = lib.mkIf (cfg.user == "ijima") {
      isSystemUser = true;
      group = "ijima";
      description = "Ijima memory daemon service user";
    };
    users.groups.ijima = lib.mkIf (cfg.user == "ijima") { };

    systemd.services.ijima = {
      description = "Ijima memory daemon (Anima company brain)";
      documentation = [ "https://ijima.industrialalgebra.com" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      environment = {
        IJIMA_DIR = cfg.dataDir;
        IJIMA_HOST = cfg.bindAddress;
        IJIMA_PORT = toString cfg.port;
        IJIMA_LOG = "ijima=info";
        # Persist the candle model cache inside the data dir (survives
        # restarts; downloaded once).
        HF_HOME = "${cfg.dataDir}/hf";
      };

      # The data dir may live on a root-owned mount (e.g. a ZFS dataset);
      # hand it to the service user. '+' prefix runs as root (once per boot).
      serviceConfig.ExecStartPre = [
        "+${pkgs.coreutils}/bin/install -d -m 0750 -o ${cfg.user} ${cfg.dataDir}"
        "+${pkgs.coreutils}/bin/install -d -m 0750 -o ${cfg.user} ${cfg.dataDir}/hf"
      ];

      serviceConfig = {
        Type = "simple";
        User = cfg.user;
        ExecStart = "${cfg.package}/bin/ijima serve";
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening (mirrors deploy/ijima.service): the daemon needs no
        # privileges beyond its data dir.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        ReadWritePaths = cfg.dataDir;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        LockPersonality = true;
        SystemCallArchitectures = "native";
        CapabilityBoundingSet = [ ];
        AmbientCapabilities = [ ];
        MemoryMax = cfg.memoryMax;
      };
    };
  };
}
