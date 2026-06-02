{
  pkgs ? import <nixpkgs> { },
}:
pkgs.mkShell {
  buildInputs = [
    pkgs.openssl.dev
    pkgs.pkg-config
  ];
}
