{
  config,
  lib,
  options,
  ...
}:

let
  inherit (lib) mkOption types;
  configuredOptions = lib.filterAttrs (_: option: option.highestPrio < 1500);

  capAdd = lib.attrNames (lib.filterAttrs (_: value: value == true) config.capabilities);
  capDrop = lib.attrNames (lib.filterAttrs (_: value: value == false) config.capabilities);

  optionValues = opts: lib.mapAttrs (_: option: option.value) (configuredOptions opts);

  renderedBuild = optionValues (
    builtins.intersectAttrs options.build {
      context = null;
      dockerfile = null;
      target = null;
      args = null;
    }
  );

  renderedHealthcheck =
    if config.healthcheck.test != null || config.healthcheck.disable != null then
      lib.filterAttrs (_: value: value != null) {
        inherit (config.healthcheck)
          test
          interval
          timeout
          start_period
          retries
          disable
          ;
      }
    else
      { };

  healthcheckType = types.submodule {
    options = {
      test = mkOption {
        type = types.nullOr (types.listOf types.str);
        default = null;
      };
      interval = mkOption {
        type = types.str;
        default = "30s";
      };
      timeout = mkOption {
        type = types.str;
        default = "30s";
      };
      start_period = mkOption {
        type = types.str;
        default = "0s";
      };
      retries = mkOption {
        type = types.int;
        default = 3;
      };
      disable = mkOption {
        type = types.nullOr types.bool;
        default = null;
      };
    };
  };

  buildType = types.submodule {
    options = {
      context = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      dockerfile = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      target = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      args = mkOption {
        type = types.attrsOf types.anything;
        default = { };
      };
    };
  };
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
      type = buildType;
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
      type = types.attrsOf (types.either types.str types.int);
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
      type = healthcheckType;
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
    out.compose = mkOption {
      type = types.attrsOf types.anything;
      readOnly = true;
      internal = true;
    };
  };

  config.out.compose =
    lib.mapAttrs (_: option: option.value) (configuredOptions {
      inherit (options)
        command
        entrypoint
        container_name
        hostname
        user
        working_dir
        environment
        env_file
        labels
        ports
        expose
        volumes
        tmpfs
        devices
        depends_on
        restart
        stop_signal
        stop_grace_period
        privileged
        tty
        dns
        extra_hosts
        links
        external_links
        network_mode
        networks
        sysctls
        blkio_config
        ;
    })
    // lib.optionalAttrs (renderedBuild != { }) {
      build = renderedBuild;
    }
    // lib.optionalAttrs (renderedHealthcheck != { }) {
      healthcheck = renderedHealthcheck;
    }
    // lib.optionalAttrs (capAdd != [ ]) {
      cap_add = capAdd;
    }
    // lib.optionalAttrs (capDrop != [ ]) {
      cap_drop = capDrop;
    }
    // config.extraOptions;
}
