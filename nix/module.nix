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

  sopsTemplateName = projectName: "nirion/${projectName}/compose.yaml";
  sopsTemplatePath = projectName: config.sops.templates.${sopsTemplateName projectName}.path;
  hasSops = options ? sops;
  hasProjectSops = lib.any (project: project.sops.secrets != { } || project.sops.templates != { }) (
    lib.attrValues cfg.projects
  );

  projectSopsAccessDefaults =
    project:
    lib.optionalAttrs (project.sops.group != null) {
      owner = lib.mkDefault "root";
      group = lib.mkDefault project.sops.group.name;
      mode = lib.mkDefault "0440";
    };

  projectSopsSecretDefaults =
    projectName: project:
    lib.optionalAttrs (project.sops.file != null) {
      sopsFile = lib.mkDefault project.sops.file;
    }
    // projectSopsAccessDefaults project;

  projectSopsReloadUnits =
    projectName: project: entry:
    lib.optionalAttrs project.sops.reloadOnChange {
      reloadUnits = lib.unique ((entry.reloadUnits or [ ]) ++ [ "nirion-${projectName}.service" ]);
    };

  projectSopsEntry =
    projectName: project: defaults: entry:
    defaults // entry // projectSopsReloadUnits projectName project entry;

  projectSopsSecrets = lib.foldlAttrs (
    acc: projectName: project:
    acc
    // lib.mapAttrs (
      _: secret:
      projectSopsEntry projectName project (projectSopsSecretDefaults projectName project) secret
    ) project.sops.secrets
  ) { } cfg.projects;

  projectSopsTemplate =
    projectName: project: template:
    {
      content = template.content;
      mode = lib.mkDefault template.mode;
      uid = template.uid;
      gid = template.gid;
      reloadUnits =
        template.reloadUnits ++ lib.optional project.sops.reloadOnChange "nirion-${projectName}.service";
      restartUnits = template.restartUnits;
    }
    // lib.optionalAttrs (template.path != null) {
      path = template.path;
    }
    // lib.optionalAttrs (template.file != null) {
      file = template.file;
    }
    // lib.optionalAttrs (template.owner != null) {
      owner = template.owner;
    }
    // lib.optionalAttrs (template.group != null) {
      group = template.group;
    }
    // projectSopsAccessDefaults project;

  projectSopsTemplates = lib.foldlAttrs (
    acc: projectName: project:
    acc
    // lib.mapAttrs (
      _: template: projectSopsTemplate projectName project template
    ) project.sops.templates
  ) { } cfg.projects;

in
{
  imports = [
    ./module/lib
  ];

  options.virtualisation.nirion = import ./module/options.nix { inherit lib; };

  config = lib.mkMerge [
    {
      environment.systemPackages = [ nirion ];
      environment.etc."nirion/projects.json".text = builtins.toJSON cfg.out.projects;
      environment.variables = envVars;

      assertions = [
        {
          assertion = !cfg.sops.overrideComposeFile || hasSops;
          message = "virtualisation.nirion.sops.overrideComposeFile requires a module that provides the sops.templates option, such as sops-nix.";
        }
        {
          assertion = !hasProjectSops || hasSops;
          message = "virtualisation.nirion.projects.*.sops requires a module that provides the sops options, such as sops-nix.";
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
            composeFile =
              if cfg.sops.overrideComposeFile && hasSops then sopsTemplatePath projectName else compose.file;
          in
          {
            name = compose.name;
            dockerCompose = composeFile;
            services = lib.mapAttrs (serviceName: renderedService: {
              image = project.services.${serviceName}.image or null;
              resolvedImage = renderedService.image or null;
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
      lib.mkIf cfg.sops.overrideComposeFile {
        sops.templates = lib.mapAttrs' (projectName: project: {
          name = sopsTemplateName projectName;
          value = {
            content = cfg.out.compose.${projectName}.text;
          }
          // lib.optionalAttrs project.sops.reloadOnChange {
            reloadUnits = [ "nirion-${projectName}.service" ];
          };
        }) cfg.projects;
      }
    ))

    {
      users.groups = lib.foldlAttrs (
        acc: _: project:
        acc
        // lib.optionalAttrs (project.sops.group != null) {
          ${project.sops.group.name}.gid = project.sops.group.gid;
        }
      ) { } cfg.projects;
    }

    (lib.optionalAttrs hasSops {
      sops.secrets = projectSopsSecrets;
      sops.templates = projectSopsTemplates;
    })

    (import ./module/systemd.nix {
      inherit
        cfg
        envVars
        lib
        nirionPkg
        pkgs
        ;
    })
  ];
}
