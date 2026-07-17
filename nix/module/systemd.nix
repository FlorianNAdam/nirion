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
        nirion up --plain ${lib.escapeShellArg projectName}
      '';
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecReload = ''
          ${nirionPkg}/bin/nirion reload --plain ${lib.escapeShellArg projectName}
        '';
        ExecStop = ''
          ${nirionPkg}/bin/nirion down --plain ${lib.escapeShellArg projectName}
        '';
      };
    }
  ) cfg.projects;
}
