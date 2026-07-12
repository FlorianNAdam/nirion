{
  description = "Docker Compose management for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks-nix = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      git-hooks-nix,
      self,
      nixpkgs,
      flake-utils,
      naersk,
    }:

    flake-utils.lib.eachSystem
      [
        "x86_64-linux"
        "aarch64-linux"
      ]
      (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          naersk-lib = pkgs.callPackage naersk { };
          nirion = pkgs.callPackage ./nix/package.nix { inherit naersk-lib; };
          pre-commit = import ./nix/pre-commit.nix { inherit git-hooks-nix pkgs system; };
          pre-commit-app = pkgs.writeShellApplication {
            name = "pre-commit-run";
            runtimeInputs = [ pre-commit.config.package ] ++ pre-commit.enabledPackages;
            text = ''
              exec pre-commit run --config ${pre-commit.config.configFile} --all-files "$@"
            '';
          };
          check-commit-messages = pkgs.callPackage ./nix/check-commit-messages.nix { };
        in
        {
          packages = {
            inherit nirion;
            default = nirion;
          };

          apps.pre-commit = {
            type = "app";
            program = "${pre-commit-app}/bin/pre-commit-run";
            meta.description = "Run pre-commit hooks for all files";
          };

          apps.check-commit-messages = {
            type = "app";
            program = "${check-commit-messages}/bin/check-commit-messages";
            meta.description = "Check commit messages in a git revision range";
          };

          checks = {
            package = nirion;
            rust-tests = nirion.tests.rust;
          }
          // import ./tests/module { inherit pkgs self; }
          // import ./tests/vm { inherit pkgs self; };

          devShells.default = pkgs.callPackage ./nix/dev-shell.nix { inherit pre-commit; };
        }
      )
    // {
      nixosModules.nirion = import ./nix/module.nix { inherit self; };
    };
}
