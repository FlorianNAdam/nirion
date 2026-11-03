{
  cfg,
  envVars,
  lib,
  nirionPkg,
  pkgs,
}:

lib.mkIf (cfg.projects != { }) {
  systemd.targets.nirion = {
    description = "Nirion projects";
    wantedBy = [ "multi-user.target" ];
    conflicts = [ "shutdown.target" ];
    before = [ "shutdown.target" ];
    unitConfig.DefaultDependencies = false;
  };

  systemd.services = lib.mapAttrs' (
    projectName: _:
    lib.nameValuePair "nirion-${projectName}" {
      wantedBy = [ "nirion.target" ];
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
