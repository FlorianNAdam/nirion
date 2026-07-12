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
          rustSource = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              let
                rel = pkgs.lib.removePrefix "${toString ./.}/" (toString path);
              in
              rel == "Cargo.toml"
              || rel == "Cargo.lock"
              || pkgs.lib.hasPrefix "nirion-bin/" rel
              || pkgs.lib.hasPrefix "nirion-lib/" rel
              || pkgs.lib.hasPrefix "nirion-oci-lib/" rel
              || pkgs.lib.hasPrefix "nirion-tui-lib/" rel
              || (
                type == "directory"
                && builtins.elem rel [
                  "nirion-bin"
                  "nirion-lib"
                  "nirion-oci-lib"
                  "nirion-tui-lib"
                ]
              );
          };
          workspace = naersk-lib.buildPackage {
            pname = "nirion-workspace-tests";
            src = rustSource;
            mode = "test";
            cargoTestOptions = options: options ++ [ "--workspace" ];
          };
        in
        {
          packages = {
            inherit nirion;
            default = nirion;
          };

          checks = {
            package = nirion;
            inherit workspace;
          }
          // import ./tests/module { inherit pkgs self; }
          // import ./tests/vm { inherit pkgs self; };

          devShells.default = pkgs.callPackage ./nix/dev-shell.nix { };
        }
      )
    // {
      nixosModules.nirion = import ./nix/module.nix { inherit self; };
    };
}
