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
        docker compose --file ${lib.escapeShellArg composeFile} --project-name ${lib.escapeShellArg compose.name} up -d
      '';
    }
  ) cfg.projects;
}
