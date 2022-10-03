{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup
    cargo-release
  ]
  ++ pkgs.lib.optionals pkgs.stdenvNoCC.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
    libiconv
    CoreServices
  ]);

  NIX_LDFLAGS = pkgs.lib.optionalString pkgs.stdenvNoCC.isDarwin "-framework CoreFoundation";
}
