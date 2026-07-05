{
  cfg,
  lib,
  pkgs,
  sopsTemplatePath,
  hasSops,
}:

lib.mkIf (cfg.projects != { }) {
  systemd.services = lib.mapAttrs' (
    projectName: project:
    let
      compose = cfg.out.compose.${projectName};
      composeFile = if cfg.enableSops && hasSops then sopsTemplatePath projectName else compose.file;
      composeArgs = "--file ${lib.escapeShellArg composeFile} --project-name ${lib.escapeShellArg compose.name}";
    in
    lib.nameValuePair "nirion-${projectName}" {
      wantedBy = [ "multi-user.target" ];
      after = [
        "docker.service"
        "docker.socket"
      ];
      requires = [ "docker.service" ];
      path = [ pkgs.docker ];
      script = ''
        docker-compose ${composeArgs} up -d
      '';
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecReload = ''
          docker-compose ${composeArgs} up -d
        '';
      };
    }
  ) cfg.projects;
}
