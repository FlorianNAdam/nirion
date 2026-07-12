{
  pkgs,
  commit-message ? import ./commit-message.nix { inherit pkgs; },
}:

pkgs.writeShellApplication {
  name = "check-commit-messages";
  runtimeInputs = with pkgs; [
    coreutils
    git
  ];
  text = ''
    if [ "$#" -ne 1 ]; then
      echo "Usage: check-commit-messages <rev-range>" >&2
      echo "Example: check-commit-messages origin/main..HEAD" >&2
      exit 2
    fi

    range="$1"
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT
    failed=0

    while IFS= read -r commit; do
      msg_file="$tmp_dir/$commit"
      git log -1 --format=%B "$commit" > "$msg_file"

      if ! ${commit-message.checkFile} "$msg_file"; then
        subject="$(git log -1 --format=%s "$commit")"
        echo "Invalid commit message in $commit: $subject" >&2
        failed=1
      fi
    done < <(git rev-list "$range")

    exit "$failed"
  '';
}
