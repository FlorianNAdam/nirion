{
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    {
      virtualisation.nirion = baseNirionConfig // {
        projects.secret = {
          sops.group.gid = 9004;
          services.app.extraOptions.image = "example.invalid/app:latest";
        };
      };
    }
  ];

  service = system.config.virtualisation.nirion.out.compose.secret.attrs.services.app;
in
[
  {
    assertion = system.config.users.groups."nirion-secret".gid == 9004;
    message = "sops group was not created with the default project group name";
  }
  {
    assertion = service.group_add == [ "9004" ];
    message = "sops group gid was not added to rendered services";
  }
]
