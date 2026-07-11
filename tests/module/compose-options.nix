{
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        projects.app = {
          enableDefaultNetwork = false;
          volumes.data = { };
          networks.backend = {
            name = "app-backend";
            internal = true;
          };
          services.api = {
            extraOptions.image = "example.invalid/api:latest";
            command = [ "serve" ];
            environment.PORT = "8080";
            volumes = [ "data:/data" ];
            networks.backend.aliases = [ "api.internal" ];
            capabilities = {
              NET_ADMIN = true;
              SYS_ADMIN = false;
            };
            healthcheck = {
              test = [
                "CMD"
                "true"
              ];
              interval = "10s";
              retries = 5;
            };
          };
        };
      };
    }
  ];

  compose = system.config.virtualisation.nirion.out.compose.app.attrs;
  service = compose.services.api;
in
[
  {
    assertion =
      compose.networks.backend == {
        name = "app-backend";
        internal = true;
      };
    message = "custom network options were not rendered";
  }
  {
    assertion = compose.volumes.data == { };
    message = "project volume was not rendered";
  }
  {
    assertion = !(compose.networks ? default);
    message = "default network was rendered even though enableDefaultNetwork is false";
  }
  {
    assertion = service.image == "example.invalid/api:latest";
    message = "service extraOptions were not rendered";
  }
  {
    assertion = service.environment.PORT == "8080";
    message = "service environment was not rendered";
  }
  {
    assertion = service.networks.backend.aliases == [ "api.internal" ];
    message = "service network attachment options were not rendered";
  }
  {
    assertion = service.cap_add == [ "NET_ADMIN" ];
    message = "enabled capabilities were not rendered as cap_add";
  }
  {
    assertion = service.cap_drop == [ "SYS_ADMIN" ];
    message = "disabled capabilities were not rendered as cap_drop";
  }
  {
    assertion =
      service.healthcheck == {
        test = [
          "CMD"
          "true"
        ];
        interval = "10s";
        timeout = "30s";
        start_period = "0s";
        retries = 5;
      };
    message = "explicit healthcheck options were not rendered";
  }
]
