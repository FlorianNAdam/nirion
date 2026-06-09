{
  lib,
  pkgs,
  nirionPkg,
  nirionEnvVars,
}:

pkgs.stdenv.mkDerivation {
  name = "nirion";

  src = "${nirionPkg}";

  buildInputs = [ pkgs.makeWrapper ];

  installPhase =
    let
      wrapperFlags = lib.concatStringsSep " " (
        lib.mapAttrsToList (name: value: "--set-default ${name} ${value}") nirionEnvVars
      );
    in
    ''
      mkdir -p $out/bin
      makeWrapper ${nirionPkg}/bin/nirion $out/bin/nirion ${wrapperFlags}

      patch() {
        local f="$1"
        [ -f "$f" ] || return 0

        sed -i \
          's|/nix/store/[^[:space:]]*/bin/nirion|'"$out"'/bin/nirion|g' \
          "$f"
      }

      # Fish completion
      mkdir -p $out/share/fish/vendor_completions.d
      COMPLETE=fish $out/bin/nirion > $out/share/fish/vendor_completions.d/nirion.fish
      patch $out/share/fish/vendor_completions.d/nirion.fish
    '';
}
