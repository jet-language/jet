{
  devShells.x86_64-linux.default = pkgs.mkShell {
    packages = [
      (builtins.getFlake "path:./dep").packages.x86_64-linux.default
      pkgs.pkgsCross.aarch64-multiplatform.foo
    ];
  };
  packages.x86_64-linux.fetched = builtins.derivationStrict {
    name = "fetched";
    system = "x86_64-linux";
    builder = "/bin/sh";
    args = [ "-c" "echo $src > $out" ];
    src = builtins.fetchurl {
      url = "file:./source.txt";
      sha256 = "03e21237b264daa4b7dc6bc5ada1c24ba9d646a29720c31d1c0ab09cc5b03867";
      name = "source.txt";
    };
  };
}
