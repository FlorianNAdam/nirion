{
  git-hooks-nix,
  pkgs,
  system,
}:

let
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

  commit-message = import ./commit-message.nix { inherit pkgs; };
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
      entry = "${commit-message.checkFile}";
    };
  };
}
