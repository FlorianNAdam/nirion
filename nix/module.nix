{ self, arion }:

{
  config,
  pkgs,
  lib,
  ...
}:
let
  system = pkgs.stdenv.hostPlatform.system;

  nirionPkg = self.packages.${system}.nirion;

  nirionConfig = config.virtualisation.nirion;

  arionConfig = config.virtualisation.arion;

  sopsTemplateName =
    projectName: "nirion/${arionConfig.projects.${projectName}.serviceName}/docker-compose.yaml";

  sopsTemplatePath = projectName: config.sops.templates.${sopsTemplateName projectName}.path;

  nixEvalOption = nirionConfig.nixEval;

  nirionNixTarget =
    let
      setCount =
        (if nixEvalOption.target != null then 1 else 0)
        + (if nixEvalOption.rawTarget != null then 1 else 0)
        + (if nixEvalOption.nixos.config != null || nixEvalOption.nixos.host != null then 1 else 0);
    in
    if setCount <= 1 then
      if nixEvalOption.target != null then
        {
          name = "NIRION_NIX_TARGET";
          value = "${nixEvalOption.target}";
        }
      else if nixEvalOption.rawTarget != null then
        {
          name = "NIRION_RAW_NIX_TARGET";
          value = "${nixEvalOption.rawTarget}";
        }
      else if nixEvalOption.nixos.config != null || nixEvalOption.nixos.host != null then
        {
          name = "NIRION_NIX_TARGET";
          value = "${nixEvalOption.nixos.config}#nixosConfigurations.${nixEvalOption.nixos.host}";
        }
      else
        null
    else
      throw "Only one of nixEval.target, nixEval.rawTarget, or nixEval.nixos may be set";

  nirionEnvVars = {
    NIRION_LOCK_FILE = nirionConfig.lockFileOutput;
    NIRION_PROJECT_FILE = nirionConfig.out.projectsFile;
  }
  // lib.optionalAttrs (nirionConfig.authFile != null) {
    NIRION_AUTH_FILE = nirionConfig.authFile;
  }
  // lib.optionalAttrs (nirionNixTarget != null) {
    ${nirionNixTarget.name} = nirionNixTarget.value;
  };

  nirion = import ./module/wrapper.nix {
    inherit
      lib
      pkgs
      nirionPkg
      nirionEnvVars
      ;
  };
in
{
  imports = [
    arion.nixosModules.arion
    ./module/lib.nix
  ];

  options = import ./module/options.nix { inherit config lib; };

  config = import ./module/config.nix {
    inherit
      arionConfig
      lib
      nirion
      nirionConfig
      nirionEnvVars
      pkgs
      sopsTemplateName
      sopsTemplatePath
      ;
  };
}
