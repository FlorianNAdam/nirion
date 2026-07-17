{
  mkBaseMachine,
  mkHttpImage,
  loadImageScript,
  nirionHelper,
  imageRef,
  longRunningCommand,
  ...
}:

{ pkgs, ... }:

let
  testImage = mkHttpImage pkgs;
in
{
  name = "nirion-vm-basic";

  nodes.machine = mkBaseMachine pkgs {
    projects.web.services = {
      http = {
        # Keep the VM-local test image out of Nirion's lock-file image path.
        extraOptions.image = imageRef;
        ports = [ "18080:8080" ];
      };

      worker = {
        extraOptions.image = imageRef;
        command = longRunningCommand;
        depends_on = [ "http" ];
      };
    };
  };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}

    machine.succeed("systemctl restart nirion-web.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("list | grep -- '- web'")
    nirion("list web | grep -- '- http'")
    nirion("list web | grep -- '- worker'")
    nirion("cat web | grep '18080:8080'")
    nirion("cat web.http | grep 'nirion-test-http:latest'")
    nirion("ps web | grep http")
    nirion("exec -T web.http -- /bin/sh -c 'echo exec-ok' | grep exec-ok")

    machine.succeed("systemctl reload nirion-web.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    machine.succeed("systemctl stop nirion-web.service")
    machine.wait_until_fails("curl --fail http://localhost:18080")
  '';
}
