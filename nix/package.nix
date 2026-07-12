{
  naersk-lib,
  pkgs,
}:

let
  inherit (pkgs) lib;
  root = ../.;

  workspace = fromTOML (builtins.readFile (root + "/Cargo.toml"));
  crates = workspace.workspace.members;

  crateVersions = map (
    name: (fromTOML (builtins.readFile (root + "/${name}/Cargo.toml"))).package.version
  ) crates;

  version =
    assert lib.assertMsg (lib.all (v: v == lib.head crateVersions)
      crateVersions
    ) "all crate versions must be equal, got: ${lib.concatStringsSep ", " crateVersions}";
    lib.head crateVersions;

  rustSource = lib.cleanSourceWith {
    src = root;
    filter =
      path: type:
      let
        rel = lib.removePrefix "${toString root}/" (toString path);
      in
      rel == "Cargo.toml"
      || rel == "Cargo.lock"
      || lib.any (crate: lib.hasPrefix "${crate}/" rel) crates
      || (type == "directory" && builtins.elem rel crates);
  };
in
naersk-lib.buildPackage {
  pname = "nirion";
  inherit version;
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
