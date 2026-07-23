{ lib, dockerEscape }:

{
  backend,
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
  shellEscape = lib.escapeShellArg;
  url = "http://${host}:${toString port}${path}";

  shellExpected = value: ''
    expected=$(cat <<'EXPECTED'
    ${value}
    EXPECTED
    )
  '';

  curlFetch = ''
    status="$(${shellEscape binary} -sS -m ${toString timeout} -o "$body_file" -w '%{http_code}' ${shellEscape url})" || fail "Failed to connect"
  '';

  wgetFetch = ''
    wget_exit=0
    ${shellEscape binary} -T ${toString timeout} -O "$body_file" --server-response --content-on-error ${shellEscape url} 2>"$headers_file" || wget_exit=$?
    status=""
    while IFS= read -r line; do
      case "$line" in
        *'HTTP/'*) status=''${line#*HTTP/} ; status=''${status#* } ; status=''${status%% *} ;;
      esac
    done < "$headers_file"
    [ -n "$status" ] || [ "$wget_exit" = 0 ] || fail "Failed to connect"
  '';

  fetch =
    if backend == "curl" then
      curlFetch
    else if backend == "wget" then
      wgetFetch
    else
      throw "unexpected backend";

  statusCheck = expected: ''
    ${fetch}
    [ "$status" = ${shellEscape (toString expected)} ] || fail "Expected HTTP status ${toString expected}, got $status"
  '';

  expectationCheck =
    if expectKey == "status" then
      statusCheck expectValue
    else if expectKey == "bodyEquals" then
      ''
        ${fetch}
        ${lib.optionalString (expectedStatus != null) ''
          [ "$status" = ${shellEscape (toString expectedStatus)} ] || fail "Expected HTTP status ${toString expectedStatus}, got $status"
        ''}
        ${shellExpected expectValue}
        body="$(cat "$body_file")"
        [ "$body" = "$expected" ] || fail "Response body did not equal expected body"
      ''
    else if expectKey == "bodyContains" then
      ''
        ${fetch}
        ${lib.optionalString (expectedStatus != null) ''
          [ "$status" = ${shellEscape (toString expectedStatus)} ] || fail "Expected HTTP status ${toString expectedStatus}, got $status"
        ''}
        ${shellExpected expectValue}
        body="$(cat "$body_file")"
        case "$body" in
          *"$expected"*) ;;
          *) fail "Response body did not contain expected text" ;;
        esac
      ''
    else
      throw "unexpected expectKey";

  script = ''
    fail() {
      printf '%s\n' "$1"
      exit 1
    }

    body_file="/tmp/nirion-healthcheck-body.$$"
    headers_file="/tmp/nirion-healthcheck-headers.$$"
    trap 'rm -f "$body_file" "$headers_file"' EXIT

    ${expectationCheck}

    exit 0
  '';
in
[
  "CMD"
  "sh"
  "-ec"
  (dockerEscape script)
]
