# Nirion
[![check](https://img.shields.io/github/actions/workflow/status/FlorianNAdam/nirion/check.yml?branch=main&style=flat-square&label=check)](https://github.com/FlorianNAdam/nirion/actions/workflows/check.yml)
[![release](https://img.shields.io/github/v/release/FlorianNAdam/nirion?sort=semver&style=flat-square&label=release)](https://github.com/FlorianNAdam/nirion/releases)
[![nix flake](https://img.shields.io/badge/nix-flake-5277C3?style=flat-square&logo=nixos&logoColor=white)](#installation)
[![coverage](https://img.shields.io/coverallsCoverage/github/FlorianNAdam/nirion?branch=main&style=flat-square&label=coverage)](https://coveralls.io/github/FlorianNAdam/nirion?branch=main)

**Nirion** is a Docker Compose manager for simpler Nix-based home lab Docker setups and management.  
It streamlines container management and Nix evaluation for reproducible and maintainable workflows.\
By adding a lock-file mechanism, it makes deployments even more deterministic and reproducible.

---

## Features

- Start, stop, and manage Docker services with ease
- NixOS module for Docker Compose projects
- Lock file support for deterministic deployments
- Inspect and monitor running containers
- Works with Docker Compose under the hood

---

## Installation

Flake Integration

nirion can be integrated directly into a Nix flake.

```nix
inputs = {
  nirion = {
    url = "github:FlorianNAdam/nirion";
    inputs.nixpkgs.follows = "nixpkgs";
  };
};
```

## Configuration

nirion needs some configuration to work correctly:

```nix
virtualisation.nirion = {
  # required lock file readable by nix / nirion
  lockFile = ./nirion.lock;

  # required lock file writable by nirion
  lockFileOutput = "${host.homeDirectory}/my-nixos/nirion.lock";

  # path to the flake for dynamic reloads / evaluation
  nixEval.nixos = {
    config = "${host.homeDirectory}/my-nixos";
    host = "${host.name}";
  };
};
```

## Usage

### Projects

Nirion projects are Docker Compose projects defined directly in the NixOS module.
The module generates Compose JSON files. Docker Compose accepts these files even though they are not YAML.

#### Docker image from Docker Hub

```nix
virtualisation.nirion.projects.webapp = {
  services = {
    webserver = {
      image = "nginx:latest";
      ports = [
        "8000:8000" # host:container
      ];
      restart = "unless-stopped";
    };
  };
};
```

#### Compose project name override

```nix
virtualisation.nirion.projects.webapp = {
  composeProjectName = "webapp-prod";
  services.webserver.image = "nginx:latest";
};
```

#### Volumes and networks

```nix
virtualisation.nirion.projects.postgres = {
  services.db = {
    image = "postgres:16";
    volumes = [ "data:/var/lib/postgresql/data" ];
    environment.POSTGRES_DB = "app";
  };

  volumes.data = { };
};
```

#### Healthchecks

```nix
{ config, ... }:
{
  virtualisation.nirion.projects.web.services.nginx = {
    image = "nginx:latest";
    healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
      port = 80;
      path = "/";
      expect.status = 200;
    };
  };
}
```

#### SOPS secrets

Projects can declare sops-nix secrets and templates. If `sops.group` is set, Nirion creates the group, defaults generated secrets and templates to `root:<group>` with mode `0440`, and adds the group GID to every service in the project through Compose `group_add`.

```nix
{ config, ... }:
{
  virtualisation.nirion.projects.password = {
    sops = {
      file = ./secrets.yaml;

      group = {
        gid = 9004;
      };

      secrets."vaultwarden/admin_token" = { };

      templates."vaultwarden.env".content = ''
        ADMIN_TOKEN="${config.sops.placeholder."vaultwarden/admin_token"}"
      '';
    };

    services.vaultwarden = {
      image = "vaultwarden/server:latest";
      env_file = [
        config.sops.templates."vaultwarden.env".path
      ];
    };
  };
}
```

`sops.file` is optional and is used as the default `sopsFile` for the project's secrets. It does not apply to templates. `sops.group.name` defaults to `nirion-<project-name>`, while `sops.group.gid` must be set when a group is used. Project secret and template declarations are forwarded to the global `sops.secrets` and `sops.templates` options, so sops-nix must be imported when they are used.

Project sops secrets and templates reload the generated `nirion-<project>.service` unit by default when their materialized contents change. Any explicitly configured `reloadUnits` are preserved and the Nirion unit is appended. Generated compose-file templates do the same when `virtualisation.nirion.sops.overrideComposeFile` is enabled. Set `sops.reloadOnChange = false;` on a project to opt out.

### The Lock File

One of the core features of nirion is the ability to lock docker images to specific commits and update them.\
To use this feature simply use `nirion lock` to create/populate the lock file.\
Nirion will automatically use locked images if possible.
To update images simply use `nirion update` to update the lock file and then rebuild the system.

### NixOS Module Behavior

Generated systemd units call `nirion up --plain`, `nirion reload --plain`, and `nirion down --plain` for start, reload, and stop. Systemd restart uses stop plus start. The Rust CLI shells out to Docker Compose v2 (`docker compose`) under the hood.

`virtualisation.nirion.sops.overrideComposeFile` is intentionally opt-in. If it is enabled, generated compose files are written through sops-nix templates. A module that provides `sops.templates`, such as sops-nix, must also be imported.

Project-level sops secrets, templates, and generated compose-file templates add `nirion-<project>.service` to `reloadUnits` by default, so material changes reload the affected project unless `sops.reloadOnChange = false;` is set.


## Nirion-Cli

```bash
nirion [OPTIONS] <COMMAND>
```

### Commands

| Command        | Description                                           |
| -------------- | ----------------------------------------------------- |
| `up`           | Create and start service containers                   |
| `down`         | Stop and remove service containers and networks       |
| `reload`       | Stop and recreate service containers                  |
| `list`         | List projects or services                             |
| `pull`         | Pull service images                                   |
| `update`       | Update lock file entries                              |
| `lock`         | Create missing lock file entries                      |
| `exec`         | Execute a command in a running service container      |
| `logs`         | View output from service containers                   |
| `cat`          | Print the Docker Compose file                         |
| `ps`           | List running service containers                       |
| `top`          | Display running processes of a service container      |
| `volumes`      | List volumes                                          |
| `restart`      | Restart service containers                            |
| `compose-exec` | Run a Docker Compose command for a project or service |
| `monitor`      | Monitor running containers (TBD)                      |
| `inspect`      | Inspect images and services                           |
| `help`         | Print help message for commands                       |

### Options

| Option                              | Description                                     | Environment Variable  |
| ----------------------------------- | ----------------------------------------------- | --------------------- |
| `--lock-file <LOCK_FILE>`           | Path to the lock file                           | `NIRION_LOCK_FILE`    |
| `--project-file <PROJECT_FILE>`     | Path to the project file                        | `NIRION_PROJECT_FILE` |
| `--nix-eval`                        | Evaluate a Nix target to build the project file | —                     |
| `--nix-target <NIX_TARGET>`         | A Nix target to evaluate                        | `NIX_TARGET`          |
| `--raw-nix-target <RAW_NIX_TARGET>` | A raw Nix target to evaluate                    | `RAW_NIX_TARGET`      |
| `-h, --help`                        | Print help                                      | —                     |

---

## Examples

Start all services:

```bash
nirion up
```

Stop and remove all services:

```bash
nirion down
```

Run a command inside a running service:

```bash
nirion exec webserver.frontend bash
```

View logs of a service:

```bash
nirion logs application.db
```

Print the Docker Compose file:

```bash
nirion cat
```

## License

[MIT License](LICENSE)
