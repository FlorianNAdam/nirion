{
  self,
  mkHttpImage,
  loadImageScript,
  nirionHelper,
  imageRef,
  ...
}:

{ pkgs, ... }:

let
  testImage = mkHttpImage pkgs;

  sopsNixSrc = pkgs.fetchzip {
    url = "https://github.com/Mic92/sops-nix/archive/f1406619a3884cd5c47992a70b8b35c9c0fcb4c9.tar.gz";
    hash = "sha256-aCWC8ngycU7OdJrU2+Je3qf+1a2ykuBvpPhZT/9tXMc=";
  };

  sopsFixture =
    pkgs.runCommand "nirion-vm-sops-compose-template-fixture"
      {
        nativeBuildInputs = [
          pkgs.age
          pkgs.sops
        ];
      }
      ''
        mkdir -p $out
        age-keygen -o $out/age-key.txt >/dev/null
        recipient=$(age-keygen -y $out/age-key.txt)

        cat > plain.yaml <<'EOF'
        app:
          token: vm-compose-template-token
        EOF

        SOPS_AGE_RECIPIENTS="$recipient" \
          sops --encrypt --input-type yaml --output-type yaml plain.yaml > $out/secrets.yaml
      '';
in
{
  name = "nirion-vm-sops-compose-template";

  nodes.machine =
    { config, ... }:
    {
      imports = [
        self.nixosModules.nirion
        "${sopsNixSrc}/modules/sops"
      ];

      system.stateVersion = "26.05";

      virtualisation.memorySize = 2048;
      virtualisation.diskSize = 4096;

      environment.systemPackages = [ pkgs.curl ];
      environment.etc."nirion-vm-sops-compose-template-age-key.txt".source = "${sopsFixture}/age-key.txt";

      sops = {
        age.keyFile = "/etc/nirion-vm-sops-compose-template-age-key.txt";
        defaultSopsFile = "${sopsFixture}/secrets.yaml";
      };

      virtualisation.nirion = {
        lockFile = builtins.toFile "nirion-lock.json" "{}";
        lockFileOutput = "/var/lib/nirion/lock.json";

        sops.overrideComposeFile = true;

        projects.web = {
          sops = {
            file = "${sopsFixture}/secrets.yaml";
            secrets."app/token" = { };
            templates."app.env".content = ''
              SECRET_TOKEN=${config.sops.placeholder."app/token"}
            '';
          };

          services.app = {
            extraOptions.image = imageRef;
            ports = [ "18083:8080" ];
            env_file = [ config.sops.templates."app.env".path ];
            command = [
              "/bin/sh"
              "-c"
              ''
                test "$$SECRET_TOKEN" = "vm-compose-template-token"
                while true; do printf 'HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\ncompose-ok' | nc -l -p 8080; done
              ''
            ];
          };
        };
      };
    };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}

    machine.succeed("test -f /run/secrets/rendered/nirion/web/compose.yaml")
    machine.succeed("grep -F '/run/secrets/rendered/nirion/web/compose.yaml' /etc/nirion/projects.json")
    machine.succeed("test -f /run/secrets/rendered/app.env")
    machine.succeed("grep -Fx SECRET_TOKEN=vm-compose-template-token /run/secrets/rendered/app.env")

    nirion("cat web | grep '/run/secrets/rendered/app.env'")
    nirion("cat web | grep 'nirion-test-http:latest'")

    machine.succeed("systemctl restart nirion-web.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18083 | grep compose-ok")
  '';
}
