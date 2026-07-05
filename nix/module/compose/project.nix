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
