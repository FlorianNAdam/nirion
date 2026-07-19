{
  pkgs,
  lib,
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  port = 18080;
  system = evalConfig [
    (
      { ... }:
      {
        virtualisation.nirion = baseNirionConfig // {
          projects.health-runtime.services.probe.extraOptions.image = "example.invalid/probe:latest";
        };
      }
    )
  ];

  mkHttpHealthcheck = system.config.lib.nirion.mkHttpHealthcheck;

  composeInterpolated = builtins.replaceStrings [ "$$" ] [ "$" ];
  runnableCommand = command: lib.escapeShellArgs (map composeInterpolated (builtins.tail command));
  checks = {
    status = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/status";
      expect.status = 204;
    };

    body = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/body";
      expect.bodyEquals = "ready";
    };

    bodyContains = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/body-contains";
      expectedStatus = null;
      expect.bodyContains = "sentinel is $READY";
    };

    jsonEquals = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/json-equals";
      expect.jsonEquals = {
        ok = true;
        nested.count = 2;
      };
    };

    jsonContains = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/json-contains";
      expect.jsonContains = {
        items = [ "ok" ];
      };
    };

    statusFailure = mkHttpHealthcheck {
      host = "127.0.0.1";
      inherit port;
      path = "/status-failure";
      expect.status = 204;
    };
  };
in
pkgs.runCommand "nirion-runtime-healthchecks" { nativeBuildInputs = [ pkgs.perl ]; } ''
  perl ${./healthchecks-server.pl} ${toString port} "$TMPDIR/ready" &
  server_pid=$!
  trap 'kill "$server_pid"' EXIT

  while [ ! -e "$TMPDIR/ready" ]; do
    sleep 0.05
  done

  ${runnableCommand checks.status}
  ${runnableCommand checks.body}
  ${runnableCommand checks.bodyContains}
  ${runnableCommand checks.jsonEquals}
  ${runnableCommand checks.jsonContains}

  if ${runnableCommand checks.statusFailure}; then
    echo "expected failing healthcheck to exit non-zero"
    exit 1
  fi

  touch $out
''
