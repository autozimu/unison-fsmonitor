{ pkgs ? import <nixpkgs> { } }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup
  ]
  ++ pkgs.lib.optionals pkgs.stdenvNoCC.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [
    CoreServices
  ]);

  NIX_LDFLAGS = pkgs.lib.optionalString pkgs.stdenvNoCC.isDarwin "-framework CoreFoundation";
}
