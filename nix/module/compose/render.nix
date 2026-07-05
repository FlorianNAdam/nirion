{
  cfg,
  lib,
  pkgs,
}:

let
  renderService =
    projectName: project: serviceName: service:
    let
      resolvedImage =
        if service.lockedImage != null then
          cfg.out.locked_images.${service.lockedImage}
            or (throw "nirion: lockedImage '${service.lockedImage}' not found for service '${projectName}.${serviceName}'")
        else if service.image != null then
          cfg.out.locked_images."${projectName}.${serviceName}" or service.image
        else
          null;

      sopsGroupAdd = lib.optional (project.sops.group != null) (toString project.sops.group.gid);

    in
    service.out.compose
    // lib.optionalAttrs (sopsGroupAdd != [ ]) {
      group_add = lib.unique ((service.out.compose.group_add or [ ]) ++ sopsGroupAdd);
    }
    // lib.optionalAttrs (resolvedImage != null) {
      image = resolvedImage;
    };
in
lib.mapAttrs (
  projectName: project:
  let
    services = lib.mapAttrs (renderService projectName project) project.services;

    attrs = {
      inherit services;
      networks = project.out.networks;
      volumes = project.out.volumes;
    }
    // project.extraOptions;

    text = builtins.toJSON attrs;
    file = pkgs.writeText "compose-${projectName}.yaml" text;
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
