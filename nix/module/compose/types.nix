{ lib }:

let
  inherit (lib) types;
in
rec {
  primitive = types.oneOf [
    types.str
    types.int
    types.bool
    types.float
  ];

  environmentValue = primitive;

  healthcheck = types.submodule {
    options = {
      test = lib.mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
      };
      interval = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      timeout = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      start_period = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      retries = lib.mkOption {
        type = types.nullOr types.int;
        default = null;
      };
      disable = lib.mkOption {
        type = types.nullOr types.bool;
        default = null;
      };
    };
  };

  build = types.submodule {
    options = {
      context = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      dockerfile = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      target = lib.mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      args = lib.mkOption {
        type = types.attrsOf types.anything;
        default = { };
      };
    };
  };
}
