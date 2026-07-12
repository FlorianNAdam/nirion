{
  pkgs,
  self,
}:
let
  common = rec {
    inherit self;

    imageName = "nirion-test-http";
    imageRef = "${imageName}:latest";

    mkHttpImage =
      pkgs:
      pkgs.dockerTools.buildLayeredImage {
        name = imageName;
        tag = "latest";
        contents = [ pkgs.busybox ];
        config.Cmd = [
          "/bin/sh"
          "-c"
          ''while true; do printf 'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok' | nc -l -p 8080; done''
        ];
      };

    mkBaseMachine = pkgs: nirionConfig: {
      imports = [ self.nixosModules.nirion ];

      system.stateVersion = "26.05";

      virtualisation.memorySize = 2048;
      virtualisation.diskSize = 4096;

      environment.systemPackages = [ pkgs.curl ];

      virtualisation.nirion = {
        lockFile = builtins.toFile "nirion-lock.json" "{}";
        lockFileOutput = "/var/lib/nirion/lock.json";
      }
      // nirionConfig;
    };

    loadImageScript = image: ''
      machine.wait_for_unit("docker.service")
      machine.succeed("docker load < ${image}")
    '';

    nirionHelper = ''
      def nirion(command):
          return machine.succeed(f"nirion {command}")
    '';

    longRunningCommand = [
      "/bin/sh"
      "-c"
      "while true; do sleep 3600; done"
    ];
  };

  tests = {
    basic = ./basic.nix;
    cli-lifecycle = ./cli-lifecycle.nix;
    multi-project = ./multi-project.nix;
    sops = ./sops.nix;
    sops-compose-template = ./sops-compose-template.nix;
  };
in
builtins.listToAttrs (
  map (test: {
    name = "vm-${test}";
    value = pkgs.testers.runNixOSTest {
      imports = [ (import tests.${test} common) ];
    };
  }) (builtins.attrNames tests)
)
