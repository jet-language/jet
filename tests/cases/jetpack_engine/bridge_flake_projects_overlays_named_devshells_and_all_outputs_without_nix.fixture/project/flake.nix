{
  packages.x86_64-linux.default = builtins.derivation {
    name = "many";
    system = "x86_64-linux";
    builder = "/bin/sh";
    outputs = [ "out" "dev" "doc" ];
  };
  devShells.x86_64-linux.default =
    (import pkgs { overlays = [ (final: prev: { custom = prev.fd; }) ]; }).mkShell {
      packages = [ (import pkgs { overlays = [ (final: prev: { custom = prev.fd; }) ]; }).custom ];
    };
  devShells.x86_64-linux.debug = {
    packages = [ pkgs.ripgrep ];
    inputsFrom = [];
    shellHook = "export DEBUG=1";
  };
}
