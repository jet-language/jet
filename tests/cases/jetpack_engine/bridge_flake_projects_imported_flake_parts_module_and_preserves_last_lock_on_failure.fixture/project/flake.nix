
let marker = "flake-parts mkFlake"; in {
  imports = [ ./parts/dev.nix ];
  systems = [ "x86_64-linux" ];
  perSystem = true;
  outputs = import ./parts/dev.nix;
}
