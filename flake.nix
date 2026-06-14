{
  description = ''
    flowghetti — exporte le code Terraform glowwiththeflow en graphes Graphviz.
    Analyse statique du HCL, aucun terraform/AWS requis.
  '';

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      flake.homeModules.default = import ./nix/hm-module.nix;

      perSystem = {pkgs, ...}: {
        formatter = pkgs.alejandra;

        packages =
          {
            default = pkgs.callPackage ./nix/package.nix {};
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
            # Statically-linked (musl) binary for distribution.
            static = pkgs.pkgsStatic.callPackage ./nix/package.nix {};
          };

        devShells.default = pkgs.mkShell {
          name = "flowghetti";
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            just
            graphviz
            git
          ];
          env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          shellHook = ''
            echo ""
            echo "  flowghetti — terraform flows → graphviz"
            echo "  rustc $(rustc --version | cut -d' ' -f2), just, graphviz prêts."
            echo "  just ci"
            echo ""
          '';
        };
      };
    };
}
