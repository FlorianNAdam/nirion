{
  lib,
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  projectSopsFile = builtins.toFile "project-secrets.yaml" "{}";

  fakeSopsModule = {
    options.sops = {
      secrets = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule {
            options = {
              owner = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
              };
              group = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
              };
              mode = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
              };
              sopsFile = lib.mkOption {
                type = lib.types.nullOr lib.types.path;
                default = null;
              };
              reloadUnits = lib.mkOption {
                type = lib.types.listOf lib.types.str;
                default = [ ];
              };
            };
          }
        );
        default = { };
      };
      templates = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule (
            { name, ... }: {
              options = {
                path = lib.mkOption {
                  type = lib.types.str;
                  default = "/run/sops/templates/${name}";
                };
                content = lib.mkOption {
                  type = lib.types.lines;
                  default = "";
                };
                mode = lib.mkOption {
                  type = lib.types.str;
                  default = "0400";
                };
                uid = lib.mkOption {
                  type = lib.types.nullOr lib.types.int;
                  default = null;
                };
                gid = lib.mkOption {
                  type = lib.types.nullOr lib.types.int;
                  default = null;
                };
                reloadUnits = lib.mkOption {
                  type = lib.types.listOf lib.types.str;
                  default = [ ];
                };
                restartUnits = lib.mkOption {
                  type = lib.types.listOf lib.types.str;
                  default = [ ];
                };
                owner = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  default = null;
                };
                group = lib.mkOption {
                  type = lib.types.nullOr lib.types.str;
                  default = null;
                };
              };
            }
          )
        );
        default = { };
      };
      placeholder = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
      };
    };

    config.sops.placeholder."app/password" = "placeholder-app-password";
    config.sops.placeholder."custom/token" = "placeholder-custom-token";
  };

  system = evalConfig [
    fakeSopsModule
    ({ config, ... }: {
      virtualisation.nirion = baseNirionConfig // {
        sops.overrideComposeFile = true;

        projects.secret = {
          sops = {
            file = projectSopsFile;
            group = {
              name = "apps-secrets";
              gid = 9004;
            };
            secrets = {
              "app/password" = { };
              "custom/token" = {
                owner = "app";
                mode = "0400";
                reloadUnits = [ "custom-reload.service" ];
              };
            };
            templates = {
              "app.env".content = ''
                PASSWORD=${config.sops.placeholder."app/password"}
              '';
              "custom.env" = {
                content = ''
                  TOKEN=${config.sops.placeholder."custom/token"}
                '';
                mode = "0400";
                owner = "app";
                group = "app";
                reloadUnits = [ "custom-reload.service" ];
                restartUnits = [ "custom-restart.service" ];
              };
            };
          };

          services.app = {
            extraOptions.image = "example.invalid/app:latest";
            env_file = [ "/run/secrets/app.env" ];
          };
        };

        projects.no-reload = {
          sops = {
            reloadOnChange = false;
            group.gid = 9005;
            secrets."no-reload/password" = { };
            templates."no-reload.env".content = "PASSWORD=placeholder";
          };
          services.app.extraOptions.image = "example.invalid/no-reload:latest";
        };
      };
    })
  ];

  cfg = system.config;
  service = system.config.virtualisation.nirion.out.compose.secret.attrs.services.app;
  noReloadService = system.config.virtualisation.nirion.out.compose.no-reload.attrs.services.app;
in
[
  {
    assertion = cfg.users.groups."apps-secrets".gid == 9004;
    message = "sops group was not created with the configured group name";
  }
  {
    assertion = service.group_add == [ "9004" ];
    message = "sops group gid was not added to rendered services";
  }
  {
    assertion =
      cfg.sops.secrets."app/password".owner == "root"
      && cfg.sops.secrets."app/password".group == "apps-secrets"
      && cfg.sops.secrets."app/password".mode == "0440"
      && cfg.sops.secrets."app/password".sopsFile == projectSopsFile
      && cfg.sops.secrets."app/password".reloadUnits == [ "nirion-secret.service" ];
    message = "project sops secret defaults and sopsFile were not forwarded";
  }
  {
    assertion =
      cfg.sops.secrets."custom/token".owner == "app"
      && cfg.sops.secrets."custom/token".group == "apps-secrets"
      && cfg.sops.secrets."custom/token".mode == "0400"
      && cfg.sops.secrets."custom/token".sopsFile == projectSopsFile
      &&
        cfg.sops.secrets."custom/token".reloadUnits == [
          "custom-reload.service"
          "nirion-secret.service"
        ];
    message = "project sops secret overrides were not preserved";
  }
  {
    assertion =
      cfg.sops.templates."app.env".content == ''
        PASSWORD=placeholder-app-password
      ''
      && cfg.sops.templates."app.env".mode == "0440"
      && cfg.sops.templates."app.env".uid == 0
      && cfg.sops.templates."app.env".gid == 0
      && cfg.sops.templates."app.env".reloadUnits == [ "nirion-secret.service" ]
      && cfg.sops.templates."app.env".restartUnits == [ ]
      && cfg.sops.templates."app.env".owner == "root"
      && cfg.sops.templates."app.env".group == "apps-secrets";
    message = "project sops template defaults were not forwarded";
  }
  {
    assertion =
      cfg.sops.templates."custom.env".content == ''
        TOKEN=placeholder-custom-token
      ''
      && cfg.sops.templates."custom.env".mode == "0440"
      && cfg.sops.templates."custom.env".owner == "root"
      && cfg.sops.templates."custom.env".group == "apps-secrets"
      &&
        cfg.sops.templates."custom.env".reloadUnits == [
          "custom-reload.service"
          "nirion-secret.service"
        ]
      && cfg.sops.templates."custom.env".restartUnits == [ "custom-restart.service" ];
    message = "project sops template content or unit settings were not preserved";
  }
  {
    assertion =
      cfg.sops.templates."nirion/secret/compose.yaml".content
      == cfg.virtualisation.nirion.out.compose.secret.text;
    message = "overrideComposeFile did not forward the generated compose file as a sops template";
  }
  {
    assertion =
      cfg.virtualisation.nirion.out.projects.secret.dockerCompose
      == cfg.sops.templates."nirion/secret/compose.yaml".path;
    message = "overrideComposeFile did not point project metadata at the sops template path";
  }
  {
    assertion =
      cfg.users.groups."nirion-no-reload".gid == 9005 && noReloadService.group_add == [ "9005" ];
    message = "default sops group was not created for the no-reload project";
  }
  {
    assertion = cfg.sops.secrets."no-reload/password".reloadUnits == [ ];
    message = "sops.reloadOnChange = false should not add secret reloadUnits";
  }
  {
    assertion = cfg.sops.templates."no-reload.env".reloadUnits == [ ];
    message = "sops.reloadOnChange = false should not add template reloadUnits";
  }
  {
    assertion = cfg.sops.templates."nirion/no-reload/compose.yaml".reloadUnits == [ ];
    message = "sops.reloadOnChange = false should not add compose template reloadUnits";
  }
]
