{
  arionConfig,
  lib,
  nirion,
  nirionConfig,
  nirionEnvVars,
  pkgs,
  sopsTemplateName,
  sopsTemplatePath,
}:

{
  environment.systemPackages = [
    nirion
  ];

  environment.etc."nirion/projects.json".text = builtins.toJSON nirionConfig.out.projects;

  environment.variables = nirionEnvVars;

  virtualisation.nirion = {
    out.locked_images =
      let
        lockFile =
          if nirionConfig.lockFile != null then
            lib.importJSON nirionConfig.lockFile
          else if nirionConfig.images != { } then
            lib.warn "nirion: No lockFile specified" { }
          else
            { };
      in
      lib.mapAttrs (
        name: imageRef:
        let
          hasDigest = builtins.match ".*@sha256:.*" imageRef != null;
        in
        if hasDigest then
          imageRef
        else
          let
            digest =
              if builtins.hasAttr name lockFile then
                let
                  entry = lockFile.${name};
                in
                if builtins.isString entry then entry else entry.digest
              else
                null;
          in
          if digest != null then
            "${imageRef}@${digest}"
          else
            lib.warn "nirion: Image '${name}' (${imageRef}) not locked - using mutable tag" imageRef
      ) nirionConfig.images;

    out.images_v2 = lib.mapAttrs (
      _: projectConfig:
      lib.filterAttrs (_: v: v != null) (
        lib.mapAttrs (_: lib.attrByPath [ "service" "image" ] null) (projectConfig.settings.services or { })
      )
    ) (nirionConfig.projects or { });

    images = lib.foldlAttrs (
      acc: name: value:
      if builtins.isAttrs value then
        acc
        // (lib.mapAttrs' (subname: subvalue: {
          name = "${name}.${subname}";
          value = subvalue;
        }) value)
      else
        acc // { ${name} = value; }
    ) { } nirionConfig.out.images_v2;

    out.projects = lib.mapAttrs (
      projectName: project:
      let
        images = nirionConfig.out.images_v2.${projectName} or { };
        arionProjectConfig = arionConfig.projects.${projectName};
        dockerCompose =
          if nirionConfig.enableSops then
            sopsTemplatePath projectName
          else
            arionProjectConfig.settings.out.dockerComposeYaml;
      in
      {
        name = arionProjectConfig.settings.project.name;
        docker-compose = dockerCompose;
        services = lib.mapAttrs (serviceName: service: {
          image = images.${serviceName} or null;
          healthcheck = service.service.healthcheck or null;
          restart = service.service.restart or null;
        }) project.settings.services;
      }
    ) nirionConfig.projects;

    out.projectsFile = "/etc/nirion/projects.json";
    out.projectsFileStatic =
      let
        json = pkgs.writeText "projects.json" (builtins.toJSON nirionConfig.out.projects);
      in
      "${json}";
  };

  virtualisation.arion.projects = import ./arion.nix { inherit lib nirionConfig; };

  sops.templates = lib.mkIf nirionConfig.enableSops (
    lib.attrsets.mapAttrs' (
      projectName: projectConfig:
      let
        arionProjectConfig = arionConfig.projects.${projectName};
        templateName = sopsTemplateName projectName;
        dockerComposeText = arionProjectConfig.settings.out.dockerComposeYamlText;
      in
      lib.nameValuePair templateName {
        content = dockerComposeText;
      }
    ) nirionConfig.projects
  );

  systemd.services = lib.mkIf nirionConfig.enableSops (
    lib.attrsets.mapAttrs' (
      projectName: projectConfig:
      let
        arionProjectConfig = arionConfig.projects.${projectName};
        serviceName = arionProjectConfig.serviceName;
        templatePath = sopsTemplatePath projectName;
      in
      lib.nameValuePair serviceName {
        environment.ARION_PREBUILT = lib.mkOverride 60 "${templatePath}";
      }
    ) nirionConfig.projects
  );
}
