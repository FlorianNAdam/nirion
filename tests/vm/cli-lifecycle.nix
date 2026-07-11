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
  name = "nirion-vm-cli-lifecycle";

  nodes.machine = mkBaseMachine pkgs {
    projects.web.services = {
      http = {
        extraOptions.image = imageRef;
        ports = [ "18080:8080" ];
      };

      worker = {
        extraOptions.image = imageRef;
        command = longRunningCommand;
      };
    };
  };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}
    machine.succeed("systemctl stop nirion-web.service || true")

    nirion("up --no-tui web")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("stop --no-tui web")
    machine.wait_until_fails("curl --fail http://localhost:18080")

    nirion("start --no-tui web")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("restart --no-tui web")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("reload --no-tui web")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("stop --no-tui web.http")
    machine.wait_until_fails("curl --fail http://localhost:18080")
    nirion("start --no-tui web.http")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    nirion("down --no-tui web")
    machine.wait_until_fails("curl --fail http://localhost:18080")
  '';
}
