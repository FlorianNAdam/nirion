{
  naersk,
  pkgs,
}:

let
  nixcovSrc = pkgs.fetchFromGitHub {
    owner = "FlorianNAdam";
    repo = "nixcov";
    rev = "332d0df1b9f49dbf284ca6e064a48a744536b0a9";
    hash = "sha256-bgrrO7Dp+yiSH3peyNggyMRfY/6JaeWFDs31HkKNhss=";
  };

  nixcov = import nixcovSrc { inherit pkgs naersk; };
in
pkgs.writeShellApplication {
  name = "nix-coverage";
  text = ''
    exec ${nixcov}/bin/nixcov check --summary files --lcov coverage-nix.info "$@"
  '';
}
