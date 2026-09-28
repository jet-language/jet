{
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts?rev=0123456789abcdef0123456789abcdef01234567";
    nixpkgs.url = "github:NixOS/nixpkgs?rev=89abcdef0123456789abcdef0123456789abcdef";
  };
  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ ./parts/dev.nix ];
      systems = [ "x86_64-linux" ];
      perSystem = { pkgs, ... }: {
        packages.default = pkgs.hello;
        devShells.default = pkgs.mkShell { };
      };
    };
}
