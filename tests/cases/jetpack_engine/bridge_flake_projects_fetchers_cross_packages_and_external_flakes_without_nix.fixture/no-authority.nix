{ devShells.x86_64-linux.default = pkgs.mkShell {
  packages = [ (builtins.getFlake "github:NixOS/nixpkgs").packages.x86_64-linux.default ];
}; }