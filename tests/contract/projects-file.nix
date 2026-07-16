{
  pkgs,
  self,
  lockFile,
  evalConfig,
  baseNirionConfig,
  emptyLockFile,
  ...
}:

let
  systemName = pkgs.stdenv.hostPlatform.system;
  nirion = self.packages.${systemName}.nirion;

  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        lockFile = lockFile {
          "web.nginx" = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
          "shared.postgres" = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        };

        images."shared.postgres" = "postgres:16-alpine";

        projects.web.services = {
          nginx = {
            image = "nginx:latest";
            restart = "unless-stopped";
            ports = [ "8080:80" ];
          };

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

  projectsFile = system.config.virtualisation.nirion.out.projectsFileStatic;
in
pkgs.runCommand "nirion-contract-projects-file" { } ''
  export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt

  ${nirion}/bin/nirion \
    --lock-file ${emptyLockFile} \
    --project-file ${projectsFile} \
    list \
    > /dev/null

  touch $out
''
