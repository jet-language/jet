{ devShells.x86_64-linux.default = pkgs.mkShell {
  packages = builtins.attrValues (builtins.fromJSON "{\"b\":\"ripgrep\",\"a\":\"fd\"}");
}; }