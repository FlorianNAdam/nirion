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
          workspace = naersk-lib.buildPackage {
            pname = "nirion-workspace-tests";
            src = pkgs.lib.cleanSource ./.;
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
            module = pkgs.callPackage ./tests/module { inherit self; };
            vm-basic = pkgs.testers.runNixOSTest {
              imports = [
                (import ./tests/vm {
                  inherit self;
                  test = "basic";
                })
              ];
            };
            vm-cli-lifecycle = pkgs.testers.runNixOSTest {
              imports = [
                (import ./tests/vm {
                  inherit self;
                  test = "cli-lifecycle";
                })
              ];
            };
            vm-multi-project = pkgs.testers.runNixOSTest {
              imports = [
                (import ./tests/vm {
                  inherit self;
                  test = "multi-project";
                })
              ];
            };
          };

          devShells.default = pkgs.callPackage ./nix/dev-shell.nix { };
        }
      )
    // {
      nixosModules.nirion = import ./nix/module.nix { inherit self; };
    };
}
