{ lib, ... }:

let
  dockerEscape = string: builtins.replaceStrings [ "$" ] [ "$$" ] string;

  mkPerlHealthcheck = import ./perl.nix {
    inherit lib dockerEscape;
    jsonHelpers = builtins.readFile ../micro-json.pl;
  };

  mkShellHealthcheck = import ./shell.nix {
    inherit lib dockerEscape;
  };
in
{
  lib.nirion = {
    mkHttpHealthcheck =
      {
        backend ? "perl",
        binary ? null,
        host ? "localhost",
        port,
        path,
        timeout ? 3,
        expectedStatus ? 200,
        expect,
      }:
      let
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
        resolvedBinary = if binary == null then backend else binary;
        common = {
          binary = resolvedBinary;
          inherit
            host
            port
            path
            timeout
            expectedStatus
            expectKey
            expectValue
            ;
        };
        unsupportedJson = expectKey == "jsonEquals" || expectKey == "jsonContains";
      in
      if backend == "perl" then
        mkPerlHealthcheck common
      else if backend == "curl" || backend == "wget" then
        if unsupportedJson then
          throw "mkHttpHealthcheck: expect.${expectKey} requires backend = \"perl\", got \"${backend}\""
        else
          mkShellHealthcheck (common // { inherit backend; })
      else
        throw "mkHttpHealthcheck: unknown backend \"${backend}\"";
  };
}
