{
  normalizeCoverage,
  pkgs,
}:

pkgs.writeShellApplication {
  name = "merge-coverage";
  runtimeInputs = with pkgs; [
    coreutils
    lcov
  ];
  text = ''
    rm -f coverage-merged.info
    lcov -a coverage-rust.info -a coverage-nix.info -o coverage-merged.info
    ${normalizeCoverage}/bin/normalize-coverage coverage-merged.info
  '';
}
