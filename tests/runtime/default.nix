{
  pkgs,
  self,
}:
let
  lib = pkgs.lib;
  lockFile = attrs: builtins.toFile "nirion-lock.json" (builtins.toJSON attrs);

  common = {
    inherit
      pkgs
      lib
      self
      lockFile
      ;

    emptyLockFile = lockFile { };

    baseNirionConfig = {
      lockFile = lockFile { };
      lockFileOutput = "/var/lib/nirion/lock.json";
    };

    evalConfig =
      modules:
      import (pkgs.path + "/nixos/lib/eval-config.nix") {
        system = pkgs.stdenv.hostPlatform.system;
        modules = [
          self.nixosModules.nirion
          { system.stateVersion = "26.05"; }
        ]
        ++ modules;
      };
  };

  tests = {
    healthchecks = ./healthchecks.nix;
    lock-file = ./lock-file.nix;
    micro-json = ./micro-json.nix;
    projects-file = ./projects-file.nix;
  };
in
builtins.listToAttrs (
  map (name: {
    name = "runtime-${name}";
    value = import tests.${name} common;
  }) (builtins.attrNames tests)
)
