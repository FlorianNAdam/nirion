{
  pkgs,
  lib,
  self,
}:

let
  lockFile = contents: builtins.toFile "nirion-lock.json" contents;

  common = {
    inherit
      pkgs
      lib
      self
      lockFile
      ;

    emptyLockFile = lockFile "{}";

    baseNirionConfig = {
      lockFile = lockFile "{}";
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

  testFiles = [
    ./basic-systemd.nix
    ./compose-options.nix
    ./healthchecks.nix
    ./nix-eval.nix
    ./sops.nix
  ];

  checks = lib.concatMap (testFile: import testFile common) testFiles;
in
pkgs.runCommand "nirion-module-tests" { } ''
  ${lib.concatMapStringsSep "\n" (check: ''
    ${if check.assertion then ":" else "echo ${lib.escapeShellArg check.message}; exit 1"}
  '') checks}
  touch $out
''
