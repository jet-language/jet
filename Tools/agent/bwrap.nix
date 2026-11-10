# Jet's pinned Bubblewrap: the Linux build-action sandbox Jet fetches on first
# need when the host has no usable `bwrap` (Docs/spec/reference/environment.md,
# "Native sandbox"). `Tools/release/Main.jet -- admit` builds it with
#
#   nix-build Tools/agent/bwrap.nix
#
# and refuses a release whose binary hash differs from the one compiled into
# jet (NATIVE_SANDBOX_BWRAP_ASSETS in Jetpack/Foundation/Source/NativeSandbox.jet).
# It uses the C toolchain pack's nixpkgs pin (Tools/agent/cc-pack.nix), so
# anyone can rebuild it and compare the hash.
#
# The output holds the release asset, one static stripped executable, and the
# Bubblewrap license it is published with:
#   jet-bwrap-x86_64-linux           static bwrap (libcap without its Go tools)
#   bubblewrap-COPYING               its license (LGPL-2.0-or-later)
#   sha256                           the asset's SHA-256
let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/7c8764b7c7b09b34f632464276218ef9090eaa11.tar.gz";
    sha256 = "sha256-MWhktfFtFE8Jl7bf9Pkp7TR5MSdYCjCswSo0Gkb4ywc=";
  }) { };
  host = pkgs.llvmPackages_21;
  bwrap = pkgs.pkgsStatic.bubblewrap.override { libcap = pkgs.pkgsStatic.libcap.override { withGo = false; }; };
in
pkgs.runCommand "jet-bwrap-x86_64-linux" { } ''
  set -euo pipefail
  mkdir -p $out
  cp -L ${bwrap}/bin/bwrap $out/jet-bwrap-x86_64-linux
  chmod u+w $out/jet-bwrap-x86_64-linux
  ${host.llvm}/bin/llvm-strip --strip-all $out/jet-bwrap-x86_64-linux
  chmod 0555 $out/jet-bwrap-x86_64-linux
  cp ${bwrap.src}/COPYING $out/bubblewrap-COPYING
  sha256sum $out/jet-bwrap-x86_64-linux | cut -d' ' -f1 > $out/sha256
''
