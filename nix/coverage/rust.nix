{
  normalizeCoverage,
  pkgs,
}:

let
  tarpaulinConfig = (pkgs.formats.toml { }).generate "tarpaulin.toml" {
    coverage = {
      workspace = true;
      skip-clean = true;
      timeout = "20m";
      locked = true;
      out = [
        "Stdout"
        "Lcov"
      ];
      engine = "Llvm";
    };
  };
in

pkgs.writeShellApplication {
  name = "rust-coverage";
  runtimeInputs = with pkgs; [
    cargo
    cargo-tarpaulin
    coreutils
    distribution
    openssl
    pkg-config
    rustc
  ];
  text = ''
    cargo tarpaulin \
      --config ${tarpaulinConfig} \
      --root "$PWD" \
      --target-dir "$PWD/target/tarpaulin" \
      --output-dir "$PWD/target/coverage/rust" \
      "$@"

    for arg in "$@"; do
      case "$arg" in
        -h|--help|-V|--version)
          exit 0
          ;;
      esac
    done

    if [ ! -f target/coverage/rust/lcov.info ]; then
      echo "Tarpaulin did not write target/coverage/rust/lcov.info" >&2
      exit 1
    fi

    cp target/coverage/rust/lcov.info coverage-rust.info
    ${normalizeCoverage}/bin/normalize-coverage coverage-rust.info
  '';
}
