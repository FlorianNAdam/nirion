{ pkgs, ... }:

pkgs.runCommand "nirion-runtime-micro-json" { nativeBuildInputs = [ pkgs.perl ]; } ''
  perl ${./micro-json.pl} ${../../nix/module/lib/micro-json.pl}
  touch $out
''
