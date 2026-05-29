{
  description = "unison-fsmonitor";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, flake-utils, nixpkgs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      {
        packages = {
          default = import ./default.nix { inherit pkgs; };
        };

        devShells = {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo-release
              rustup
            ];
          };
        };
      }
    );
}
