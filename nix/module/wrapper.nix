{
  lib,
  pkgs,
  nirionPkg,
  envVars,
}:

pkgs.runCommand "nirion" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
  mkdir -p $out/bin
  makeWrapper ${nirionPkg}/bin/nirion $out/bin/nirion ${
    lib.concatStringsSep " " (
      lib.mapAttrsToList (
        name: value: "--set-default ${name} ${lib.escapeShellArg (toString value)}"
      ) envVars
    )
  }

  patch_completion() {
    local f="$1"
    [ -f "$f" ] || return 0

    sed -i \
      's|/nix/store/[^[:space:]]*/bin/nirion|'"$out"'/bin/nirion|g' \
      "$f"
  }

  mkdir -p $out/share/fish/vendor_completions.d
  COMPLETE=fish $out/bin/nirion > $out/share/fish/vendor_completions.d/nirion.fish
  patch_completion $out/share/fish/vendor_completions.d/nirion.fish
''
