{
  lib,
  evalConfig,
  lockFile,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = {
        lockFile = lockFile {
          "web.nginx" = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        };
        lockFileOutput = "/var/lib/nirion/lock.json";

        projects.web = {
          services.nginx = {
            image = "nginx:latest";
            ports = [ "8080:80" ];
            restart = "unless-stopped";
          };
        };
      };
    }
  ];

  cfg = system.config.virtualisation.nirion;
  compose = cfg.out.compose.web.attrs;
  service = compose.services.nginx;
  unit = system.config.systemd.services.nirion-web;
in
[
  {
    assertion =
      service.image
      == "nginx:latest@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    message = "locked image digest was not applied to the rendered service image";
  }
  {
    assertion = service.ports == [ "8080:80" ];
    message = "service ports were not rendered";
  }
  {
    assertion = service.restart == "unless-stopped";
    message = "service restart policy was not rendered";
  }
  {
    assertion = compose.networks.default.name == "web";
    message = "default network was not rendered with the compose project name";
  }
  {
    assertion =
      cfg.out.projects.web == {
        name = "web";
        dockerCompose = cfg.out.compose.web.file;
        services.nginx = {
          image = "nginx:latest";
          healthcheck = false;
          restart = "unless-stopped";
        };
      };
    message = "project metadata was not rendered as expected";
  }
  {
    assertion = lib.hasInfix "nirion up --plain web" unit.script;
    message = "systemd start script does not call nirion up";
  }
  {
    assertion = lib.hasInfix "nirion reload --plain web" unit.serviceConfig.ExecReload;
    message = "systemd reload command does not call nirion reload";
  }
  {
    assertion = lib.hasInfix "nirion down --plain web" unit.serviceConfig.ExecStop;
    message = "systemd stop command does not call nirion down";
  }
]
