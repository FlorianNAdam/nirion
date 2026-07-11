{
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        projects.stack = {
          networks = {
            dmz = {
              name = "dmz";
              external = true;
            };
            internal = { };
          };

          services = {
            app = {
              extraOptions.image = "example.invalid/app:latest";
              container_name = "app";
              labels = {
                "traefik.enable" = "true";
                "traefik.docker.network" = "dmz";
              };
              env_file = [ "/run/secrets/app.env" ];
              hostname = "app-host";
              user = "1000:1000";
              working_dir = "/srv/app";
              ports = [ "8080:80" ];
              expose = [ "9000" ];
              volumes = [
                "/storage/app:/data"
                {
                  type = "bind";
                  source = "/run/secrets/config.yml";
                  target = "/config.yml";
                  bind.propagation = "rshared";
                }
              ];
              entrypoint = "sh -c";
              command = [ "exec app" ];
              stop_signal = "SIGTERM";
              stop_grace_period = "30s";
              init = true;
              tty = true;
              privileged = true;
              devices = [ "/dev/fuse:/dev/fuse" ];
              dns = [ "1.1.1.1" ];
              extra_hosts = [ "host.docker.internal:host-gateway" ];
              security_opt = [ "no-new-privileges:true" ];
              shm_size = "256m";
              sysctls."net.core.somaxconn" = 1024;
              logging.driver = "json-file";
              ulimits.nofile = {
                soft = 1024;
                hard = 2048;
              };
              depends_on = {
                db.condition = "service_healthy";
                redis.condition = "service_healthy";
              };
              networks = [
                "dmz"
                "internal"
              ];
              restart = "always";
            };

            db = {
              extraOptions.image = "postgres:16-alpine";
              networks = [ "internal" ];
              healthcheck.test = [
                "CMD-SHELL"
                "pg_isready"
              ];
            };

            redis = {
              extraOptions.image = "redis:alpine";
              networks = [ "internal" ];
              healthcheck.test = [
                "CMD-SHELL"
                "redis-cli ping | grep PONG"
              ];
            };
          };
        };
      };
    }
  ];

  compose = system.config.virtualisation.nirion.out.compose.stack.attrs;
  app = compose.services.app;
in
[
  {
    assertion =
      compose.networks.dmz == {
        name = "dmz";
        external = true;
      };
    message = "external dmz-style network was not rendered";
  }
  {
    assertion = app.labels."traefik.docker.network" == "dmz";
    message = "service labels were not rendered";
  }
  {
    assertion = app.env_file == [ "/run/secrets/app.env" ];
    message = "service env_file was not rendered";
  }
  {
    assertion = app.hostname == "app-host" && app.user == "1000:1000" && app.working_dir == "/srv/app";
    message = "service identity fields were not rendered";
  }
  {
    assertion = app.ports == [ "8080:80" ] && app.expose == [ "9000" ];
    message = "service port fields were not rendered";
  }
  {
    assertion =
      builtins.elemAt app.volumes 1 == {
        type = "bind";
        source = "/run/secrets/config.yml";
        target = "/config.yml";
        bind.propagation = "rshared";
      };
    message = "attribute-set bind volume was not rendered";
  }
  {
    assertion = app.depends_on.db.condition == "service_healthy";
    message = "depends_on health condition was not rendered";
  }
  {
    assertion =
      app.networks == [
        "dmz"
        "internal"
      ];
    message = "list-style service networks were not rendered";
  }
  {
    assertion = app.entrypoint == "sh -c" && app.command == [ "exec app" ];
    message = "service entrypoint or command was not rendered";
  }
  {
    assertion = app.privileged == true && app.init == true && app.tty == true;
    message = "service boolean runtime flags were not rendered";
  }
  {
    assertion =
      app.stop_signal == "SIGTERM"
      && app.stop_grace_period == "30s"
      && app.devices == [ "/dev/fuse:/dev/fuse" ];
    message = "service stop or device fields were not rendered";
  }
  {
    assertion = app.dns == [ "1.1.1.1" ] && app.extra_hosts == [ "host.docker.internal:host-gateway" ];
    message = "service DNS or extra host fields were not rendered";
  }
  {
    assertion =
      app.security_opt == [ "no-new-privileges:true" ]
      && app.shm_size == "256m"
      && app.sysctls."net.core.somaxconn" == 1024;
    message = "service security or kernel tuning fields were not rendered";
  }
  {
    assertion = app.logging.driver == "json-file" && app.ulimits.nofile.hard == 2048;
    message = "service logging or ulimits fields were not rendered";
  }
]
