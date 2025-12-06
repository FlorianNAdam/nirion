{
  description = "Convenience wrapper for arion";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      naersk,
    }:
    {
      nixosModules.nirion-v2 =
        { config, lib, ... }:
        let
          cfg = config.virtualisation.nirion;
        in
        {
          options.virtualisation.nirion.projects = lib.mkOption {
            type = lib.types.attrsOf (lib.types.anything);
            default = { };
            description = "Arion project configuration with lockfile support";
          };

          config.virtualisation.arion.projects = lib.mapAttrs (
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
                  if builtins.hasAttr attrName cfg.out.locked_images then
                    {
                      service = (builtins.removeAttrs (serviceConfig.service or { }) [ "locked_image" ]) // {
                        image = cfg.out.locked_images.${attrName};
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
          ) cfg.projects;
        };

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

          projectMapping = lib.concatStringsSep "\n" (
            builtins.attrValues (
              lib.attrsets.mapAttrs' (name: project: {
                inherit name;
                value = "${name}=${project.settings.out.dockerComposeYaml}";
              }) arionConfig.projects
            )
          );

          imageNameRefs = lib.concatStringsSep "\n" (
            lib.attrsets.mapAttrsToList (name: ref: "${name}=${ref}") nirionConfig.images
          );

          lockFileOutputStr =
            if nirionConfig.lockFileOutput != null then toString nirionConfig.lockFileOutput else "";

          nirionScript = pkgs.writeScriptBin "nirion" ''
            #!/usr/bin/env bash
            set -e

            # Project to YAML mapping
            declare -A PROJECTS
            while IFS='=' read -r name yaml; do
              PROJECTS["$name"]="$yaml"
            done <<< "${projectMapping}"

            # Handle 'update' command
            if [[ "$1" == "update" ]]; then
              shift  # Remove 'update' from arguments

              # Check if lockfile is enabled
              if [[ -z "${lockFileOutputStr}" ]]; then
                echo "Error: Lockfile functionality is not enabled (nirion.lockFileOutput is not set)"
                exit 1
              fi

              # Read image name->ref mapping
              declare -A IMAGE_MAP
              while IFS='=' read -r name ref; do
                IMAGE_MAP["$name"]="$ref"
              done <<< "${imageNameRefs}"

              # Determine images to update
              if [[ $# -gt 0 ]]; then
                # Validate provided image names
                IMAGES_TO_UPDATE=()
                for name in "$@"; do
                  if [[ -z "''${IMAGE_MAP[$name]}" ]]; then
                    echo "Error: Unknown image name '$name'. Available images:"
                    printf "  - %s\n" "''${!IMAGE_MAP[@]}"
                    exit 1
                  fi
                  IMAGES_TO_UPDATE+=("$name")
                done
              else
                # Update all images
                IMAGES_TO_UPDATE=("''${!IMAGE_MAP[@]}")
              fi

              LOCKFILE="${lockFileOutputStr}"

              # Read existing lockfile
              declare -A LOCKED
              if [[ -f "$LOCKFILE" ]]; then
                while IFS="=" read -r key digest; do
                  LOCKED["$key"]="$digest"
                done < <(${pkgs.jq}/bin/jq -r 'to_entries|map("\(.key)=\(.value)")|.[]' "$LOCKFILE")
              fi

              # Update each selected image
              for NAME in "''${IMAGES_TO_UPDATE[@]}"; do
                IMAGE="''${IMAGE_MAP[$NAME]}"
                echo "Resolving digest for $NAME ($IMAGE)"
                DIGEST=$(${pkgs.skopeo}/bin/skopeo inspect --format "{{.Digest}}" "docker://$IMAGE" || echo "failed")
                if [[ "$DIGEST" == "failed" ]]; then
                  echo "Error resolving digest for $IMAGE. Skipping."
                  continue
                fi
                OLD_DIGEST="''${LOCKED[$NAME]-}"
                if [[ -n "$OLD_DIGEST" ]]; then
                  if [[ "$OLD_DIGEST" != "$DIGEST" ]]; then
                    echo "Digest changed for $NAME: $OLD_DIGEST -> $DIGEST"
                  fi
                else
                  echo "Added digest for $NAME: $DIGEST"
                fi
                LOCKED["$NAME"]="$DIGEST"
              done

              # Write new lockfile
              echo "Updating lockfile $LOCKFILE"
              rm -f "$LOCKFILE.tmp"
              for key in "''${!LOCKED[@]}"; do
                echo "$key ''${LOCKED[$key]}" >> "$LOCKFILE.tmp"
              done
              ${pkgs.jq}/bin/jq -Rn 'reduce inputs as $line ({}; ($line | split(" ") ) as $parts | . + { ($parts[0]): $parts[1] })' "$LOCKFILE.tmp" > "$LOCKFILE"
              rm -f "$LOCKFILE.tmp"

              exit 0
            fi

            # Handle 'list' command
            if [[ "$1" == "list" ]]; then
              echo "Available Arion projects:"
              for proj in "''${!PROJECTS[@]}"; do
                echo "  - $proj"
              done
              exit 0
            fi

            # Ensure at least one argument is provided
            if [[ $# -lt 1 ]]; then
              echo "Usage: nirion <command> [options] [project]"
              echo "       nirion list          # Show available projects"
              echo "       nirion update [image...]  # Update lockfile for specified/all images"
              exit 1
            fi

            # Extract the last argument as project name (if provided)
            LAST_ARG="''${!#}"

            # Check if the last argument is a valid project name
            if [[ -n "''${PROJECTS[$LAST_ARG]}" ]]; then
              PROJECTS_TO_RUN=("$LAST_ARG")  # Single project mode
              ARION_COMMAND="''${@:1:$#-1}"  # All args except the last one
            else
              PROJECTS_TO_RUN=("''${!PROJECTS[@]}")  # Run for all projects
              ARION_COMMAND="$@"  # Full command
            fi

            # Execute Arion for all selected projects
            for PROJECT in "''${PROJECTS_TO_RUN[@]}"; do
              echo "Arion project: $PROJECT"
              ${pkgs.expect}/bin/unbuffer arion --prebuilt-file "''${PROJECTS[$PROJECT]}" $ARION_COMMAND | grep -v "the attribute \`version\` is obsolete"
              echo
            done
          '';

          nirion-v2 = pkgs.stdenv.mkDerivation {
            name = "nirion-v2";

            src = "${nirionPkg}";

            buildInputs = [ pkgs.makeWrapper ];

            installPhase = ''
              mkdir -p $out/bin
              makeWrapper ${nirionPkg}/bin/nirion $out/bin/nirion-v2 \
                --set NIRION_LOCK_FILE "${nirionConfig.lockFileOutput}" \
                --set NIRION_PROJECT_FILE "${nirionConfig.out.projectsFile}"
            '';
          };
        in
        {
          imports = [
            self.nixosModules.nirion-v2
          ];

          options = {
            virtualisation.nirion = {
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

              out.projects = lib.mapAttrs (name: project: {
                docker-compose = arionConfig.projects.${name}.settings.out.dockerComposeYaml;
                services = nirionConfig.out.images_v2.${name} or { };
              }) nirionConfig.projects;

              out.projectsFile =
                let
                  json = builtins.toJSON nirionConfig.out.projects;
                  file = pkgs.writeText "projects.json" "${json}";
                in
                "${file}";
            };

            environment.systemPackages = [
              nirionScript
              nirion-v2
            ];
          };
        };
    }
    // flake-utils.lib.eachDefaultSystem (
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
            sqlx-cli
            skopeo
          ];

          shellHook =
            let
              compose = {
                "networks" = {
                  "default" = {
                    "name" = "hello-world";
                  };
                  "dmz" = {
                    "external" = true;
                    "name" = "dmz";
                  };
                };
                "services" = {
                  "hello-world-service" = {
                    "container_name" = "hello-world";
                    "environment" = { };
                    "image" = "library/hello-world";
                    "networks" = [ "dmz" ];
                    "restart" = "always";
                    "sysctls" = { };
                    "volumes" = [ ];
                  };
                };
                "version" = "3.4";
                "volumes" = { };
              };

              projects = {
                "hello-world" = {
                  "docker-compose" = "/nix/store/a3hp9zdwg9w9x1fq7dh71alccgbcdacj-docker-compose.yaml";
                  "images" = {
                    "hello-world" = "library/hello-world";
                  };
                };
                "hello-world-project" = {
                  "docker-compose" = "${pkgs.writeText "project.json" (builtins.toJSON compose)}";
                  "images" = {
                    "hello-world-service-0" = "library/hello-world";
                    "hello-world-service-1" = "library/hello-world";
                    "hello-world-service-2" = "library/hello-world";
                    "hello-world-service-3" = "library/hello-world";
                    "hello-world-service-4" = "library/hello-world";
                    "hello-world-service-5" = "library/hello-world";
                    "hello-world-service-6" = "library/hello-world";
                    "hello-world-service-7" = "library/hello-world";
                  };
                };
              };
            in
            ''
              export NIRION_LOCK_FILE="/home/florian/my-nixos/modules/arion/nirion.lock"
              export NIRION_PROJECT_FILE="${pkgs.writeText "project.json" (builtins.toJSON projects)}"
            '';
        };
      }
    );
}
