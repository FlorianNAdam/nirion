{ pkgs }:

pkgs.writeShellApplication {
  name = "coverage";
  runtimeInputs = with pkgs; [
    cargo
    cargo-tarpaulin
    openssl
    pkg-config
    rustc
  ];
  text = ''
    exec cargo tarpaulin --config tarpaulin.toml "$@"
  '';
}
