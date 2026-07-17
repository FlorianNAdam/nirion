{
  naersk,
  pkgs,
}:

let
  nixcovSrc = pkgs.fetchFromGitHub {
    owner = "FlorianNAdam";
    repo = "nixcov";
    rev = "44c466dc69d3517301b8f46159faa3e33b702104";
    hash = "sha256-IXivYZpccieuibiU7tQjEEQ1JnnEJuVq7KS7B9Y66OI=";
  };

  nixcov = import nixcovSrc { inherit pkgs naersk; };
in
pkgs.writeShellApplication {
  name = "nix-coverage";
  text = ''
    exec ${nixcov}/bin/nixcov check --summary files --lcov coverage-nix.info "$@"
  '';
}
