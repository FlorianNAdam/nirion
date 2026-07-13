{
  nixcovProgram,
  pkgs,
}:

pkgs.writeShellApplication {
  name = "nix-coverage";
  text = ''
    exec ${nixcovProgram} --lcov coverage-nix.info "$@"
  '';
}
