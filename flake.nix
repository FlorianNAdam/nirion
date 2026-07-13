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
    nixcov = {
      url = "github:FlorianNAdam/nixcov";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.naersk.follows = "naersk";
    };
  };

  outputs =
    {
      git-hooks-nix,
      self,
      nixpkgs,
      flake-utils,
      naersk,
      nixcov,
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
          check-message = pkgs.callPackage ./nix/check-message.nix { };
          rust-coverage = pkgs.callPackage ./nix/coverage/rust.nix { };
          nix-coverage = pkgs.callPackage ./nix/coverage/nix.nix {
            nixcovProgram = nixcov.apps.${system}.default.program;
          };
          full-coverage = pkgs.callPackage ./nix/coverage/full.nix {
            nixCoverage = nix-coverage;
            rustCoverage = rust-coverage;
          };
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

          apps.check-message = {
            type = "app";
            program = "${check-message}/bin/check-message";
            meta.description = "Check a single commit-style message";
          };

          apps.coverage = {
            type = "app";
            program = "${rust-coverage}/bin/rust-coverage";
            meta.description = "Run Rust coverage using Tarpaulin";
          };

          apps.rust-coverage = {
            type = "app";
            program = "${rust-coverage}/bin/rust-coverage";
            meta.description = "Run Rust coverage using Tarpaulin and write coverage-rust.info";
          };

          apps.nix-coverage = {
            type = "app";
            program = "${nix-coverage}/bin/nix-coverage";
            meta.description = "Run Nix coverage using nixcov and write coverage-nix.info";
          };

          apps.full-coverage = {
            type = "app";
            program = "${full-coverage}/bin/full-coverage";
            meta.description = "Run Rust and Nix coverage and write coverage-merged.info";
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
