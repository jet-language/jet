{
  description = "Jet — beginner-first, memory-safe compiled language";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        jet = pkgs.rustPlatform.buildRustPackage {
          pname = "jet";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # jet invokes rustc at runtime to compile generated code.
          nativeBuildInputs = [ pkgs.makeWrapper ];
          buildInputs = [ pkgs.rustc ];

          doCheck = true;

          postInstall = ''
            wrapProgram $out/bin/jet \
              --prefix PATH : "${pkgs.lib.makeBinPath [ pkgs.rustc pkgs.stdenv.cc ]}"
          '';

          meta = with pkgs.lib; {
            description = "Compiler for the Jet programming language";
            homepage = "https://github.com/HalcyonOmega/lex-lang";
            mainProgram = "jet";
            platforms = platforms.unix;
          };
        };
      in
      {
        packages.default = jet;
        packages.jet = jet;

        apps.default = {
          type = "app";
          program = "${jet}/bin/jet";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            gcc
            jet
          ];
          shellHook = ''
            echo "Jet dev shell — rustc on PATH for \`jet run\`"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
