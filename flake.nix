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
          pkgs.lld
          # D-FFI-RUBY1=A: provision the supervised Ruby worker and stdlib JSON/Ripper.
          pkgs.ruby
          # D-FFI-PHP1=A: provision the supervised PHP worker pool.
          pkgs.php
        ];
        jetTzdb = "${pkgs.tzdata}/share/zoneinfo";

        jet = pkgs.rustPlatform.buildRustPackage {
          pname = "jet";
          version = "1.0.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.makeWrapper ];
          buildInputs = [ pkgs.rustc ];

          doCheck = true;

          postInstall = ''
            wrapProgram $out/bin/jet \
              --prefix PATH : "${jetRuntimePath}" \
              --set-default TZDIR "${jetTzdb}"
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
        mkJetDevBin =
          name:
          pkgs.writeShellScriptBin name ''
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
            bin="$root/target/debug/${name}"
            export PATH="${jetRuntimePath}:$PATH"
            if [ ! -x "$bin" ]; then
              echo "jet: no debug binary at $bin" >&2
              echo "fix: cargo build" >&2
              exit 1
            fi
            exec "$bin" "$@"
          '';
        jetDev = mkJetDevBin "jet";
        jetpackDev = mkJetDevBin "jetpack";
      in
      {
        packages = {
          default = jet;
          inherit jet;
          jetpack = jet;
        };

        apps.default = {
          type = "app";
          program = "${jet}/bin/jet";
        };
        apps.jetpack = {
          type = "app";
          program = "${jet}/bin/jetpack";
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.gcc
            # D-FFI-ADA1=A: provision GNAT for C-ABI Ada binder compilation.
            pkgs.gnat
            # D-FFI-PASCAL1=A: provision FreePascal for cdecl estate bindings.
            pkgs.fpc
            # D-FFI-DART1=A: provision Dart native FFI and AOT tooling.
            pkgs.dart
            # D-FFI-PWSH1=A: provision the persistent PowerShell 7 worker.
            pkgs.powershell
            # D-FFI-FORTRAN1=A: provision the ISO_C_BINDING bridge compiler.
            pkgs.gfortran
            # D-FFI-COBOL1=A: provision GnuCOBOL for C-ABI estate bindings.
            (pkgs.lib.getBin pkgs.gnucobol)
            # D-FFI-GO1=A: provision the in-process c-archive bridge compiler.
            pkgs.go
            # D-FFI-JVM1=A: provision javac/javap plus the embedded libjvm runtime.
            pkgs.jdk
            # D-FFI-DOTNET1=A: provision SDK plus hostfxr/hostpolicy embedding runtime.
            pkgs.dotnet-sdk_8
            # D-FFI-TCL1=A: provision embeddable Tcl headers, runtime, and shell.
            pkgs.tcl
            pkgs.lld
            # D-FFI-RUBY1=A: provision the supervised Ruby worker and stdlib JSON/Ripper.
            pkgs.ruby
            # D-FFI-PHP1=A: provision the supervised PHP worker pool.
            pkgs.php
            # Compiler freestanding smoke tests execute aarch64 output under
            # qemu-aarch64. OS image and VM tooling does not belong here.
            pkgs.qemu
            pkgs.nodejs_22
            pkgs.nixfmt
            pkgs.ripgrep
            pkgs.jq
            pkgs.gh
            pkgs.fd
            # Prompt acceptance executes generated rc in every supported shell.
            pkgs.bashInteractive
            pkgs.zsh
            pkgs.fish
            pkgs.util-linux
            # D-DEP-WASM1=A (c81): `jet build --target=plugin` lifts the
            # rustc-built wasm32-unknown-unknown core module into a WASM
            # Component using `wasm-tools component embed`/`new` — an external
            # CLI tool (I6: shelled out to, like cargo/rustc, never linked
            # into the compiler).
            pkgs.wasm-tools
            pkgs.tree-sitter
            pkgs.emscripten
            pkgs.lldb
            jetDev
            jetpackDev
            pkgs.pkg-config
            pkgs.raylib
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # D-CANVASTEST1=A: Canvas interaction tests drive Chromium through
            # the repo-owned stdlib-only CDP pipe driver. nixpkgs Chromium is
            # Linux-only, so it must not make macOS dev shells unevaluable.
            pkgs.chromium
            # The native GTK backend is Linux-first (D-UIDEVSHELL1=A); keep its
            # headers off platforms where that backend is not supported.
            pkgs.gtk4
            # D-BUILDENTRY1 / #95: programmable build actions execute only
            # inside bubblewrap (E3505 fail-closed; no ambient fallback).
            pkgs.bubblewrap
          ];

          shellHook = ''
            if repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"; then
              export JET_ROOT="$repo_root"
            else
              export JET_ROOT="$PWD"
            fi
            export TZDIR="${jetTzdb}"
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [ pkgs.raylib ]}:''${LD_LIBRARY_PATH:-}"

            if [ "''${JET_NIX_TMP_CLEANED:-}" != "1" ]; then
              "${self}/scripts/agent/clean-nix-tmp.sh"
            fi
            export JET_NIX_TMP_CLEANED=1

            # D-CI3: wire the fast pre-push doc-sync hook, idempotent, only
            # when inside a git repo (worktrees/CI checkouts included).
            if git rev-parse --git-dir >/dev/null 2>&1; then
              current_hooks_path="$(git config --get core.hooksPath || true)"
              if [ "$current_hooks_path" != "scripts/githooks" ]; then
                git config core.hooksPath scripts/githooks
              fi
            fi

            # banner on stderr: `nix develop -c <cmd>` stdout stays clean for
            # grepping/capture (agents misread results otherwise)
            {
              echo "Jet dev shell"
              echo "  build:    cargo build"
              echo "  run:      jet run examples/features/basics/hello.jet"
              echo "  package:  jetpack help"
              echo "  search:   rg \"pattern\" docs Source tests"
              echo "  LSP:      jet lsp        (tests: cargo test --test lsp)"
              echo "  editor:   editors/vscode/install.sh   (Cursor/VS Code)"
              echo "            editors/zed/install.sh        (Zed dev extension)"
              echo "  debug:    jet debug <file.jet>  (native lldb backend: tests/debug.rs)"
              echo "  release:  nix build .#jet"
            } >&2
          '';
        };

        formatter = pkgs.nixfmt;
      }
    );
}
