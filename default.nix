{ pkgs ? import <nixpkgs> {} }:

pkgs.rustPlatform.buildRustPackage {
  pname = "unison-fsmonitor";
  version = "0.2.3";
  src = ./.;
  cargoSha256 = "0xj5hincwm3zr4bkkcf60971595fl9cjib7kf5pil7x75f29l8gj";

  buildInputs = pkgs.stdenv.lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.CoreServices
  ];
}
