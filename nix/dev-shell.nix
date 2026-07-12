{
  pkgs,
  pre-commit ? null,
}:

pkgs.mkShell {
  shellHook = pkgs.lib.optionalString (pre-commit != null) pre-commit.shellHook;

  buildInputs = with pkgs; [
    cargo
    rustc
    rustfmt
    openssl
  ];

  nativeBuildInputs = with pkgs; [
    pkg-config
  ];

  packages =
    with pkgs;
    [
      rust-analyzer
    ]
    ++ pkgs.lib.optionals (pre-commit != null) pre-commit.enabledPackages;
}
