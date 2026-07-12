{ pkgs }:

let
  lib = pkgs.lib;

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
  checkFile = pkgs.writeShellScript "check-commit-message" ''
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
{
  inherit
    allowedCommitTypes
    allowedScopes
    checkFile
    regex
    ;
}
