{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";

      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    nixpkgs,
    utils,
    rust-overlay,
    ...
  }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;

        overlays = [(import rust-overlay)];
      };

      rust = pkgs.pkgsBuildHost.rust-bin.beta.latest.default;
    in {
      devShells.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          lldb
          just
          rust
          bacon
          mold

          cmake
          clang
          pkg-config
        ];
      };
    });
}
