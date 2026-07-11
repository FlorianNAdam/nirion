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
    pkgs.runCommand "nirion-vm-sops-fixture"
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
          token: vm-sops-token
          message: vm-sops-message
        EOF

        SOPS_AGE_RECIPIENTS="$recipient" \
          sops --encrypt --input-type yaml --output-type yaml plain.yaml > $out/secrets.yaml
      '';
in
{
  name = "nirion-vm-sops";

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
      environment.etc."nirion-vm-sops-age-key.txt".source = "${sopsFixture}/age-key.txt";

      sops = {
        age.keyFile = "/etc/nirion-vm-sops-age-key.txt";
        defaultSopsFile = "${sopsFixture}/secrets.yaml";
      };

      virtualisation.nirion = {
        lockFile = builtins.toFile "nirion-lock.json" "{}";
        lockFileOutput = "/var/lib/nirion/lock.json";

        projects.secret = {
          sops = {
            file = "${sopsFixture}/secrets.yaml";
            group.gid = 9006;
            secrets."app/message" = { };
            secrets."app/token" = { };
            templates."app.env".content = ''
              SECRET_TOKEN=${config.sops.placeholder."app/token"}
            '';
          };

          services.app = {
            extraOptions.image = imageRef;
            ports = [ "18082:8080" ];
            env_file = [ config.sops.templates."app.env".path ];
            volumes = [ "${config.sops.secrets."app/message".path}:/run/secret/message:ro" ];
            command = [
              "/bin/sh"
              "-c"
              ''
                test "$$SECRET_TOKEN" = "vm-sops-token"
                test "$$(cat /run/secret/message)" = "vm-sops-message"
                while true; do printf 'HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nsops-ok' | nc -l -p 8080; done
              ''
            ];
          };
        };
      };
    };

  testScript = ''
    ${nirionHelper}
    ${loadImageScript testImage}

    machine.succeed("test -f /etc/nirion-vm-sops-age-key.txt")
    machine.succeed("test -f /run/secrets/app/message")
    machine.succeed("grep -Fx vm-sops-message /run/secrets/app/message")
    machine.succeed("test -f /run/secrets/rendered/app.env")
    machine.succeed("grep -Fx SECRET_TOKEN=vm-sops-token /run/secrets/rendered/app.env")
    machine.succeed("systemctl restart nirion-secret.service")
    machine.wait_until_succeeds("curl --fail http://localhost:18082 | grep sops-ok")
    machine.succeed("docker inspect secret-app-1 --format '{{json .HostConfig.GroupAdd}}' | grep 9006")

    nirion("cat secret | grep '/run/secrets/rendered/app.env'")
    nirion("cat secret | grep '/run/secrets/app/message:/run/secret/message:ro'")
  '';
}
