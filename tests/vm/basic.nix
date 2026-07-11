{ self }:

{ pkgs, ... }:

let
  testImage = pkgs.dockerTools.buildLayeredImage {
    name = "nirion-test-http";
    tag = "latest";
    contents = [ pkgs.busybox ];
    config.Cmd = [
      "/bin/sh"
      "-c"
      ''while true; do printf 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok' | nc -l -p 8080; done''
    ];
  };
in
{
  name = "nirion-vm-basic";

  nodes.machine = {
    imports = [ self.nixosModules.nirion ];

    system.stateVersion = "26.05";

    virtualisation.memorySize = 2048;
    virtualisation.diskSize = 4096;

    virtualisation.nirion = {
      lockFile = builtins.toFile "nirion-lock.json" "{}";
      lockFileOutput = "/var/lib/nirion/lock.json";

      projects.web.services.http = {
        # Keep the VM-local test image out of Nirion's lock-file image path.
        extraOptions.image = "nirion-test-http:latest";
        ports = [ "18080:8080" ];
      };
    };
  };

  testScript = ''
    machine.wait_for_unit("docker.service")
    machine.succeed("docker load < ${testImage}")
    machine.succeed("systemctl restart nirion-web.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18080")
    machine.succeed("systemctl stop nirion-web.service")
    machine.wait_until_fails("curl --fail http://localhost:18080")
  '';
}
