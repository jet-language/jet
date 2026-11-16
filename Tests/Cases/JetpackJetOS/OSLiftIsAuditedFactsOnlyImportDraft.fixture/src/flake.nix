{
  inputs.flake-parts.url = "github:hercules-ci/flake-parts";
  inputs.home-manager.url = "github:nix-community/home-manager";
  outputs = { self, nixpkgs, ... }: {
    nixosConfigurations = { laptop = nixpkgs.lib.nixosSystem {}; };
  };
}