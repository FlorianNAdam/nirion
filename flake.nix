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
            # Bash completion
            mkdir -p $out/share/bash-completion/completions
            COMPLETE=bash $out/bin/nirion > $out/share/bash-completion/completions/nirion

            # Zsh completion
            mkdir -p $out/share/zsh/site-functions
            COMPLETE=zsh $out/bin/nirion > $out/share/zsh/site-functions/_nirion

            # Fish completion
            mkdir -p $out/share/fish/vendor_completions.d
            COMPLETE=fish $out/bin/nirion > $out/share/fish/vendor_completions.d/nirion.fish
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
          ];
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

              installPhase =
                let
                  wrapperFlags = [
                    "--set NIRION_LOCK_FILE ${nirionConfig.lockFileOutput}"
                    "--set NIRION_PROJECT_FILE ${nirionConfig.out.projectsFile}"
                  ]
                  ++ lib.optional (nirionConfig.authFile != null) "--set NIRION_AUTH_FILE ${nirionConfig.authFile}";
                in
                ''
                  mkdir -p $out/bin
                  makeWrapper ${nirionPkg}/bin/nirion $out/bin/nirion \
                    ${lib.concatStringsSep " " wrapperFlags} \
                    ${nixTarget}

                  patch() {
                    local f="$1"
                    [ -f "$f" ] || return 0

                    sed -i \
                      's|/nix/store/[^[:space:]]*/bin/nirion|'"$out"'/bin/nirion|g' \
                      "$f"
                  }

                  # Fish completion
                  mkdir -p $out/share/fish/vendor_completions.d
                  COMPLETE=fish $out/bin/nirion > $out/share/fish/vendor_completions.d/nirion.fish
                  patch $out/share/fish/vendor_completions.d/nirion.fish
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

              # Auth
              authFile = lib.mkOption {
                type = lib.types.nullOr lib.types.path;
                default = null;
                description = "Optional path to file with oci registry auth configs";
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
                projectsFileStatic = lib.mkOption {
                  type = lib.types.str;
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

            environment.etc."nirion/projects.json".text = builtins.toJSON nirionConfig.out.projects;

            environment.variables = {
              NIRION_LOCK_FILE = "${nirionConfig.lockFileOutput}";
              NIRION_PROJECT_FILE = "${nirionConfig.out.projectsFile}";
            }
            // lib.optionalAttrs (nirionConfig.authFile != null) {
              NIRION_AUTH_FILE = "${nirionConfig.authFile}";
            };

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
                      digest =
                        if builtins.hasAttr name lockFile then
                          let
                            entry = lockFile.${name};
                          in
                          if builtins.isString entry then entry else entry.digest
                        else
                          null;
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
                  arionProjectConfig = arionConfig.projects.${projectName};
                in
                {
                  name = arionProjectConfig.settings.project.name;
                  docker-compose = arionProjectConfig.settings.out.dockerComposeYaml;
                  services = lib.mapAttrs (serviceName: service: {
                    image = images.${serviceName} or null;
                    healthcheck = service.service.healthcheck or null;
                    restart = service.service.restart or null;
                  }) project.settings.services;
                }
              ) nirionConfig.projects;

              out.projectsFile = "/etc/nirion/projects.json";
              out.projectsFileStatic =
                let
                  json = pkgs.writeText "projects.json" (builtins.toJSON nirionConfig.out.projects);
                in
                "${json}";
            };

            virtualisation.arion.projects =
              let
                imageTransform =
                  projectName: serviceName: serviceConfig:
                  if builtins.hasAttr "image" serviceConfig then
                    let
                      lockedImages = nirionConfig.out.locked_images;
                      lockedImage = lockedImages."${projectName}.${serviceName}" or null;
                    in
                    serviceConfig // { image = lockedImage; }
                  else
                    serviceConfig;

                lockedImageTransform =
                  projectName: serviceName: serviceConfig:
                  if builtins.hasAttr "locked_image" serviceConfig then
                    let
                      lockedImages = nirionConfig.out.locked_images;
                      lockedImage =
                        lockedImages."${serviceConfig.locked_image}"
                          or (throw "nirion: Image reference ${serviceConfig.locked_image} not found");
                    in
                    (builtins.removeAttrs serviceConfig [
                      "locked_image"
                    ])
                    // {
                      image = lockedImage;
                    }
                  else
                    serviceConfig;

                applyTransforms =
                  transforms: projectName: serviceName: serviceConfig:
                  lib.lists.foldl (
                    svcConfig: transform: transform projectName serviceName svcConfig
                  ) serviceConfig transforms;

                transformService = applyTransforms [
                  imageTransform
                  lockedImageTransform
                ];
              in
              lib.mapAttrs (
                projectName: projectConfig:
                let
                  services = projectConfig.settings.services or { };
                  transform = transformService projectName;
                  transformedServices = lib.mapAttrs (
                    serviceName: serviceConfig:
                    if builtins.hasAttr "service" serviceConfig then
                      serviceConfig
                      // {
                        service = transform serviceName serviceConfig.service;
                      }
                    else
                      serviceConfig
                  ) services;
                in
                projectConfig
                // {
                  settings = (projectConfig.settings or { }) // {
                    services = transformedServices;
                  };
                }
              ) nirionConfig.projects;
          };
        };
    };
}
