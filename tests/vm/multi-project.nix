{
  mkBaseMachine,
  mkHttpImage,
  loadImageScript,
  nirionHelper,
  imageRef,
  ...
}:

{ pkgs, ... }:

let
  testImage = mkHttpImage pkgs;
in
{
  name = "nirion-vm-multi-project";

  nodes.machine = mkBaseMachine pkgs {
    projects = {
      web.services.http = {
        extraOptions.image = imageRef;
        ports = [ "18080:8080" ];
      };

      admin = {
        composeProjectName = "nirion-admin-test";
        services.http = {
          extraOptions.image = imageRef;
          ports = [ "18081:8080" ];
        };
      };
    };
  };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}

    machine.succeed("systemctl restart nirion-web.service")
    machine.succeed("systemctl restart nirion-admin.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")
    machine.wait_until_succeeds("curl --fail http://localhost:18081")

    nirion("list | grep -- '- web'")
    nirion("list | grep -- '- admin'")
    machine.succeed("docker ps --format '{{.Names}}' | grep nirion-admin-test-http")

    nirion("down --no-tui admin")
    machine.wait_until_fails("curl --fail http://localhost:18081")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("up --no-tui admin")
    machine.wait_until_succeeds("curl --fail http://localhost:18081")

    machine.succeed("systemctl stop nirion-admin.service")
    machine.wait_until_fails("curl --fail http://localhost:18081")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    machine.succeed("systemctl stop nirion-web.service")
    machine.wait_until_fails("curl --fail http://localhost:18080")
  '';
}
