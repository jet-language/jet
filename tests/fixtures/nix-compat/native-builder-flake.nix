{
  packages.x86_64-linux.native = builtins.derivationStrict {
    name = "native-repository";
    system = "x86_64-linux";
    builder = "/bin/sh";
    args = [ "-c" "printf native-repository > $out" ];
  };
}
