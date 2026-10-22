{
  cfg,
  envVars,
  lib,
  nirionPkg,
  pkgs,
}:

let
  projectServices = lib.mapAttrs' (
    projectName: _:
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

  reloadServices = lib.mapAttrs' (
    projectName: _:
    lib.nameValuePair "nirion-${projectName}-reload" {
      description = "Reload Nirion project ${projectName}";
      restartIfChanged = false;
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
      serviceConfig = {
        Type = "exec";
        ExecStart = "${nirionPkg}/bin/nirion reload --plain ${lib.escapeShellArg projectName}";
      };
    }
  ) cfg.projects;
in
lib.mkIf (cfg.projects != { }) {
  systemd.services = projectServices // lib.optionalAttrs cfg.systemd.asyncReload reloadServices;
}
