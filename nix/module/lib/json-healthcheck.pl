sub json_error {
  die "invalid JSON at offset $pos\n";
}

sub json_ws {
  $pos++ while $pos < length($text) && substr($text, $pos, 1) =~ /\s/;
}

sub json_string {
  json_error() unless substr($text, $pos, 1) eq '"';
  $pos++;
  my $out = "";
  while ($pos < length($text)) {
    my $ch = substr($text, $pos++, 1);
    return $out if $ch eq '"';
    if ($ch eq '\\') {
      my $esc = substr($text, $pos++, 1);
      $out .= $esc eq '"' ? '"'
        : $esc eq '\\' ? '\\'
        : $esc eq '/' ? '/'
        : $esc eq 'b' ? "\b"
        : $esc eq 'f' ? "\f"
        : $esc eq 'n' ? "\n"
        : $esc eq 'r' ? "\r"
        : $esc eq 't' ? "\t"
        : json_error();
    } else {
      $out .= $ch;
    }
  }
  json_error();
}

sub json_value {
  json_ws();
  my $ch = substr($text, $pos, 1);

  if ($ch eq '"') {
    return json_string();
  }

  if ($ch eq '{') {
    $pos++;
    my $obj = {};
    json_ws();
    if (substr($text, $pos, 1) eq '}') {
      $pos++;
      return $obj;
    }
    while (1) {
      json_ws();
      my $key = json_string();
      json_ws();
      json_error() unless substr($text, $pos++, 1) eq ':';
      $obj->{$key} = json_value();
      json_ws();
      my $sep = substr($text, $pos++, 1);
      return $obj if $sep eq '}';
      json_error() unless $sep eq ',';
    }
  }

  if ($ch eq '[') {
    $pos++;
    my $arr = [];
    json_ws();
    if (substr($text, $pos, 1) eq ']') {
      $pos++;
      return $arr;
    }
    while (1) {
      push @$arr, json_value();
      json_ws();
      my $sep = substr($text, $pos++, 1);
      return $arr if $sep eq ']';
      json_error() unless $sep eq ',';
    }
  }

  if (substr($text, $pos) =~ /\A(-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?)/) {
    $pos += length($1);
    return 0 + $1;
  }

  if (substr($text, $pos, 4) eq 'true') {
    $pos += 4;
    return 1;
  }

  if (substr($text, $pos, 5) eq 'false') {
    $pos += 5;
    return 0;
  }

  if (substr($text, $pos, 4) eq 'null') {
    $pos += 4;
    return undef;
  }

  json_error();
}

sub decode_json {
  local $text = shift;
  local $pos = 0;
  my $value = json_value();
  json_ws();
  json_error() unless $pos == length($text);
  return $value;
}

sub json_canonical {
  my ($value) = @_;
  return 'null' unless defined $value;

  if (ref($value) eq 'HASH') {
    return '{' . join(',', map { json_canonical($_) . ':' . json_canonical($value->{$_}) } sort keys %$value) . '}';
  }

  if (ref($value) eq 'ARRAY') {
    return '[' . join(',', map { json_canonical($_) } @$value) . ']';
  }

  if ($value =~ /\A-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?\z/) {
    return 0 + $value;
  }

  $value =~ s/(["\\])/\\$1/g;
  $value =~ s/\n/\\n/g;
  $value =~ s/\r/\\r/g;
  $value =~ s/\t/\\t/g;
  return '"' . $value . '"';
}

sub json_contains {
  my ($actual, $expected) = @_;

  if (ref($expected) eq "HASH") {
    return 0 unless ref($actual) eq "HASH";
    for my $key (keys %$expected) {
      return 0 unless exists $actual->{$key};
      return 0 unless json_contains($actual->{$key}, $expected->{$key});
    }
    return 1;
  }

  if (ref($expected) eq "ARRAY") {
    return 0 unless ref($actual) eq "ARRAY";
    return 0 unless @$actual == @$expected;
    for (my $i = 0; $i < @$expected; $i++) {
      return 0 unless json_contains($actual->[$i], $expected->[$i]);
    }
    return 1;
  }

  return json_canonical($actual) eq json_canonical($expected);
}
