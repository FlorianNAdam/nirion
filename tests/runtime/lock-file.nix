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

  digestOnlyLockFile = lockFile {
    "web.nginx" = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  };

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
        lockFile = digestOnlyLockFile;
        projects.web.services.nginx.image = "nginx:latest";
      };
    }
  ];

  projectsFile = system.config.virtualisation.nirion.out.projectsFileStatic;
in
pkgs.runCommand "nirion-runtime-lock-file" { } ''
  export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt

  for lock_file in ${digestOnlyLockFile} ${fullLockFile}; do
    ${nirion}/bin/nirion \
      --lock-file "$lock_file" \
      --project-file ${projectsFile} \
      list \
      > /dev/null
  done

  touch $out
''
