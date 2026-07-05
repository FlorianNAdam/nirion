{
  config,
  lib,
  options,
  ...
}:

let
  inherit (lib) mkOption types;
  configuredOptions = lib.filterAttrs (_: option: option.highestPrio < 1500);
in
{
  options = {
    name = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    driver = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    driver_opts = mkOption {
      type = types.attrsOf types.str;
      default = { };
    };
    external = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    internal = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    attachable = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    enable_ipv6 = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    ipam = mkOption {
      type = types.nullOr types.anything;
      default = null;
    };
    labels = mkOption {
      type = types.attrsOf types.str;
      default = { };
    };
    extraOptions = mkOption {
      type = types.attrsOf types.anything;
      default = { };
    };
    out.compose = mkOption {
      type = types.attrsOf types.anything;
      readOnly = true;
      internal = true;
    };
  };

  config.out.compose =
    lib.mapAttrs (_: option: option.value) (configuredOptions {
      inherit (options)
        name
        driver
        driver_opts
        external
        internal
        attachable
        enable_ipv6
        ipam
        labels
        ;
    })
    // config.extraOptions;
}
