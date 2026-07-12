{
  git-hooks-nix,
  pkgs,
  system,
}:

let
  lib = pkgs.lib;

  cargo-lock = pkgs.writeShellApplication {
    name = "cargo-lock";
    runtimeInputs = with pkgs; [
      cargo
      rustc
    ];
    text = ''
      exec cargo generate-lockfile
    '';
  };

  flake-lock = pkgs.writeShellApplication {
    name = "flake-lock";
    runtimeInputs = [ pkgs.nix ];
    text = ''
      exec nix --extra-experimental-features 'nix-command flakes' flake lock
    '';
  };

  commit-message =
    let
      allowedCommitTypes = [
        "feat"
        "fix"
        "tests"
        "nix"
        "ci"
        "docs"
        "refactor"
        "tooling"
        "chore"
      ];
      allowedScopes = [
        "[a-z0-9_-]+"
      ];
      typeRegex = lib.concatStringsSep "|" allowedCommitTypes;
      scopeRegex = lib.concatStringsSep "|" allowedScopes;
      regex = "^(${typeRegex})(\\((${scopeRegex})\\))?: .+";
      script = pkgs.writeShellScript "check-commit-message" ''
        commit_msg_file="$1"
        first_line="$(head -n1 "$commit_msg_file")"
        regex=${lib.escapeShellArg regex}

        if ! [[ "$first_line" =~ $regex ]]; then
          echo "ERROR: Commit message must match: $regex" >&2
          echo "Allowed commit types: ${lib.concatStringsSep ", " allowedCommitTypes}" >&2
          echo "Allowed scopes: ${lib.concatStringsSep ", " allowedScopes}" >&2
          exit 1
        fi
      '';
    in
    script;
in
git-hooks-nix.lib.${system}.run {
  src = ../.;
  hooks = {
    cargo-lock = {
      enable = true;
      name = "Update Cargo.lock";
      package = cargo-lock;
      entry = "${cargo-lock}/bin/cargo-lock";
      files = "(^|/)Cargo\\.toml$|^Cargo\\.lock$";
      pass_filenames = false;
    };
    flake-lock = {
      enable = true;
      name = "Update flake.lock";
      package = flake-lock;
      entry = "${flake-lock}/bin/flake-lock";
      files = "^(flake\\.nix|flake\\.lock)$";
      pass_filenames = false;
    };
    rustfmt = {
      enable = true;
    };
    commit-message = {
      enable = true;
      name = "Check commit message";
      stages = [ "commit-msg" ];
      entry = "${commit-message}";
    };
  };
}
