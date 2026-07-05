# Nirion

**Nirion** is a Docker Compose manager for simpler Nix-based home lab Docker setups and management.  
It streamlines container management, Nix evaluation, and service patching for reproducible and maintainable workflows.\
By adding a lock-file mechanism, it makes deployments even more deterministic and reproducible.

---

## Features

- Start, stop, and manage Docker services with ease
- NixOS module for Docker Compose projects
- Lock file support for deterministic deployments
- Patch service files using `mirage-patch`
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
  # lock file readable by nix / nirion
  lockFile = ./nirion.lock;

  # lock file writable by nirion
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

### The Lock File

One of the core features of nirion is the ability to lock docker images to specific commits and update them.\
To use this feature simply use `nirion lock` to create/populate the lock file.\
Nirion will automatically use locked images if possible.
To update images simply use `nirion update` to update the lock file and then rebuild the system.

### NixOS Module Behavior

The NixOS module uses Docker Compose v2 (`docker compose`) for generated systemd units.
The current Rust CLI still shells out to the legacy `docker-compose` binary in some commands; that compatibility will be cleaned up separately.

`virtualisation.nirion.enableSops` is intentionally opt-in. If it is enabled, a module that provides `sops.templates`, such as sops-nix, must also be imported.

Generated systemd services currently run `docker compose up -d` during start. Stop/reload behavior is intentionally minimal for now and should be expanded separately if Nirion should fully manage service lifecycle.


## Nirion-Cli

```bash
nirion [OPTIONS] <COMMAND>
```

### Commands

| Command        | Description                                           |
| -------------- | ----------------------------------------------------- |
| `up`           | Create and start service containers                   |
| `down`         | Stop and remove service containers and networks       |
| `list`         | List projects or services                             |
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
| `patch`        | Patch service files using `mirage-patch`              |
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

Patch service files:

```bash
nirion patch
```

---

## License

[MIT License](LICENSE)
