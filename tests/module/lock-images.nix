{
  evalConfig,
  lockFile,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = {
        lockFile = lockFile {
          "app.web" = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
          "shared.postgres".digest =
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        };
        lockFileOutput = "/var/lib/nirion/lock.json";

        images."shared.postgres" = "postgres:16-alpine";

        projects.app.services = {
          web.image = "nginx:1.27";
          pinned.image = "redis:7@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
          db = {
            lockedImage = "shared.postgres";
            healthcheck.test = [
              "CMD-SHELL"
              "pg_isready"
            ];
          };
        };
      };
    }
  ];

  cfg = system.config.virtualisation.nirion;
  services = cfg.out.compose.app.attrs.services;
in
[
  {
    assertion =
      services.web.image
      == "nginx:1.27@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    message = "project service image lock digest was not applied";
  }
  {
    assertion =
      services.pinned.image
      == "redis:7@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    message = "already-digested service image should not be rewritten";
  }
  {
    assertion =
      services.db.image
      == "postgres:16-alpine@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    message = "lockedImage did not resolve through virtualisation.nirion.images";
  }
  {
    assertion =
      cfg.images == {
        "app.web" = "nginx:1.27";
        "app.pinned" = "redis:7@sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        "shared.postgres" = "postgres:16-alpine";
      };
    message = "image registry did not combine project and explicitly configured images";
  }
  {
    assertion = cfg.out.projects.app.services.db.image == null;
    message = "project metadata should not report an image for lockedImage-only services";
  }
  {
    assertion = cfg.out.projects.app.services.web.image == "nginx:1.27";
    message = "project metadata should keep the configured mutable image reference";
  }
]
