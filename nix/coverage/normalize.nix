{ pkgs }:

pkgs.writeShellApplication {
  name = "normalize-coverage";
  runtimeInputs = with pkgs; [
    coreutils
    gawk
  ];
  text = ''
    if [ "$#" -ne 1 ]; then
      echo "usage: normalize-coverage <lcov-file>" >&2
      exit 2
    fi

    lcov_file=$1
    tmp_file="$lcov_file.tmp"

    awk -v prefix="SF:$PWD/" '
      index($0, prefix) == 1 {
        $0 = "SF:" substr($0, length(prefix) + 1)
      }

      { print }
    ' "$lcov_file" > "$tmp_file"

    mv "$tmp_file" "$lcov_file"
  '';
}
