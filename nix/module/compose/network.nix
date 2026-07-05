{ config, lib, ... }:

let
  inherit (lib) mkOption types;
  nonEmpty = value: value != null && value != { };
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
    external = mkOption {
      type = types.nullOr (types.either types.bool types.anything);
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
    lib.filterAttrs (_: nonEmpty) {
      inherit (config)
        name
        driver
        external
        internal
        attachable
        enable_ipv6
        ipam
        labels
        ;
    }
    // config.extraOptions;
}
