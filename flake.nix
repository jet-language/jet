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

        # jet build/run shells out to rustc; keep this path in one place.
        jetRuntimePath = pkgs.lib.makeBinPath [
          pkgs.rustc
          pkgs.stdenv.cc
        ];

        jet = pkgs.rustPlatform.buildRustPackage {
          pname = "jet";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.makeWrapper ];
          buildInputs = [ pkgs.rustc ];

          doCheck = true;

          postInstall = ''
            wrapProgram $out/bin/jet \
              --prefix PATH : "${jetRuntimePath}"
          '';

          meta = with pkgs.lib; {
            description = "Compiler for the Jet programming language";
            homepage = "https://github.com/jet-language/jet";
            mainProgram = "jet";
            platforms = platforms.unix;
          };
        };

        # `jet` in the dev shell: run the cargo-built debug binary from anywhere
        # in the repo, with rustc + cc on PATH for `jet build`/`jet run`.
        jetDev = pkgs.writeShellScriptBin "jet" ''
          set -euo pipefail
          root="''${JET_ROOT:-}"
          if [ -z "$root" ]; then
            dir="$PWD"
            while [ "$dir" != "/" ]; do
              if [ -f "$dir/Cargo.toml" ] && [ -f "$dir/flake.nix" ]; then
                root="$dir"
                break
              fi
              dir=$(dirname "$dir")
            done
          fi
          root="''${root:-$PWD}"
          bin="$root/target/debug/jet"
          export PATH="${jetRuntimePath}:$PATH"
          if [ ! -x "$bin" ]; then
            echo "jet: no debug binary at $bin" >&2
            echo "fix: cargo build" >&2
            exit 1
          fi
          exec "$bin" "$@"
        '';
      in
      {
        packages = {
          default = jet;
          inherit jet;
        };

        apps.default = {
          type = "app";
          program = "${jet}/bin/jet";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            gcc
            nodejs_22
            jetDev
          ];

          shellHook = ''
            export JET_ROOT="$PWD"

            echo "Jet dev shell"
            echo "  build:    cargo build"
            echo "  run:      jet run examples/01_hello.jet"
            echo "  LSP:      jet lsp        (tests: cargo test --test lsp)"
            echo "  editor:   editors/vscode/install.sh   (then open the repo in Cursor)"
            echo "  release:  nix build .#jet"
          '';
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
