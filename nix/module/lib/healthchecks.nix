{ lib, ... }:

{
  lib.nirion = {
    mkHttpHealthcheck =
      {
        binary ? "perl",
        host ? "localhost",
        port,
        path,
        timeout ? 3,
        expectedStatus ? 200,
        expect,
      }:
      let
        dockerEscape = string: builtins.replaceStrings [ "$" ] [ "$$" ] string;
        expectKeys = builtins.attrNames expect;
        allowedExpectKeys = [
          "status"
          "bodyEquals"
          "bodyContains"
          "jsonEquals"
          "jsonContains"
        ];
        selectedExpectKeys = builtins.filter (key: builtins.hasAttr key expect) allowedExpectKeys;
        unknownExpectKeys = builtins.filter (key: !(builtins.elem key allowedExpectKeys)) expectKeys;
        expectKey =
          if unknownExpectKeys != [ ] then
            throw "mkHttpHealthcheck: unknown expect variant(s): ${builtins.concatStringsSep ", " unknownExpectKeys}"
          else if builtins.length selectedExpectKeys != 1 then
            throw "mkHttpHealthcheck: exactly one expect variant must be set"
          else
            builtins.head selectedExpectKeys;
        expectValue = expect.${expectKey};

        jsonHelpers = builtins.readFile ./micro-json.pl;

        expectationCheck =
          if expectKey == "status" then
            ''
              fail("Expected HTTP status ${builtins.toString expectValue}, got " . (defined $status ? $status : "none"))
                unless defined $status && $status == ${builtins.toString expectValue};
            ''
          else if expectKey == "bodyEquals" then
            ''
              my $expected = <<'EXPECTED';
              ${expectValue}
              EXPECTED
              chomp $expected;
              fail("Response body did not equal expected body") unless $body eq $expected;
            ''
          else if expectKey == "bodyContains" then
            ''
              my $expected = <<'EXPECTED';
              ${expectValue}
              EXPECTED
              chomp $expected;
              fail("Response body did not contain expected text") unless index($body, $expected) >= 0;
            ''
          else if expectKey == "jsonEquals" then
            ''
              ${jsonHelpers}
              my $json = eval { decode_json($body) };
              fail("Response body was not valid JSON: $@") if $@;
              my $expected = decode_json(<<'JSON');
              ${builtins.toJSON expectValue}
              JSON
              fail("Response JSON did not equal expected JSON\nExpected JSON: " . json_canonical($expected) . "\nActual JSON: " . json_canonical($json))
                unless json_canonical($json) eq json_canonical($expected);
            ''
          else if expectKey == "jsonContains" then
            ''
              ${jsonHelpers}
              my $json = eval { decode_json($body) };
              fail("Response body was not valid JSON: $@") if $@;
              my $expected = decode_json(<<'JSON');
              ${builtins.toJSON expectValue}
              JSON
              fail("Response JSON did not contain expected JSON\nExpected JSON subset: " . json_canonical($expected) . "\nActual JSON: " . json_canonical($json))
                unless json_contains($json, $expected);
            ''
          else
            throw "unexpected expectKey";

        script = ''
          my $s = IO::Socket::INET->new(
            PeerHost => "${host}",
            PeerPort => ${builtins.toString port},
            Timeout => ${builtins.toString timeout},
          ) or do {
            print "Failed to connect";
            exit 1;
          };

          print $s "GET ${path} HTTP/1.0\r\n";
          print $s "Host: ${host}:${builtins.toString port}\r\n";
          print $s "Connection: close\r\n";
          print $s "\r\n";

          local $/ = undef;
          my $resp = <$s>;
          my ($headers, $body) = split /\r?\n\r?\n/, $resp, 2;
          $body = "" unless defined $body;

          my ($status) = $headers =~ m{^HTTP/\S+\s+(\d+)};
          sub fail {
            print shift, "\n";
            exit 1;
          }

          ${lib.optionalString (expectKey != "status" && expectedStatus != null) ''
            fail("Expected HTTP status ${builtins.toString expectedStatus}, got " . (defined $status ? $status : "none"))
              unless defined $status && $status == ${builtins.toString expectedStatus};
          ''}

          ${expectationCheck}

          exit 0;
        '';

        perlModules = [
          "-MIO::Socket::INET"
        ];
      in
      [
        "CMD"
        binary
      ]
      ++ perlModules
      ++ [
        "-e"
        (dockerEscape script)
      ];
  };
}
