{ lib }:

let
  inherit (lib) mkOption types;
in
{
  lockFile = mkOption {
    type = types.path;
    description = "Path to image lock file.";
  };

  lockFileOutput = mkOption {
    type = types.str;
    description = "Writable output path for lock file updates.";
  };

  authFile = mkOption {
    type = types.nullOr types.path;
    default = null;
    description = "Optional path to OCI registry auth config.";
  };

  nixEval = {
    target = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    rawTarget = mkOption {
      type = types.nullOr types.str;
      default = null;
    };
    nixos = {
      config = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      host = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
    };
  };

  projects = mkOption {
    type = types.attrsOf (types.submodule ./compose/project.nix);
    default = { };
    description = "Nirion Docker Compose projects.";
  };

  sops = {
    overrideComposeFile = mkOption {
      type = types.bool;
      default = false;
      description = "Write compose files through sops-nix templates. This is intentionally opt-in.";
    };
  };

  images = mkOption {
    type = types.attrsOf types.str;
    default = { };
    description = "Image references to resolve with lock file entries.";
  };

  out = {
    images_v2 = mkOption {
      type = types.attrsOf types.anything;
      readOnly = true;
      internal = true;
    };
    locked_images = mkOption {
      type = types.attrsOf types.str;
      readOnly = true;
      internal = true;
    };
    projects = mkOption {
      type = types.attrsOf types.anything;
      readOnly = true;
      internal = true;
    };
    compose = mkOption {
      type = types.attrsOf types.anything;
      readOnly = true;
      internal = true;
    };
    projectsFile = mkOption {
      type = types.str;
      readOnly = true;
      internal = true;
    };
    projectsFileStatic = mkOption {
      type = types.str;
      readOnly = true;
      internal = true;
    };
  };
}
