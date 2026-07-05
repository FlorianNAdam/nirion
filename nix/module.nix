{ self }:

{
  config,
  lib,
  options,
  pkgs,
  ...
}:
let
  cfg = config.virtualisation.nirion;
  system = pkgs.stdenv.hostPlatform.system;
  nirionPkg = self.packages.${system}.nirion;

  nixEvalOption = cfg.nixEval;
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
          value = nixEvalOption.target;
        }
      else if nixEvalOption.rawTarget != null then
        {
          name = "NIRION_RAW_NIX_TARGET";
          value = nixEvalOption.rawTarget;
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

  envVars = {
    NIRION_LOCK_FILE = cfg.lockFileOutput;
    NIRION_PROJECT_FILE = cfg.out.projectsFile;
  }
  // lib.optionalAttrs (cfg.authFile != null) {
    NIRION_AUTH_FILE = cfg.authFile;
  }
  // lib.optionalAttrs (nirionNixTarget != null) {
    ${nirionNixTarget.name} = nirionNixTarget.value;
  };

  nirion = import ./module/wrapper.nix {
    inherit
      lib
      pkgs
      nirionPkg
      envVars
      ;
  };

  sopsTemplateName = projectName: "nirion/${projectName}/docker-compose.json";
  sopsTemplatePath = projectName: config.sops.templates.${sopsTemplateName projectName}.path;
  hasSops = options ? sops.templates;

in
{
  imports = [
    ./module/lib.nix
  ];

  options.virtualisation.nirion = import ./module/options.nix { inherit lib; };

  config = lib.mkMerge [
    {
      environment.systemPackages = [ nirion ];
      environment.etc."nirion/projects.json".text = builtins.toJSON cfg.out.projects;
      environment.variables = envVars;

      assertions = [
        {
          assertion = !cfg.enableSops || hasSops;
          message = "virtualisation.nirion.enableSops requires a module that provides the sops.templates option, such as sops-nix.";
        }
        {
          assertion = (cfg.nixEval.nixos.config == null) == (cfg.nixEval.nixos.host == null);
          message = "virtualisation.nirion.nixEval.nixos.config and virtualisation.nirion.nixEval.nixos.host must be set together.";
        }
      ];

      virtualisation.docker.enable = lib.mkIf (cfg.projects != { }) true;

      virtualisation.nirion = {
        out.images_v2 = lib.mapAttrs (
          _: project:
          lib.filterAttrs (_: value: value != null) (
            lib.mapAttrs (_: service: service.image) project.services
          )
        ) cfg.projects;

        images = lib.foldlAttrs (
          acc: projectName: images:
          acc
          // lib.mapAttrs' (serviceName: image: {
            name = "${projectName}.${serviceName}";
            value = image;
          }) images
        ) { } cfg.out.images_v2;

        out.locked_images =
          let
            lockFile =
              if cfg.lockFile != null then
                lib.importJSON cfg.lockFile
              else if cfg.images != { } then
                lib.warn "nirion: No lockFile specified" { }
              else
                { };
          in
          lib.mapAttrs (
            name: imageRef:
            if builtins.match ".*@sha256:.*" imageRef != null then
              imageRef
            else
              let
                entry = lockFile.${name} or null;
                digest = if builtins.isAttrs entry then entry.digest else entry;
              in
              if digest != null then
                "${imageRef}@${digest}"
              else
                lib.warn "nirion: Image '${name}' (${imageRef}) not locked - using mutable tag" imageRef
          ) cfg.images;

        out.compose = import ./module/compose/render.nix { inherit cfg lib pkgs; };

        out.projects = lib.mapAttrs (
          projectName: project:
          let
            compose = cfg.out.compose.${projectName};
            composeFile = if cfg.enableSops && hasSops then sopsTemplatePath projectName else compose.file;
          in
          {
            name = compose.name;
            docker-compose = composeFile;
            services = lib.mapAttrs (serviceName: renderedService: {
              image = project.services.${serviceName}.image or null;
              healthcheck = renderedService ? healthcheck;
              restart = renderedService.restart or null;
            }) compose.services;
          }
        ) cfg.projects;

        out.projectsFile = "/etc/nirion/projects.json";
        out.projectsFileStatic = toString (
          pkgs.writeText "projects.json" (builtins.toJSON cfg.out.projects)
        );
      };
    }

    (lib.optionalAttrs hasSops (
      lib.mkIf cfg.enableSops {
        sops.templates = lib.mapAttrs' (projectName: project: {
          name = sopsTemplateName projectName;
          value.content = cfg.out.compose.${projectName}.text;
        }) cfg.projects;
      }
    ))

    (import ./module/systemd.nix {
      inherit
        cfg
        lib
        pkgs
        sopsTemplatePath
        hasSops
        ;
    })
  ];
}
