{
  pkgs,
  self,
  lockFile,
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  systemName = pkgs.stdenv.hostPlatform.system;
  nirion = self.packages.${systemName}.nirion;

  fullLockFile = lockFile {
    "web.nginx" = {
      image = "nginx:latest";
      version = "latest";
      digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    };
  };

  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        lockFile = fullLockFile;
        projects.web.services.nginx.image = "nginx:latest";
      };
    }
  ];

  projectsFile = system.config.virtualisation.nirion.out.projectsFileStatic;
in
pkgs.runCommand "nirion-runtime-lock-file" { } ''
  export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt

  ${nirion}/bin/nirion \
    --lock-file ${fullLockFile} \
    --project-file ${projectsFile} \
    list \
    > /dev/null

  touch $out
''
