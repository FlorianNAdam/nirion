# Nirion

**Nirion** is a wrapper around [arion](https://docs.hercules-ci.com/arion/) for simpler Nix-based home lab Docker setups and management.  
It streamlines container management, Nix evaluation, and service patching for reproducible and maintainable workflows.

---

## Features

- Start, stop, and manage Docker services with ease
- Integrates with Nix for reproducible builds
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

If arion is added separately, nirions input **must** follow arion:

```nix
inputs = {
  arion = {
    url = "github:hercules-ci/arion";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  nirion = {
    url = "github:FlorianNAdam/nirion";
    inputs.nixpkgs.follows = "nixpkgs";
    inputs.arion.follows = "arion"; # ensures nirion tracks arion
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

The basics of nirion projects follow [arion](https://docs.hercules-ci.com/arion/) 1:1.\
The only different is the usage of `virtualisation.nirion` instead of `virtualisation.arion`

#### Minimal: Plain command using nixpkgs

```nix
virtualisation.nirion.projects.webapp.settings = {
  project.name = "webapp";
  services = {

    webserver = {
      image.enableRecommendedContents = true;
      service.useHostStore = true;
      service.command = [ "sh" "-c" ''
        cd "$$WEB_ROOT"
        ${pkgs.python3}/bin/python -m http.server
      ''];
      service.ports = [
        "8000:8000" # host:container
      ];
      service.environment.WEB_ROOT = "${pkgs.nix.doc}/share/doc/nix/manual";
      service.stop_signal = "SIGINT";
    };
  };
};
```

#### NixOS: run full OS

```nix
virtualisation.nirion.projects.some-project.settings = {
  project.name = "full-nixos";
  services.webserver = { pkgs, lib, ... }: {
    nixos.useSystemd = true;
    nixos.configuration.boot.tmp.useTmpfs = true;
    nixos.configuration.services.nginx.enable = true;
    nixos.configuration.services.nscd.enable = false;
    nixos.configuration.system.nssModules = lib.mkForce [];
    service.useHostStore = true;
    service.ports = [
      "8000:80" # host:container
    ];
  };
};
```

#### Docker image from DockerHub

```nix
virtualisation.nirion = {
  projects.hello-world.settings = {
    project.name = "hello-world";
    services = {
      hello-world.service = {
        image = "library/hello-world";
        container_name = "hello-world";
        restart = "always";
      };
    };
  };
};
```

### The Lock File

One of the core features of nirion is the ability to lock docker images to specific commits and update them.\
To use this feature simply use `nirion lock` to create/populate the lock file.\
Nirion will automatically use locked images if possible.
To update images simply use `nirion update` to update the lock file and then rebuild the system.


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
| `cat`          | Print the Docker Compose file as YAML                 |
| `ps`           | List running service containers                       |
| `top`          | Display running processes of a service container      |
| `volumes`      | List volumes                                          |
| `restart`      | Restart service containers                            |
| `compose-exec` | Run a docker-compose command for a project or service |
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

Print the Docker Compose YAML:

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
