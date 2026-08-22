{
  description = "Ijima — centralized agentic memory service for the Anima ecosystem";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # Rust toolchains by date — Ijima builds with a pinned nightly
    # (nix/toolchain-manifest.toml); nixpkgs' stable rustc mis-compiles
    # diskann's AVX-512 VNNI paths (see nix/package.nix).
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      ijima = pkgs.callPackage ./nix/package.nix {
        inherit fenix;
        src = self;
      };
      module = import ./nix/module.nix;
    in
    {
      packages.${system} = {
        inherit ijima;
        default = ijima;
      };

      nixosModules = {
        ijima = module;
        default = module;
      };

      checks.${system} = {
        # Real NixOS evaluation with the module enabled (eval-only; the
        # check writes the generated unit's ExecStart as its text).
        module-eval = pkgs.callPackage ./nix/check-module.nix {
          inherit ijima module;
          nixpkgs = nixpkgs;
        };
        package = ijima;
      };
    };
}
