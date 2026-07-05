{
  cfg,
  envVars,
  lib,
  nirionPkg,
  pkgs,
}:

lib.mkIf (cfg.projects != { }) {
  systemd.services = lib.mapAttrs' (
    projectName: project:
    lib.nameValuePair "nirion-${projectName}" {
      wantedBy = [ "multi-user.target" ];
      after = [
        "docker.service"
        "docker.socket"
      ];
      requires = [ "docker.service" ];
      environment = envVars;
      path = [
        nirionPkg
        pkgs.docker
      ];
      script = ''
        nirion up --no-tui ${lib.escapeShellArg projectName}
      '';
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecReload = ''
          nirion reload --no-tui ${lib.escapeShellArg projectName}
        '';
        ExecStop = ''
          nirion down --no-tui ${lib.escapeShellArg projectName}
        '';
      };
    }
  ) cfg.projects;
}
