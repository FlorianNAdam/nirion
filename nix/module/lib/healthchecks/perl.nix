{
  lib,
  dockerEscape,
  jsonHelpers,
}:

{
  binary,
  host,
  port,
  path,
  timeout,
  expectedStatus,
  expectKey,
  expectValue,
}:

let
  expectationCheck =
    if expectKey == "status" then
      ''
        fail("Expected HTTP status ${toString expectValue}, got " . (defined $status ? $status : "none"))
          unless defined $status && $status == ${toString expectValue};
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
      PeerPort => ${toString port},
      Timeout => ${toString timeout},
    ) or do {
      print "Failed to connect";
      exit 1;
    };

    print $s "GET ${path} HTTP/1.0\r\n";
    print $s "Host: ${host}:${toString port}\r\n";
    print $s "Connection: close\r\n";
    print $s "\r\n";

    my $resp = do { local $/ = undef; <$s> };
    my ($headers, $body) = split /\r?\n\r?\n/, $resp, 2;
    $body = "" unless defined $body;

    my ($status) = $headers =~ m{^HTTP/\S+\s+(\d+)};
    sub fail {
      print shift, "\n";
      exit 1;
    }

    ${lib.optionalString (expectKey != "status" && expectedStatus != null) ''
      fail("Expected HTTP status ${toString expectedStatus}, got " . (defined $status ? $status : "none"))
        unless defined $status && $status == ${toString expectedStatus};
    ''}

    ${expectationCheck}

    exit 0;
  '';
in
[
  "CMD"
  binary
  "-MIO::Socket::INET"
  "-e"
  (dockerEscape script)
]
