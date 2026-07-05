{
  cfg,
  lib,
  pkgs,
}:

let
  nonEmpty = value: value != null && value != [ ] && value != { };

  renderService =
    projectName: serviceName: service:
    let
      resolvedImage =
        if service.lockedImage != null then
          cfg.out.locked_images.${service.lockedImage}
            or (throw "nirion: lockedImage '${service.lockedImage}' not found for service '${projectName}.${serviceName}'")
        else if service.image != null then
          cfg.out.locked_images."${projectName}.${serviceName}" or service.image
        else
          null;

      build = lib.filterAttrs (_: nonEmpty) service.build;
      healthcheck = lib.filterAttrs (_: nonEmpty) service.healthcheck;
      capAdd = lib.attrNames (lib.filterAttrs (_: value: value == true) service.capabilities);
      capDrop = lib.attrNames (lib.filterAttrs (_: value: value == false) service.capabilities);
    in
    lib.filterAttrs (_: nonEmpty) {
      image = resolvedImage;
      inherit build healthcheck;
      inherit (service)
        command
        entrypoint
        container_name
        hostname
        user
        working_dir
        environment
        env_file
        labels
        ports
        expose
        volumes
        tmpfs
        devices
        depends_on
        restart
        stop_signal
        stop_grace_period
        privileged
        tty
        dns
        extra_hosts
        links
        external_links
        network_mode
        networks
        sysctls
        blkio_config
        ;
    }
    // lib.optionalAttrs (capAdd != [ ]) {
      cap_add = capAdd;
    }
    // lib.optionalAttrs (capDrop != [ ]) {
      cap_drop = capDrop;
    }
    // service.extraOptions;
in
lib.mapAttrs (
  projectName: project:
  let
    services = lib.mapAttrs (renderService projectName) project.services;

    attrs = {
      inherit services;
      networks = project.out.networks;
      volumes = project.out.volumes;
    }
    // project.extraOptions;

    text = builtins.toJSON attrs;
    file = pkgs.writeText "docker-compose-${projectName}.json" text;
  in
  {
    inherit
      attrs
      text
      file
      services
      ;
    name = project.composeProjectName;
  }
) cfg.projects
