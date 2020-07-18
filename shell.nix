{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    cargo
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
    CoreServices
  ]);

  NIX_LDFLAGS = pkgs.lib.optionalString pkgs.stdenv.isDarwin "-framework CoreFoundation";

  RUST_BACKTRACE = 1;
}
