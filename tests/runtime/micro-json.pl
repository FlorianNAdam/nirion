use strict;
use warnings;

my ($micro_json) = @ARGV;
do $micro_json;
die $@ if $@;

sub ok {
  my ($condition, $message) = @_;
  die "$message\n" unless $condition;
}

sub dies {
  my ($code, $message) = @_;
  my $ok = !eval { $code->(); 1 };
  die "$message\n" unless $ok;
}

my $object = decode_json('{"b":2,"a":{"nested":true},"items":["x",null]}');
ok(ref($object) eq "HASH", "decoded object should be a hash");
ok($object->{b} == 2, "decoded number did not match");
ok($object->{a}->{nested}, "decoded boolean did not match");
ok(!defined $object->{items}->[1], "decoded null did not match");

my $empty = decode_json('{"object":{},"array":[]}');
ok(ref($empty->{object}) eq "HASH", "decoded empty object should be a hash");
ok(ref($empty->{array}) eq "ARRAY", "decoded empty array should be an array");
ok(keys(%{ $empty->{object} }) == 0, "decoded empty object should have no keys");
ok(@{ $empty->{array} } == 0, "decoded empty array should have no values");

my $spaced = decode_json(" \n\t { \"a\" : [ true , false , null ] } \r\n");
ok($spaced->{a}->[0], "parser should accept whitespace before values");
ok(!$spaced->{a}->[1], "parser should decode false values");
ok(!defined $spaced->{a}->[2], "parser should decode null values with whitespace");

my $numbers = decode_json('{"negative":-2,"decimal":3.5,"exponent":1e3,"small":-2.5e-2}');
ok($numbers->{negative} == -2, "decoded negative number did not match");
ok($numbers->{decimal} == 3.5, "decoded decimal number did not match");
ok($numbers->{exponent} == 1000, "decoded exponent number did not match");
ok($numbers->{small} == -0.025, "decoded negative exponent number did not match");

my $escaped = decode_json(q!{"quote":"\"","slash":"\/","backslash":"\\\\","line":"\n","tab":"\t","return":"\r","backspace":"\b","formfeed":"\f"}!);
ok($escaped->{quote} eq '"', "decoded quote escape did not match");
ok($escaped->{slash} eq '/', "decoded slash escape did not match");
ok($escaped->{backslash} eq "\\", "decoded backslash escape did not match");
ok($escaped->{line} eq "\n", "decoded newline escape did not match");
ok($escaped->{tab} eq "\t", "decoded tab escape did not match");
ok($escaped->{return} eq "\r", "decoded carriage return escape did not match");
ok($escaped->{backspace} eq "\b", "decoded backspace escape did not match");
ok($escaped->{formfeed} eq "\f", "decoded formfeed escape did not match");

my $unicode = decode_json('{"text":"smile: \\u263a"}');
ok($unicode->{text} eq "smile: \x{263a}", "decoded unicode escape did not match");

ok(
  json_canonical(decode_json('{"z":1,"a":2}')) eq '{"a":2,"z":1}',
  "canonical JSON should sort object keys"
);

ok(
  json_canonical(decode_json('[{"b":2,"a":1},null,"x"]')) eq '[{"a":1,"b":2},null,"x"]',
  "canonical JSON should handle nested arrays and objects"
);

ok(
  json_contains(
    decode_json('{"a":1,"b":{"c":2,"d":3}}'),
    decode_json('{"b":{"c":2}}')
  ),
  "json_contains should allow nested object subsets"
);

ok(
  !json_contains(
    decode_json('{"items":["a","b"]}'),
    decode_json('{"items":["a"]}')
  ),
  "json_contains should require arrays to match exactly"
);

ok(
  !json_contains(
    decode_json('{"a":1}'),
    decode_json('{"missing":null}')
  ),
  "json_contains should distinguish missing keys from null values"
);

ok(
  !json_contains(
    decode_json('{"a":{"b":1}}'),
    decode_json('{"a":{"b":2}}')
  ),
  "json_contains should reject mismatched nested values"
);

ok(
  !json_contains(
    decode_json('{"a":1}'),
    decode_json('{"a":{"nested":1}}')
  ),
  "json_contains should reject mismatched container types"
);

ok(
  json_contains(decode_json('{"enabled":true}'), decode_json('{"enabled":1}')),
  "json_contains intentionally treats booleans and numeric scalars loosely"
);

dies(sub { decode_json('{"unterminated":') }, "malformed JSON should fail");
dies(sub { decode_json('{"a":1} trailing') }, "trailing JSON input should fail");
dies(sub { decode_json('{"bad":"\\x"}') }, "unsupported string escape should fail");
dies(sub { decode_json('{"bad":"\\u12xz"}') }, "invalid unicode escape should fail");
dies(sub { decode_json('{"a":01}') }, "leading-zero number should fail");
dies(sub { decode_json('[1,]') }, "trailing array comma should fail");
dies(sub { decode_json('{"a":1,}') }, "trailing object comma should fail");
