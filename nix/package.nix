{
  naersk-lib,
  pkgs,
}:

let
  inherit (pkgs) lib;
  root = ../.;
  rustSource = lib.cleanSourceWith {
    src = root;
    filter =
      path: type:
      let
        rel = lib.removePrefix "${toString root}/" (toString path);
      in
      rel == "Cargo.toml"
      || rel == "Cargo.lock"
      || lib.hasPrefix "nirion-bin/" rel
      || lib.hasPrefix "nirion-lib/" rel
      || lib.hasPrefix "nirion-oci-lib/" rel
      || lib.hasPrefix "nirion-tui-lib/" rel
      || (
        type == "directory"
        && builtins.elem rel [
          "nirion-bin"
          "nirion-lib"
          "nirion-oci-lib"
          "nirion-tui-lib"
        ]
      );
  };
in
naersk-lib.buildPackage {
  pname = "nirion";
  src = rustSource;
  doCheck = false;

  buildInputs = with pkgs; [
    makeWrapper
  ];

  postInstall = ''
    # Bash completion
    mkdir -p $out/share/bash-completion/completions
    COMPLETE=bash $out/bin/nirion > $out/share/bash-completion/completions/nirion

    # Zsh completion
    mkdir -p $out/share/zsh/site-functions
    COMPLETE=zsh $out/bin/nirion > $out/share/zsh/site-functions/_nirion

    # Fish completion
    mkdir -p $out/share/fish/vendor_completions.d
    COMPLETE=fish $out/bin/nirion > $out/share/fish/vendor_completions.d/nirion.fish
  '';

  passthru = {
    tests.rust = naersk-lib.buildPackage {
      pname = "nirion-rust-tests";
      src = rustSource;
      mode = "test";
      nativeBuildInputs = [ pkgs.cacert ];
      cargoTestOptions = options: options ++ [ "--workspace" ];
    };
  };
}
