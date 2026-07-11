{
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        nixEval.target = "/etc/nixos#nixosConfigurations.host";
      };
    }
  ];
in
[
  {
    assertion =
      system.config.environment.variables.NIRION_NIX_TARGET == "/etc/nixos#nixosConfigurations.host";
    message = "nixEval.target was not exposed as NIRION_NIX_TARGET";
  }
]
