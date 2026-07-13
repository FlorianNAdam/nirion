{ pkgs }:

pkgs.writeShellApplication {
  name = "merge-coverage";
  runtimeInputs = with pkgs; [ lcov ];
  text = ''
    rm -f coverage-merged.info
    lcov -a coverage-rust.info -a coverage-nix.info -o coverage-merged.info
  '';
}
