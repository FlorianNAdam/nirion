{
  config,
  lib,
  name,
  ...
}:

let
  inherit (lib) mkOption types;
  serviceModule = import ./service.nix;
  networkModule = import ./network.nix;
  sopsGroupType = types.submodule {
    options = {
      name = mkOption {
        type = types.str;
        default = "nirion-${name}";
        defaultText = lib.literalExpression ''"nirion-<project-name>"'';
        description = "Project secret read-access group name.";
      };
      gid = mkOption {
        type = types.int;
        description = "Project secret read-access group ID.";
      };
    };
  };
  sopsTemplateType = types.submodule {
    options = {
      content = mkOption {
        type = types.lines;
        default = "";
      };
      owner = mkOption {
        type = types.nullOr types.singleLineStr;
        default = null;
      };
      group = mkOption {
        type = types.nullOr types.singleLineStr;
        default = null;
      };
      mode = mkOption {
        type = types.singleLineStr;
        default = "0400";
      };
      reloadUnits = mkOption {
        type = types.listOf types.str;
        default = [ ];
      };
      restartUnits = mkOption {
        type = types.listOf types.str;
        default = [ ];
      };
    };
  };
  sopsType = types.submodule {
    options = {
      file = mkOption {
        type = types.nullOr types.path;
        default = null;
        description = "Default sops file for this project's secrets.";
      };
      group = mkOption {
        type = types.nullOr sopsGroupType;
        default = null;
        description = "Optional project secret read-access group.";
      };
      reloadOnChange = mkOption {
        type = types.bool;
        default = true;
        description = "Reload this project's systemd unit when project sops secrets or templates change.";
      };
      secrets = mkOption {
        type = types.attrsOf types.anything;
        default = { };
        description = "Project sops-nix secrets.";
      };
      templates = mkOption {
        type = types.attrsOf sopsTemplateType;
        default = { };
        description = "Project sops-nix templates.";
      };
    };
  };
  defaultNetwork = lib.optionalAttrs config.enableDefaultNetwork {
    default = {
      name = config.composeProjectName;
    };
  };
  networks = defaultNetwork // (lib.mapAttrs (_: network: network.out.compose) config.networks);
in
{
  options = {
    composeProjectName = mkOption {
      type = types.str;
      default = name;
      description = "Docker Compose project name.";
    };
    enableDefaultNetwork = mkOption {
      type = types.bool;
      default = true;
    };
    services = mkOption {
      type = types.attrsOf (types.submodule serviceModule);
      default = { };
    };
    networks = mkOption {
      type = types.attrsOf (types.submodule networkModule);
      default = { };
    };
    volumes = mkOption {
      type = types.attrsOf types.anything;
      default = { };
    };
    extraOptions = mkOption {
      type = types.attrsOf types.anything;
      default = { };
    };
    sops = mkOption {
      type = sopsType;
      default = { };
      description = "Project-level sops-nix secret and template declarations.";
    };
    out = {
      networks = mkOption {
        type = types.attrsOf types.anything;
        readOnly = true;
        internal = true;
      };
      volumes = mkOption {
        type = types.attrsOf types.anything;
        readOnly = true;
        internal = true;
      };
    };
  };

  config = {
    out.networks = networks;
    out.volumes = config.volumes;
  };
}
