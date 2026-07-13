{
  nixCoverage,
  pkgs,
  rustCoverage,
}:

pkgs.writeShellApplication {
  name = "full-coverage";
  runtimeInputs = with pkgs; [
    coreutils
    lcov
  ];
  text = ''
    rm -f coverage-rust.info coverage-nix.info coverage-merged.info

    ${rustCoverage}/bin/rust-coverage
    ${nixCoverage}/bin/nix-coverage

    lcov -a coverage-rust.info -a coverage-nix.info -o coverage-merged.info
  '';
}
