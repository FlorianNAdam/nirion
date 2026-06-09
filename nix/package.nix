{
  naersk-lib,
  pkgs,
}:

naersk-lib.buildPackage {
  pname = "nirion";
  src = ../.;

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
}
