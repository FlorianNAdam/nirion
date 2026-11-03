{
  lib,
  evalConfig,
  lockFile,
  ...
}:

let
  module = {
    virtualisation.nirion = {
      lockFile = lockFile {
        "web.nginx" = {
          image = "nginx:latest";
          version = "latest";
          digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        };
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
  };

  system = evalConfig [ module ];

  cfg = system.config.virtualisation.nirion;
  compose = cfg.out.compose.web.attrs;
  service = compose.services.nginx;
  unit = system.config.systemd.services.nirion-web;
  target = system.config.systemd.targets.nirion;
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
          resolvedImage = "nginx:latest@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
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
  {
    assertion = unit.wantedBy == [ "nirion.target" ];
    message = "project service is not wanted by nirion.target";
  }
  {
    assertion = target.wantedBy == [ "multi-user.target" ];
    message = "nirion.target is not wanted by multi-user.target";
  }
  {
    assertion = !target.unitConfig.DefaultDependencies;
    message = "nirion.target waits for project services through default dependencies";
  }
  {
    assertion = target.conflicts == [ "shutdown.target" ];
    message = "nirion.target does not stop during shutdown";
  }
  {
    assertion = target.before == [ "shutdown.target" ];
    message = "nirion.target is not ordered before shutdown";
  }
]
