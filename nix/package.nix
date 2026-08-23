# Copyright (C) 2026 Industrial Algebra
# SPDX-License-Identifier: Apache-2.0

# ijima — the Anima centralized memory daemon, built from this repository.
#
# The flake input pins nothing: this file builds the source it lives in
# (`src` is the flake's own source). The Cargo.lock at the repo root is
# the lock. For a tagged-release build, use the tag's tree.
#
# Toolchain note: Ijima builds with a pinned nightly (committed manifest
# beside this file). nixpkgs' stable rustc (1.96) mis-selects diskann's
# AVX-512 VNNI intrinsic ("Cannot select: %llvm.x86.avx512.vpdpwssd.512");
# the pinned channel compiles it cleanly. `fromManifest` keeps the pin
# pure-eval-safe (toolchainOf's fetchurl is impure).
{
  lib,
  stdenv,
  makeRustPlatform,
  openssl,
  pkg-config,
  patchelf,
  fenix,
  src,
}:
let
  toolchain = fenix.packages.x86_64-linux.fromManifest
    (lib.importTOML ./toolchain-manifest.toml);
  rustPlatform = makeRustPlatform {
    rustc = toolchain.rustc;
    cargo = toolchain.cargo;
  };
in
rustPlatform.buildRustPackage rec {
  pname = "ijima";
  version = "0.2.3";

  inherit src;

  # The single `ijima` bin: daemon + CLI (token issue/revoke, import, export).
  # Full deployment feature set. `backend-sqlite` is import-only (reads
  # legacy pi-mempalace corpora); `tls` is built but optional (a TLS
  # terminator can front the daemon instead).
  nativeBuildInputs = [
    pkg-config
    patchelf
  ];
  buildInputs = [ openssl ];

  # The stable-rustc LLVM mis-selection (see file header) also fires under
  # LTO; disabling it avoids the codegen path entirely. Perf delta for a
  # memory daemon is noise.
  env.CARGO_PROFILE_RELEASE_LTO = "off";
  env.CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16";

  buildFeatures = [
    "cli"
    "http"
    "backend-surreal"
    "backend-sqlite"
    "server-auth"
    "rate-limit"
    "embeddings-candle"
    "mining"
    "tls"
  ];

  cargoLock.lockFile = ../Cargo.lock;
  # The lock references workspace members by path — all present in src.
  cargoLock.allowBuiltinFetchGit = true;

  # The fenix toolchain's rustc link line doesn't inherit the cc-wrapper's
  # rpath for openssl; pin it explicitly so the ELF resolves libssl.so.3.
  preFixup = ''
    rpaths="${lib.makeLibraryPath [ openssl stdenv.cc.cc.lib ]}"
    patchelf --add-rpath "$rpaths" $out/bin/ijima
    if [ -e "$out/lib/libijima_pi.so" ]; then
      patchelf --add-rpath "$rpaths" $out/lib/libijima_pi.so
    fi
  '';

  # Tests need feature-specific tooling and (for embeddings) a model
  # download; CI gates them upstream. Build-verify only here.
  doCheck = false;

  meta = with lib; {
    description = "Centralized agentic memory service for the Anima ecosystem";
    homepage = "https://ijima.industrialalgebra.com";
    license = licenses.asl20;
    mainProgram = "ijima";
    platforms = platforms.linux;
  };
}
