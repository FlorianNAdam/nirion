{
  lib,
  evalConfig,
  baseNirionConfig,
  ...
}:

let
  system = evalConfig [
    (
      { config, ... }:
      {
        virtualisation.nirion = baseNirionConfig // {
          projects.health.services = {
            http = {
              extraOptions.image = "example.invalid/http:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                port = 8080;
                path = "/ready";
                expect.status = 204;
              };
            };

            body-equals = {
              extraOptions.image = "example.invalid/body-equals:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                port = 8081;
                path = "/body";
                expectedStatus = 201;
                expect.bodyEquals = "ready";
              };
            };

            body-contains = {
              extraOptions.image = "example.invalid/body-contains:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                port = 8082;
                path = "/contains";
                expectedStatus = null;
                expect.bodyContains = "sentinel is $READY";
              };
            };

            json-equals = {
              extraOptions.image = "example.invalid/json-equals:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                port = 8083;
                path = "/json";
                expect.jsonEquals = {
                  ok = true;
                  nested.count = 2;
                };
              };
            };

            json-contains = {
              extraOptions.image = "example.invalid/json-contains:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                binary = "/usr/bin/perl";
                host = "127.0.0.1";
                port = 8084;
                path = "/json-subset";
                timeout = 7;
                expect.jsonContains.items = [ "ok" ];
              };
            };

            curl = {
              extraOptions.image = "example.invalid/curl:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                backend = "curl";
                binary = "/usr/bin/curl";
                port = 8085;
                path = "/curl";
                expect.bodyContains = "ok";
              };
            };

            wget = {
              extraOptions.image = "example.invalid/wget:latest";
              healthcheck.test = config.lib.nirion.mkHttpHealthcheck {
                backend = "wget";
                binary = "/bin/wget";
                port = 8086;
                path = "/wget";
                expect.status = 204;
              };
            };

            disabled = {
              extraOptions.image = "example.invalid/disabled:latest";
              healthcheck.disable = true;
            };
          };
        };
      }
    )
  ];

  services = system.config.virtualisation.nirion.out.compose.health.attrs.services;
  test = services.http.healthcheck.test;
  script = serviceName: builtins.elemAt services.${serviceName}.healthcheck.test 4;

  invalidEval = expr: builtins.tryEval (builtins.deepSeq expr expr);
  emptyExpect = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      port = 8080;
      path = "/";
      expect = { };
    }
  );
  multipleExpect = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      port = 8080;
      path = "/";
      expect = {
        status = 200;
        bodyContains = "ok";
      };
    }
  );
  unknownExpect = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      port = 8080;
      path = "/";
      expect.foobar = true;
    }
  );
  curlJsonExpect = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      backend = "curl";
      port = 8080;
      path = "/";
      expect.jsonEquals.ok = true;
    }
  );
  wgetJsonExpect = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      backend = "wget";
      port = 8080;
      path = "/";
      expect.jsonContains.ok = true;
    }
  );
  unknownBackend = invalidEval (
    system.config.lib.nirion.mkHttpHealthcheck {
      backend = "netcat";
      port = 8080;
      path = "/";
      expect.status = 200;
    }
  );
in
[
  {
    assertion =
      builtins.elemAt test 0 == "CMD"
      && builtins.elemAt test 1 == "perl"
      && lib.hasInfix "GET /ready HTTP/1.0" (builtins.elemAt test 4)
      && lib.hasInfix "Expected HTTP status 204" (builtins.elemAt test 4);
    message = "mkHttpHealthcheck did not render the expected compose healthcheck command";
  }
  {
    assertion =
      lib.hasInfix "Expected HTTP status 201" (script "body-equals")
      && lib.hasInfix "Response body did not equal expected body" (script "body-equals")
      && lib.hasInfix "ready" (script "body-equals");
    message = "mkHttpHealthcheck bodyEquals did not render status and body checks";
  }
  {
    assertion =
      !(lib.hasInfix "Expected HTTP status" (script "body-contains"))
      && lib.hasInfix "Response body did not contain expected text" (script "body-contains")
      && lib.hasInfix "sentinel is $$READY" (script "body-contains");
    message = "mkHttpHealthcheck bodyContains did not honor expectedStatus = null or docker escaping";
  }
  {
    assertion =
      lib.hasInfix "Response JSON did not equal expected JSON" (script "json-equals")
      && lib.hasInfix "\"ok\":true" (script "json-equals")
      && lib.hasInfix "\"count\":2" (script "json-equals");
    message = "mkHttpHealthcheck jsonEquals did not render JSON equality checks";
  }
  {
    assertion =
      services."json-contains".healthcheck.test != [ ]
      && builtins.elemAt services."json-contains".healthcheck.test 1 == "/usr/bin/perl"
      && lib.hasInfix ''PeerHost => "127.0.0.1"'' (script "json-contains")
      && lib.hasInfix "PeerPort => 8084" (script "json-contains")
      && lib.hasInfix "Timeout => 7" (script "json-contains")
      && lib.hasInfix "Response JSON did not contain expected JSON" (script "json-contains")
      && lib.hasInfix "\"items\":[\"ok\"]" (script "json-contains");
    message = "mkHttpHealthcheck jsonContains or custom connection options were not rendered";
  }
  {
    assertion =
      services.curl.healthcheck.test != [ ]
      && builtins.elemAt services.curl.healthcheck.test 1 == "sh"
      && builtins.elemAt services.curl.healthcheck.test 2 == "-ec"
      && lib.hasInfix "/usr/bin/curl" (builtins.elemAt services.curl.healthcheck.test 3)
      && lib.hasInfix "http://localhost:8085/curl" (builtins.elemAt services.curl.healthcheck.test 3)
      && lib.hasInfix "Response body did not contain expected text" (
        builtins.elemAt services.curl.healthcheck.test 3
      );
    message = "mkHttpHealthcheck curl backend did not render the expected shell command";
  }
  {
    assertion =
      services.wget.healthcheck.test != [ ]
      && builtins.elemAt services.wget.healthcheck.test 1 == "sh"
      && builtins.elemAt services.wget.healthcheck.test 2 == "-ec"
      && lib.hasInfix "/bin/wget" (builtins.elemAt services.wget.healthcheck.test 3)
      && lib.hasInfix "--server-response" (builtins.elemAt services.wget.healthcheck.test 3)
      && lib.hasInfix "Expected HTTP status 204" (builtins.elemAt services.wget.healthcheck.test 3);
    message = "mkHttpHealthcheck wget backend did not render the expected shell command";
  }
  {
    assertion = services.disabled.healthcheck.disable == true;
    message = "healthcheck.disable was not rendered";
  }
  {
    assertion = emptyExpect.success == false;
    message = "mkHttpHealthcheck should reject missing expect variants";
  }
  {
    assertion = multipleExpect.success == false;
    message = "mkHttpHealthcheck should reject multiple expect variants";
  }
  {
    assertion = unknownExpect.success == false;
    message = "mkHttpHealthcheck should reject unknown expect variants";
  }
  {
    assertion = curlJsonExpect.success == false && wgetJsonExpect.success == false;
    message = "mkHttpHealthcheck should reject JSON expectations for curl and wget backends";
  }
  {
    assertion = unknownBackend.success == false;
    message = "mkHttpHealthcheck should reject unknown backends";
  }
]
