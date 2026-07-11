{
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  targetSystem = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        nixEval.target = "/etc/nixos#nixosConfigurations.host";
      };
    }
  ];

  rawTargetSystem = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        nixEval.rawTarget = "(import /etc/nixos).config.virtualisation.nirion.out.projectsFileStatic";
      };
    }
  ];

  nixosTargetSystem = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        nixEval.nixos = {
          config = "/home/florian/my-nixos";
          host = "server";
        };
      };
    }
  ];

  noTargetSystem = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig;
    }
  ];

  authSystem = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        authFile = "/run/secrets/nirion/auth.json";
        projects.web.services.nginx.extraOptions.image = "example.invalid/nginx:latest";
        nixEval.nixos = {
          config = "/home/florian/my-nixos";
          host = "server";
        };
      };
    }
  ];

  conflictingEval = builtins.tryEval (
    (evalConfig [
      {
        virtualisation.nirion = baseNirionConfig // {
          nixEval = {
            target = "/etc/nixos#nixosConfigurations.host";
            rawTarget = "(import /etc/nixos).config.virtualisation.nirion.out.projectsFileStatic";
          };
        };
      }
    ]).config.environment.variables.NIRION_NIX_TARGET
  );
in
[
  {
    assertion =
      targetSystem.config.environment.variables.NIRION_NIX_TARGET
      == "/etc/nixos#nixosConfigurations.host";
    message = "nixEval.target was not exposed as NIRION_NIX_TARGET";
  }
  {
    assertion = !(targetSystem.config.environment.variables ? NIRION_RAW_NIX_TARGET);
    message = "nixEval.target should not expose NIRION_RAW_NIX_TARGET";
  }
  {
    assertion =
      rawTargetSystem.config.environment.variables.NIRION_RAW_NIX_TARGET
      == "(import /etc/nixos).config.virtualisation.nirion.out.projectsFileStatic";
    message = "nixEval.rawTarget was not exposed as NIRION_RAW_NIX_TARGET";
  }
  {
    assertion = !(rawTargetSystem.config.environment.variables ? NIRION_NIX_TARGET);
    message = "nixEval.rawTarget should not expose NIRION_NIX_TARGET";
  }
  {
    assertion =
      nixosTargetSystem.config.environment.variables.NIRION_NIX_TARGET
      == "/home/florian/my-nixos#nixosConfigurations.server";
    message = "nixEval.nixos was not rendered as a flake NixOS target";
  }
  {
    assertion =
      noTargetSystem.config.environment.variables.NIRION_LOCK_FILE == "/var/lib/nirion/lock.json"
      && noTargetSystem.config.environment.variables.NIRION_PROJECT_FILE == "/etc/nirion/projects.json"
      && !(noTargetSystem.config.environment.variables ? NIRION_NIX_TARGET)
      && !(noTargetSystem.config.environment.variables ? NIRION_RAW_NIX_TARGET);
    message = "unset nixEval should only expose base Nirion environment variables";
  }
  {
    assertion =
      authSystem.config.environment.variables.NIRION_AUTH_FILE == "/run/secrets/nirion/auth.json"
      &&
        authSystem.config.environment.variables.NIRION_NIX_TARGET
        == "/home/florian/my-nixos#nixosConfigurations.server";
    message = "authFile and nixEval.nixos environment variables were not combined";
  }
  {
    assertion =
      authSystem.config.systemd.services.nirion-web.environment.NIRION_AUTH_FILE
      == "/run/secrets/nirion/auth.json"
      &&
        authSystem.config.systemd.services.nirion-web.environment.NIRION_NIX_TARGET
        == "/home/florian/my-nixos#nixosConfigurations.server";
    message = "systemd service did not inherit authFile and nixEval environment variables";
  }
  {
    assertion = conflictingEval.success == false;
    message = "conflicting nixEval modes should fail evaluation";
  }
]
