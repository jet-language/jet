{
  inputs.tools.url = "github:example/tools?rev=0123456789abcdef0123456789abcdef01234567";
  devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.fd ]; };
}
