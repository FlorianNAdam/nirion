{
  description = "Docker Compose management for NixOS";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      naersk,
    }:

    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
        nirion = pkgs.callPackage ./nix/package.nix { inherit naersk-lib; };
      in
      {
        packages = {
          inherit nirion;
          default = nirion;
        };

        devShell = pkgs.callPackage ./nix/dev-shell.nix { };
      }
    )
    // {
      nixosModules.nirion = import ./nix/module.nix { inherit self; };
    };
}
