{
  mergeCoverage,
  nixCoverage,
  pkgs,
  rustCoverage,
}:

pkgs.writeShellApplication {
  name = "full-coverage";
  runtimeInputs = with pkgs; [ coreutils ];
  text = ''
    rm -f coverage-rust.info coverage-nix.info coverage-merged.info

    ${rustCoverage}/bin/rust-coverage
    ${nixCoverage}/bin/nix-coverage

    ${mergeCoverage}/bin/merge-coverage
  '';
}
