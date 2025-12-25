{
  description = "Convenience wrapper for arion";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    arion = {
      url = "github:hercules-ci/arion";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      naersk,
      arion,
    }:

    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
        };

        naersk-lib = pkgs.callPackage naersk { };

        nirion = naersk-lib.buildPackage {
          pname = "nirion";
          src = ./.;

          buildInputs = with pkgs; [
            makeWrapper
          ];

          postInstall = ''
            wrapProgram $out/bin/nirion \
              --prefix PATH : ${pkgs.skopeo}/bin
          '';
        };
      in
      {
        packages = {
          inherit nirion;
          default = nirion;
        };

        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            cargo
            rustc
            openssl
          ];

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          packages = with pkgs; [
            rust-analyzer
            skopeo
          ];

          shellHook = ''
            export NIRION_LOCK_FILE='/home/florian/my-nixos/modules/arion/nirion.lock'
            export NIRION_PROJECT_FILE='/nix/store/sbnzfv95s8y14lsixfm43cka5si3zwg8-projects.json'
          '';
        };
      }
    )
    // {
      nixosModules.nirion =
        {
          config,
          pkgs,
          lib,
          ...
        }:
        let
          system = pkgs.stdenv.hostPlatform.system;

          nirionPkg = self.packages.${system}.nirion;

          nirionConfig = config.virtualisation.nirion;

          arionConfig = config.virtualisation.arion;

          nirion =
            let
              nixEvalOption = config.virtualisation.nirion.nixEval;

              setCount =
                (if nixEvalOption.target != null then 1 else 0)
                + (if nixEvalOption.rawTarget != null then 1 else 0)
                + (if nixEvalOption.nixos.config != null || nixEvalOption.nixos.host != null then 1 else 0);

              nixTarget =
                if setCount <= 1 then
                  if nixEvalOption.target != null then
                    "--set NIX_TARGET ${nixEvalOption.target}"
                  else if nixEvalOption.rawTarget != null then
                    "--set RAW_NIX_TARGET ${nixEvalOption.rawTarget}"
                  else if nixEvalOption.nixos.config != null || nixEvalOption.nixos.host != null then
                    "--set NIX_TARGET ${nixEvalOption.nixos.config}#nixosConfigurations.${nixEvalOption.nixos.host}"
                  else
                    ""
                else
                  throw "Only one of nixEval.target, nixEval.rawTarget, or nixEval.nixos may be set";
            in
            pkgs.stdenv.mkDerivation {
              name = "nirion";

              src = "${nirionPkg}";

              buildInputs = [ pkgs.makeWrapper ];

              installPhase = ''
                mkdir -p $out/bin
                makeWrapper ${nirionPkg}/bin/nirion $out/bin/nirion \
                  --set NIRION_LOCK_FILE "${nirionConfig.lockFileOutput}" \
                  --set NIRION_PROJECT_FILE "${nirionConfig.out.projectsFile}" \
                  ${nixTarget}
              '';
            };
        in
        {
          imports = [
            arion.nixosModules.arion
          ];

          options = {
            virtualisation.nirion = {
              # Lockfile
              lockFile = lib.mkOption {
                type = lib.types.nullOr lib.types.path;
                default = null;
                description = "Optional path to image digest lock file";
              };
              lockFileOutput = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
                description = "Optional writable output path for lockfile updates";
              };

              # Nix-eval
              nixEval = {
                target = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  default = null;
                  description = "Target for nix-eval (suffix will be appended)";
                };
                rawTarget = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  default = null;
                  description = "Raw target for nix-eval (used as-is)";
                };
                nixos = {
                  config = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                  };
                  host = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                  };
                };
              };

              # Arion
              projects = lib.mkOption {
                type = lib.types.attrsOf (lib.types.anything);
                default = { };
                description = "Arion project configuration with lockfile support";
              };

              # Internal
              images = lib.mkOption {
                type = lib.types.attrsOf lib.types.str;
                default = { };
                description = "Image references to be resolved with digests";
              };
              out = {
                images_v2 = lib.mkOption {
                  type = lib.types.attrsOf lib.types.anything;
                  readOnly = true;
                  internal = true;
                  description = "Image references to be resolved with digests";
                };
                locked_images = lib.mkOption {
                  type = lib.types.attrsOf lib.types.str;
                  readOnly = true;
                  internal = true;
                  description = "Resolved image references with digests";
                };
                projects = lib.mkOption {
                  type = lib.types.attrsOf lib.types.anything;
                  readOnly = true;
                  internal = true;
                };
                projectsFile = lib.mkOption {
                  type = lib.types.str;
                  readOnly = true;
                  internal = true;
                };
              };
            };
          };

          config = {
            environment.systemPackages = [
              nirion
            ];

            virtualisation.nirion = {
              out.locked_images =
                let
                  lockFile =
                    if nirionConfig.lockFile != null then
                      lib.importJSON nirionConfig.lockFile
                    else if nirionConfig.images != { } then
                      lib.warn "nirion: No lockFile specified" { }
                    else
                      { };
                in
                lib.mapAttrs (
                  name: imageRef:
                  let
                    hasDigest = builtins.match ".*@sha256:.*" imageRef != null;
                  in
                  if hasDigest then
                    imageRef
                  else
                    let
                      digest = lockFile.${name} or null;
                    in
                    if digest != null then
                      "${imageRef}@${digest}"
                    else
                      lib.warn "nirion: Image '${name}' (${imageRef}) not locked - using mutable tag" imageRef
                ) nirionConfig.images;

              out.images_v2 = lib.mapAttrs (
                _: projectConfig:
                lib.filterAttrs (_: v: v != null) (
                  lib.mapAttrs (_: lib.attrByPath [ "service" "image" ] null) (projectConfig.settings.services or { })
                )
              ) (nirionConfig.projects or { });

              images = lib.foldlAttrs (
                acc: name: value:
                if builtins.isAttrs value then
                  acc
                  // (lib.mapAttrs' (subname: subvalue: {
                    name = "${name}.${subname}";
                    value = subvalue;
                  }) value)
                else
                  acc // { ${name} = value; }
              ) { } nirionConfig.out.images_v2;

              out.projects = lib.mapAttrs (
                projectName: project:
                let
                  images = nirionConfig.out.images_v2.${projectName} or { };
                in
                {
                  docker-compose = arionConfig.projects.${projectName}.settings.out.dockerComposeYaml;
                  services = lib.mapAttrs (serviceName: service: {
                    image = images.${serviceName} or null;
                    healthcheck = service.service.healthcheck or null;
                    restart = service.service.restart or null;
                  }) project.settings.services;
                }
              ) nirionConfig.projects;

              out.projectsFile =
                let
                  json = builtins.toJSON nirionConfig.out.projects;
                  file = pkgs.writeText "projects.json" "${json}";
                in
                "${file}";
            };

            virtualisation.arion.projects = lib.mapAttrs (
              projectName: projectConfig:
              let
                services = projectConfig.settings.services or { };
                resolvedServices = lib.mapAttrs (
                  serviceName: serviceConfig:
                  let
                    attrName = lib.attrByPath [
                      "service"
                      "locked_image"
                    ] "${projectName}.${serviceName}" serviceConfig;
                  in
                  serviceConfig
                  // (
                    if builtins.hasAttr attrName nirionConfig.out.locked_images then
                      {
                        service = (builtins.removeAttrs (serviceConfig.service or { }) [ "locked_image" ]) // {
                          image = nirionConfig.out.locked_images.${attrName};
                        };
                      }
                    else
                      { }
                  )
                ) services;
              in
              projectConfig
              // {
                settings = (projectConfig.settings or { }) // {
                  services = resolvedServices;
                };
              }
            ) nirionConfig.projects;
          };
        };
    };
}
