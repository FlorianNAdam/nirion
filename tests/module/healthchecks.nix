{
  lib,
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    (
      { config, ... }:
      {
        virtualisation.nirion = baseNirionConfig // {
          projects.health.services.http = {
            extraOptions.image = "example.invalid/http:latest";
            healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
              port = 8080;
              path = "/ready";
              expect.status = 204;
            };
          };
        };
      }
    )
  ];

  service = system.config.virtualisation.nirion.out.compose.health.attrs.services.http;
  test = service.healthcheck.test;
in
[
  {
    assertion =
      builtins.elemAt test 0 == "CMD"
      && builtins.elemAt test 1 == "perl"
      && lib.hasInfix "GET /ready HTTP/1.0" (builtins.elemAt test 4)
      && lib.hasInfix "Expected HTTP status 204" (builtins.elemAt test 4);
    message = "mkHttpHealthcheck did not render the expected compose healthcheck command";
  }
]
