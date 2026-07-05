{
  config,
  lib,
  ...
}:

let
  inherit (lib) mkOption types;
  composeTypes = import ./types.nix { inherit lib; };
in
{
  options = {
    image = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    lockedImage = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    build = mkOption {
      type = composeTypes.build;
      default = { };
    };
    command = mkOption {
      type = types.nullOr types.anything;
      default = null;
    };
    entrypoint = mkOption {
      type = types.nullOr types.anything;
      default = null;
    };
    container_name = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    hostname = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    user = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    working_dir = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    environment = mkOption {
      type = types.attrsOf composeTypes.environmentValue;
      default = { };
    };
    env_file = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    labels = mkOption {
      type = types.attrsOf types.str;
      default = { };
    };
    ports = mkOption {
      type = types.listOf types.anything;
      default = [ ];
    };
    expose = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    volumes = mkOption {
      type = types.listOf types.anything;
      default = [ ];
    };
    tmpfs = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    devices = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    depends_on = mkOption {
      type = types.either (types.listOf types.str) (types.attrsOf types.anything);
      default = [ ];
    };
    healthcheck = mkOption {
      type = composeTypes.healthcheck;
      default = { };
    };
    restart = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    stop_signal = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    stop_grace_period = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    privileged = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    tty = mkOption {
      type = types.nullOr types.bool;
      default = null;
    };
    dns = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    extra_hosts = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    links = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    external_links = mkOption {
      type = types.listOf types.str;
      default = [ ];
    };
    network_mode = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    networks = mkOption {
      type = types.either (types.listOf types.str) (types.attrsOf types.anything);
      default = [ ];
    };
    sysctls = mkOption {
      type = types.attrsOf (types.either types.str types.int);
      default = { };
    };
    capabilities = mkOption {
      type = types.attrsOf (types.nullOr types.bool);
      default = { };
    };
    blkio_config = mkOption {
      type = types.nullOr types.anything;
      default = null;
    };
    extraOptions = mkOption {
      type = types.attrsOf types.anything;
      default = { };
    };
  };
}
