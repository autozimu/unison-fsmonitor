{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
    CoreServices
  ]);

  NIX_LDFLAGS = pkgs.lib.optionalString pkgs.stdenv.isDarwin "-framework CoreFoundation";
}
