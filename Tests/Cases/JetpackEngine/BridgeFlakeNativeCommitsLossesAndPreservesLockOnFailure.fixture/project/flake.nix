{
  devShells.x86_64-linux.default = {
    packages = [ pkgs.fd ];
    shellHook = "export FOO=1";
  };
}
