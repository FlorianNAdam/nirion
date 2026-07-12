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
    basic-systemd = ./basic-systemd.nix;
    compose-options = ./compose-options.nix;
    healthchecks = ./healthchecks.nix;
    lock-images = ./lock-images.nix;
    nix-eval = ./nix-eval.nix;
    service-shapes = ./service-shapes.nix;
    sops = ./sops.nix;
  };

  mkModuleTest =
    name: testFile:
    let
      checks = import testFile common;
    in
    pkgs.runCommand "nirion-module-${name}-tests" { } ''
      ${lib.concatMapStringsSep "\n" (check: ''
        ${if check.assertion then ":" else "echo ${lib.escapeShellArg check.message}; exit 1"}
      '') checks}
      touch $out
    '';
in
builtins.listToAttrs (
  map (name: {
    name = "module-${name}";
    value = mkModuleTest name tests.${name};
  }) (builtins.attrNames tests)
)
