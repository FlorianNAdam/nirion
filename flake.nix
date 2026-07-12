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
        in
        {
          packages = {
            inherit nirion;
            default = nirion;
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
