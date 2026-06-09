{ lib, pkgs, ... }:

{
  lib.nirion = {
    mkPerlHealthcheck =
      {
        binary ? "perl",
        host ? "localhost",
        port,
        path,
        result,
      }:
      let
        dockerEscape = string: builtins.replaceStrings [ "$" ] [ "$$" ] string;
        resultString = if builtins.isString result then result else builtins.toJSON result;
      in
      [
        "CMD"
        "${binary}"
        "-MIO::Socket::INET"
        "-e"
        (dockerEscape ''
          $s = IO::Socket::INET->new("${host}:${builtins.toString port}") or do {
            print "Failed to connect";
            exit 1;
          };
          print "Connected";
          print $s "GET ${path} HTTP/1.0\r\n";
          print $s "Host: ${host}:${builtins.toString port}\r\n";
          print $s "Connection: close\r\n";
          print $s "\r\n";
          $/ = undef;
          $resp = <$s>;
          $body = (split /\r?\n\r?\n/, $resp, 2)[1];

          my $expected = <<'JSON';
          ${resultString}
          JSON

          $body =~ s/^\s+//;
          $body =~ s/\s+$//;
          $expected =~ s/^\s+//;
          $expected =~ s/\s+$//;

          print "Healthcheck response: >>>$body<<<\n";
          print "Healthcheck expected: >>>$expected<<<\n";

          exit($body eq $expected ? 0 : 1);
        '')
      ];
  };
}
