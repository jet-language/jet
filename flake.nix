{
  description = "Jet — beginner-first, memory-safe compiled language";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        jetR = pkgs.rWrapper.override { packages = [ pkgs.rPackages.jsonlite ]; };

        # User FFI helpers opt into this compiler/sysroot explicitly. Keep
        # normal Jet compilation and runtime Rust on nixpkgs' source-built
        # toolchain: its metadata is not interchangeable with upstream std.
        # Both inputs pin the same compiler version; no ambient rustup needed.
        jetHasMuslHelpers = system == "x86_64-linux";
        jetRustToolchain = pkgs.rust-bin.stable.${pkgs.rustc.version}.default.override {
          targets = [
            "wasm32-unknown-unknown"
            "wasm32v1-none"
          ] ++ pkgs.lib.optional jetHasMuslHelpers "x86_64-unknown-linux-musl";
        };
        jetMuslCC = pkgs.pkgsCross.musl64.stdenv.cc;
        jetMuslLinker = "${jetMuslCC}/bin/x86_64-unknown-linux-musl-cc";
        jetMuslAR = "${jetMuslCC.bintools}/bin/x86_64-unknown-linux-musl-ar";
        jetRustHelperHook = pkgs.lib.optionalString jetHasMuslHelpers ''
          export JET_RUST_MUSL_TOOLCHAIN="${jetRustToolchain}"
          export JET_RUST_MUSL_LINKER="${jetMuslLinker}"
          export JET_RUST_MUSL_AR="${jetMuslAR}"
        '';

        # Tools `jet build`/`jet run` shell out to; rustc only builds user `extern rust`.
        jetRuntimePath = pkgs.lib.makeBinPath [
          pkgs.rustc
          pkgs.stdenv.cc
          # D-FFI-CPP1=A: AST discovery and deterministic C++ shim compilation.
          pkgs.clang
          pkgs.lld
          # D-FFI-RUBY1=A: provision the supervised Ruby worker and stdlib JSON/Ripper.
          pkgs.ruby
          # D-FFI-PHP1=A: provision the supervised PHP worker pool.
          pkgs.php
          # D-FFI-R1=A: provision R plus the framed worker's JSON transport.
          jetR
        ];
        jetTzdb = "${pkgs.tzdata}/share/zoneinfo";
        # D-FOUND-PLATFORM1: the existing vetted font bridge loads HarfBuzz.
        jetRuntimeLibraryPath = pkgs.lib.makeLibraryPath [ pkgs.harfbuzz ];
        jetRuntimeLibraryVariable =
          if pkgs.stdenv.hostPlatform.isDarwin then "DYLD_LIBRARY_PATH" else "LD_LIBRARY_PATH";
        jetRuntimeLibraryHook = ''
          export ${jetRuntimeLibraryVariable}="${jetRuntimeLibraryPath}:''${${jetRuntimeLibraryVariable}:-}"
        '';
        # Foreign helper Cargo builds in the dev shells (user Rust FFI, measured
        # Rust peers) link through lld. The wrapped lld keeps Nix's
        # dynamic-linker and rpath handling; bare `pkgs.lld` would emit binaries
        # that cannot find libgcc_s. Set per shell, not in .cargo/config.toml,
        # so plain cargo outside the shell keeps the stdenv linker.
        jetCargoLinker = pkgs.writeShellScript "jet-cargo-linker" ''
          exec ${pkgs.stdenv.cc}/bin/cc -B${pkgs.llvmPackages.bintools}/bin/ -fuse-ld=lld "$@"
        '';
        # The dev shells' fixed variables. They are derivation attributes, not
        # shell-hook exports, so Tools/agent/JetEnv.jet reads them from the
        # cached shell environment without running a shell; the hooks keep only
        # what needs a live shell. `libraries` go on the dynamic library path
        # in front of HarfBuzz (and the Vulkan loader on Linux).
        jetShellVariables =
          libraries:
          {
            TZDIR = jetTzdb;
            # Nix owns this shell; do not stack the global Jet env auto-hook.
            JET_ENV_DISABLE = "1";
          }
          // (
            if pkgs.stdenv.hostPlatform.isDarwin then
              { DYLD_LIBRARY_PATH = jetRuntimeLibraryPath; }
              // pkgs.lib.optionalAttrs (libraries != [ ]) { LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libraries; }
            else
              {
                LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
                  pkgs.lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.vulkan-loader ++ libraries ++ [ pkgs.harfbuzz ]
                );
              }
          )
          // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            "CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_LINKER" = "${jetCargoLinker}";
            # The static musl whose libc.a a program's C closure links on the host
            # route (CC/AR named, or no jetpack engine beside jet: JetCli
            # ForeignLink.jet): its find_folder search looks on LIBRARY_PATH. The C
            # compiler's own -L flags come first, so host links keep glibc.
            LIBRARY_PATH = "${pkgs.pkgsStatic.musl}/lib";
          }
          // pkgs.lib.optionalAttrs jetHasMuslHelpers {
            JET_RUST_MUSL_TOOLCHAIN = "${jetRustToolchain}";
            JET_RUST_MUSL_LINKER = jetMuslLinker;
            JET_RUST_MUSL_AR = jetMuslAR;
          };
        # pkgs.vitejs calls fetchPnpmDeps without pinning `pnpm`, defaulting
        # to pnpm_11 (fetcherVersion 3), which nixpkgs rejects; pin pnpm_10.
        jetVite = pkgs.vitejs.override {
          fetchPnpmDeps = args: pkgs.fetchPnpmDeps (args // { pnpm = pkgs.pnpm_10; });
        };

        # `jet` / `jetpack` in the dev shell: run the admitted native binaries
        # named by $JET / $JETPACK from anywhere, with the runtime tools on PATH.
        # Without $JET, `jet` runs the installed release that `jet-use` picked.
        mkJetDevBin =
          name: variable:
          pkgs.writeShellScriptBin name ''
            set -euo pipefail
            bin="''${${variable}:-}"
            export PATH="${jetRuntimePath}:$PATH"
            ${jetRustHelperHook}
            ${jetRuntimeLibraryHook}
            if [ -z "$bin" ] && [ "${name}" = jet ]; then
              release="$(readlink -f "''${JET_SEEDS:-$HOME/.local/share/jet-seeds}/current" 2>/dev/null || true)"
              if [ -n "$release" ] && [ -x "$release/jet" ] && [ -f "$release/IDENTITY" ]; then
                bin="$release/jet"
                JET_COMPILER_IDENTITY="$(cat "$release/IDENTITY")"
                export JET_COMPILER_IDENTITY
                ulimit -s unlimited 2>/dev/null || ulimit -s 1048576 2>/dev/null || true
              fi
            fi
            if [ -z "$bin" ] || [ ! -x "$bin" ]; then
              echo "${name}: set ${variable} to the admitted native ${name} binary, or install a release with jet-use" >&2
              exit 1
            fi
            exec "$bin" "$@"
          '';
        jetDev = mkJetDevBin "jet" "JET";
        jetpackDev = mkJetDevBin "jetpack" "JETPACK";
      in
      {
        packages = {
          rust-musl-toolchain = jetRustToolchain;
        } // pkgs.lib.optionalAttrs jetHasMuslHelpers {
          musl-linker = jetMuslCC;
        };

        # Fast everyday shell. Keep normal compiler work independent from the
        # foreign-runtime, browser, graphics, and VM toolchain fleet below.
        devShells.default = pkgs.mkShell (jetShellVariables [ ] // {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.gcc
            pkgs.clang
            pkgs.lld
            # D-DBG3/D-DX-DEBUGBACK1: `jet debug` steps and records through LLDB.
            pkgs.lldb
            pkgs.nodejs_22
            pkgs.bun
            jetVite
            pkgs.nodemon
            pkgs.entr
            # agent_workloads / verify-full adapters shell out to python3.
            pkgs.python3
            # gauntlet competitive-corpus rails (owner-approved 2026-08-27):
            # Zig on perf entries, Go as the services incumbent.
            pkgs.zig
            pkgs.go
            # The train speed gate (Throughput.jet --against, run by the
            # release admit) counts each compile's instructions:u with
            # `perf stat`.
            pkgs.perf
            # The run tier's interpreter peers next to python3 and node. luajit
            # also installs a `lua`; lua5_4 comes first, so `lua` is Lua 5.4.
            pkgs.lua5_4
            pkgs.luajit
            pkgs.nixfmt
            pkgs.ripgrep
            pkgs.jq
            pkgs.gh
            pkgs.fd
            pkgs.bashInteractive
            pkgs.zsh
            pkgs.fish
            pkgs.util-linux
            pkgs.wasm-tools
            pkgs.tree-sitter
            jetDev
            jetpackDev
            pkgs.pkg-config
            # The https/TLS cases' loopback peer is `openssl s_server`
            # (`requires: tool:openssl`), so the testkit runs them here.
            pkgs.openssl
            # Jetpack script-registry claims run `php` on hostile archives; the
            # `jet` wrapper's runtime path already carries it.
            pkgs.php
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # D-BUILDENTRY1 / #95: core verification executes typed build actions.
            pkgs.bubblewrap
          ];

          shellHook = ''
            if repo_root="$(git rev-parse --show-toplevel 2>/dev/null)"; then
              export JET_ROOT="$repo_root"
            else
              export JET_ROOT="$PWD"
            fi
          '';
        });

        # Integration shell: full FFI matrix, Canvas, graphics, emulator, and
        # OS-image work. Use `nix develop .#full` / `jet Tools/agent/JetEnv.jet full …` explicitly.
        devShells.full = pkgs.mkShell (jetShellVariables [ ] // {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustup
            pkgs.gcc
            pkgs.clang
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
            # D-FFI-LUA1=A: provision Lua's in-process VM, headers, and compiler.
            pkgs.lua5_4
            # The run tier's LuaJIT peer, as in the default shell; after
            # lua5_4, so `lua` is Lua 5.4.
            pkgs.luajit
            pkgs.lld
            # D-FFI-RUBY1=A: provision the supervised Ruby worker and stdlib JSON/Ripper.
            pkgs.ruby
            # D-FFI-PHP1=A: provision the supervised PHP worker pool.
            pkgs.php
            # D-FFI-R1=A: provision R plus the framed worker's JSON transport.
            jetR
            # D-FFI-OCTAVE1=A: provision the MATLAB-compatible sidecar worker.
            pkgs.octave
            # Compiler freestanding smoke tests execute aarch64 output under
            # qemu-aarch64. OS image and VM tooling does not belong here.
            pkgs.qemu
            pkgs.nodejs_22
            pkgs.bun
            jetVite
            pkgs.nodemon
            pkgs.entr
            # agent_workloads / verify-full adapters shell out to python3.
            pkgs.python3
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
            pkgs.wasmtime
            pkgs.tree-sitter
            pkgs.emscripten
            pkgs.lldb
            jetDev
            jetpackDev
            pkgs.pkg-config
            # `tool:openssl` loopback TLS peer, as in the default shell.
            pkgs.openssl
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            # D-CANVASTEST1=A / #390: Canvas interaction tests drive Chromium
            # through CDP and Firefox through WebDriver Classic. Browser tools
            # are Linux-only so macOS dev shells remain evaluable.
            pkgs.chromium
            pkgs.firefox
            pkgs.geckodriver
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

            # banner on stderr: `nix develop -c <cmd>` stdout stays clean for
            # grepping/capture (agents misread results otherwise)
            {
              echo "Jet dev shell"
              echo "  compiler: JET=<admitted native jet>  (built by jet build Compiler)"
              echo "  run:      jet run Examples/Basics/Hello.jet"
              echo "  package:  jetpack help        (JETPACK=<native jetpack>)"
              echo "  search:   rg \"pattern\" Docs Compiler Core Jetpack tests"
              echo "  LSP:      jet lsp"
              echo "  editor:   Tools/editors/vscode/install.sh   (Cursor/VS Code)"
              echo "            Tools/editors/zed/install.sh        (Zed dev extension)"
              echo "  debug:    jet debug <file.jet>  (native lldb backend)"
            } >&2
          '';
        });

        formatter = pkgs.nixfmt;
      }
    );
}
