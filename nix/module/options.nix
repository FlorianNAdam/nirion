{ config, lib }:

{
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

    # Sops
    enableSops = lib.mkOption {
      type = lib.types.bool;
      default = config ? sops;
      description = "Enable the sops integration";
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
}
