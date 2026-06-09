{ pkgs }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    openssl
  ];

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  packages = with pkgs; [
    rust-analyzer
  ];
}
