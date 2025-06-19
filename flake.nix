{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";

      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";

    n2c = {
      url = "github:nlewo/nix2container";

      inputs.flake-utils.follows = "utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    nixpkgs,
    utils,
    rust-overlay,
    n2c,
    crane,
    ...
  }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;

        overlays = [
          (import rust-overlay)
          (_final: _prev: {
            inherit (n2c.packages.${system}) nix2container;
          })
        ];
      };

      rust = pkgs.pkgsBuildHost.rust-bin.beta.latest.default.override {
        extensions = ["rust-analyzer" "rust-src"];
      };

      cl = (crane.mkLib pkgs).overrideToolchain rust;

      templateFilter = path: _type: builtins.match ".*\.jinja$" path != null;
      styleFilter = path: _type: builtins.match ".*\.scss" path != null;

      mineOrCargo = path: type: (templateFilter path type) || (styleFilter path type) || (cl.filterCargoSources path type);

      src = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = mineOrCargo;
        name = "sources";
      };

      common = {
        inherit src;
        nativeBuildInputs = with pkgs; [
          rust
          mold
          clang
          perl
        ];

        cargoExtraArgs = "--locked --no-default-features --features production";
      };

      cargoArtifacts = cl.buildDepsOnly common;
      bin = cl.buildPackage {
        inherit (common) src nativeBuildInputs cargoExtraArgs;

        inherit cargoArtifacts;
      };
    in {
      devShells.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          lldb
          just
          rust
          bacon
          mold
          cargo-watch
          cargo-audit

          cmake
          clang
          pkg-config
        ];

        IRZEAN_PORT = "1339";
        IRZEAN_CLONE_PATH = "/tmp/irzean-writings";
      };

      packages = {
        default = bin;

        dockerImage = pkgs.nix2container.buildImage {
          name = "git.cutie.zone/lyssieth/irzean";
          tag = "latest";

          maxLayers = 20;
          copyToRoot = pkgs.buildEnv {
            name = "root";

            paths = [bin pkgs.cacert pkgs.dumb-init];
            pathsToLink = ["/bin" "/etc"];
          };

          config = {
            Entrypoint = ["${pkgs.dumb-init}/bin/dumb-init" "--"];
            Cmd = ["${bin}/bin/irzean"];
          };
        };
      };
    });
}
