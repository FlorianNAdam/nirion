use strict;
use warnings;
use IO::Socket::INET;

my ($port, $ready) = @ARGV;
my $server = IO::Socket::INET->new(
  LocalAddr => "127.0.0.1",
  LocalPort => $port,
  Listen => 5,
  Reuse => 1,
) or die "failed to listen: $!";

open my $ready_fh, ">", $ready or die "failed to write readiness file: $!";
close $ready_fh;

while (my $client = $server->accept()) {
  local $/ = "\r\n\r\n";
  my $request = <$client> // "";
  my ($path) = $request =~ m{^GET\s+(\S+)};

  if (($path // "") eq "/status") {
    print $client "HTTP/1.0 204 No Content\r\nConnection: close\r\n\r\n";
  } elsif (($path // "") eq "/body") {
    print $client "HTTP/1.0 200 OK\r\nConnection: close\r\n\r\nready";
  } elsif (($path // "") eq "/body-contains") {
    print $client "HTTP/1.0 503 Service Unavailable\r\nConnection: close\r\n\r\nnot ready, sentinel is \$READY";
  } elsif (($path // "") eq "/json-equals") {
    print $client "HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n{\"nested\":{\"count\":2},\"ok\":true}";
  } elsif (($path // "") eq "/json-contains") {
    print $client "HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n{\"extra\":true,\"items\":[\"ok\"]}";
  } elsif (($path // "") eq "/status-failure") {
    print $client "HTTP/1.0 500 Internal Server Error\r\nConnection: close\r\n\r\nfailed";
  } else {
    print $client "HTTP/1.0 404 Not Found\r\nConnection: close\r\n\r\nnot found";
  }

  close $client;
}
