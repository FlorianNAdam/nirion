{
  pkgs,
  commit-message ? import ./commit-message.nix { inherit pkgs; },
}:

pkgs.writeShellApplication {
  name = "check-message";
  runtimeInputs = with pkgs; [
    coreutils
  ];
  text = ''
    if [ "$#" -ne 1 ]; then
      echo "Usage: check-message <message>" >&2
      echo "Example: check-message 'docs: add README badges'" >&2
      exit 2
    fi

    msg_file="$(mktemp)"
    trap 'rm -f "$msg_file"' EXIT

    printf '%s\n' "$1" > "$msg_file"
    exec ${commit-message.checkFile} "$msg_file"
  '';
}
