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

  nodes.machine =
    { ... }:
    {
      imports = [
        (mkBaseMachine pkgs {
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
        })
      ];

      systemd.services.nirion-target-test = {
        wantedBy = [ "nirion.target" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${pkgs.coreutils}/bin/true";
          ExecReload = "${pkgs.coreutils}/bin/sleep 5";
        };
      };
    };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}

    machine.succeed("systemctl restart nirion-web.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")

    machine.wait_for_unit("nirion-target-test.service")
    machine.succeed("systemctl show nirion.target -p Wants --value | grep nirion-target-test.service")
    machine.fail("systemctl show nirion.target -p After --value | grep nirion-target-test.service")
    machine.succeed("systemctl reload --no-block nirion-target-test.service")
    machine.wait_until_succeeds("test \"$(systemctl show nirion-target-test.service -p ActiveState --value)\" = reloading")
    machine.succeed("timeout 2 systemctl start nirion.target multi-user.target")
    machine.succeed("test \"$(systemctl show nirion-target-test.service -p ActiveState --value)\" = reloading")
    machine.wait_until_succeeds("test \"$(systemctl show nirion-target-test.service -p SubState --value)\" = exited")

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
