# The C toolchain pack `jet cc` fetches on first use
# (Docs/spec/reference/cc-driver.md). release-compiler.mjs builds it with
#
#   nix-build Tools/agent/cc-pack.nix
#
# and refuses a release whose pack hash differs from the one compiled into
# jetpack (Jetpack/Provider/Source/Toolchain/CcToolchainPack.jet). Every input
# is pinned below, so anyone can rebuild the pack and compare the hash. Nix
# only builds the pack on the release machine; no jet user needs Nix.
#
# Layout of jet-cc-pack-<host>.tar.zst (a ustar archive, sorted, mtime 0,
# owner 0, compressed with zstd -19):
#   jet-cc-pack.json                 schema, host, versions and covered targets
#   bin/clang bin/ld.lld bin/llvm-ar static LLVM tools for the host
#   lib/clang/21/include/            clang's resource headers
#   lib/clang/21/lib/<triple>/       compiler-rt builtins and crtbegin/crtend per target
#   musl/src/                        the musl source (covered architectures only)
#   musl/<arch>/include/             musl's installed headers for <arch>
#   musl/<arch>/sources              the libc.a sources for <arch>, as musl's Makefile picks them
#   glibc/<triple>/usr/{include,lib} glibc headers, crt objects and stripped shared objects
#                                    (link inputs only) for each gnu target, without the
#                                    eight iptables target headers whose names differ from
#                                    a match header only in case (the Hangar refuses such
#                                    paths, so the pack also unpacks on case-folding disks)
#   share/licenses/                  the licenses of LLVM, musl and glibc
#
# `static = false` assembles the same tree from the dynamic LLVM tools, for
# developing the pack format on a machine that has them; such a pack only runs
# where its /nix/store closure exists and is never released.
{ static ? true }:
let
  pkgs = import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/7c8764b7c7b09b34f632464276218ef9090eaa11.tar.gz";
    sha256 = "sha256-MWhktfFtFE8Jl7bf9Pkp7TR5MSdYCjCswSo0Gkb4ywc=";
  }) { };
  lib = pkgs.lib;
  tools = if static then pkgs.pkgsStatic.llvmPackages_21 else pkgs.llvmPackages_21;
  host = pkgs.llvmPackages_21;
  glibcFor = {
    x86_64 = pkgs.glibc;
    aarch64 = pkgs.pkgsCross.aarch64-multiplatform.glibc;
  };
  musl = pkgs.musl.src;
  compilerRt = host.compiler-rt.src;
  muslSources = pkgs.writeText "musl-sources.py" ''
    # musl's Makefile source selection for one ARCH: every src/*/*.c and
    # src/malloc/mallocng/*.c, where src/<dir>/<ARCH>/<name>.[csS] replaces
    # src/<dir>/<name>.c. crt/ and ldso/ are not part of libc.a.
    import os, sys
    root, arch = sys.argv[1], sys.argv[2]
    dirs = sorted("src/" + d for d in os.listdir(root + "/src") if os.path.isdir(root + "/src/" + d))
    dirs.append("src/malloc/mallocng")
    objs = {}
    replaced = set()
    for d in dirs:
        for n in sorted(os.listdir(root + "/" + d)):
            if n.endswith(".c"):
                objs[d + "/" + n[:-2]] = d + "/" + n
        a = root + "/" + d + "/" + arch
        if os.path.isdir(a):
            for n in sorted(os.listdir(a)):
                stem, ext = os.path.splitext(n)
                if ext in (".c", ".s", ".S"):
                    objs[d + "/" + arch + "/" + stem] = d + "/" + arch + "/" + n
                    replaced.add(d + "/" + stem)
    for r in replaced:
        objs.pop(r, None)
    for key in sorted(objs):
        print(objs[key])
  '';
  pathLaw = pkgs.writeText "path-law-check.py" ''
    # Fails on any pack path the Hangar's path law refuses
    # (Jetpack/PackageModel/Source/Envelope/PathLaw.jet): siblings that differ
    # only in ASCII case, Windows reserved names, a trailing `.` or space, or a
    # backslash.
    import os, sys
    reserved = {"CON", "PRN", "AUX", "NUL"} | {p + str(n) for p in ("COM", "LPT") for n in range(1, 10)}
    for d, dirs, files in os.walk(sys.argv[1]):
        seen = {}
        for n in sorted(dirs + files):
            p = os.path.join(d, n)
            if n.lower() in seen:
                sys.exit("case-folding collision: %s and %s" % (seen[n.lower()], p))
            seen[n.lower()] = p
            dot = n.find(".")
            if (n[:dot] if dot > 0 else n).upper() in reserved or n[-1] in ". " or "\\" in n:
                sys.exit("not a portable store path: %s" % p)
  '';
  manifest = builtins.toJSON {
    schema = 1;
    host = "x86_64-linux";
    llvm = tools.clang-unwrapped.version;
    musl = pkgs.musl.version;
    glibc = pkgs.glibc.version;
    targets = [
      { triple = "x86_64-unknown-linux-musl"; libc = "musl"; arch = "x86_64"; }
      { triple = "aarch64-unknown-linux-musl"; libc = "musl"; arch = "aarch64"; }
      { triple = "x86_64-unknown-linux-gnu"; libc = "glibc"; arch = "x86_64"; }
      { triple = "aarch64-unknown-linux-gnu"; libc = "glibc"; arch = "aarch64"; }
    ];
  };
in
pkgs.runCommand "jet-cc-pack-x86_64-linux" {
  nativeBuildInputs = [ pkgs.cmake pkgs.ninja pkgs.python3 pkgs.gnutar pkgs.zstd ];
  passAsFile = [ "manifest" ];
  inherit manifest;
} ''
  set -euo pipefail
  R=$PWD/root
  STRIP=${host.llvm}/bin/llvm-strip
  mkdir -p $R/bin $R/lib/clang/21 $R/share/licenses $out
  cp -L ${tools.clang-unwrapped}/bin/clang $R/bin/clang
  cp -L ${tools.lld}/bin/ld.lld $R/bin/ld.lld
  cp -L ${tools.llvm}/bin/llvm-ar $R/bin/llvm-ar
  chmod u+w $R/bin/*
  $STRIP --strip-all $R/bin/clang $R/bin/ld.lld $R/bin/llvm-ar
  cp -rL ${lib.getLib tools.clang-unwrapped}/lib/clang/21/include $R/lib/clang/21/include
  chmod -R u+w $R
  cp ${compilerRt}/llvm/LICENSE.TXT $R/share/licenses/LLVM-LICENSE.TXT

  # musl: the source of the covered architectures, then each one's installed headers.
  tar xzf ${musl}
  M=$PWD/musl-${pkgs.musl.version}
  cp $M/COPYRIGHT $R/share/licenses/musl-COPYRIGHT
  mkdir -p $R/musl/src/arch
  cp -r $M/include $M/src $M/crt $M/ldso $M/VERSION $R/musl/src/
  for a in generic x86_64 aarch64; do cp -r $M/arch/$a $R/musl/src/arch/$a; done
  for a in x86_64 aarch64; do
    I=$R/musl/$a/include
    mkdir -p $I/bits
    cp -r $M/include/. $I/
    cp $M/arch/generic/bits/*.h $I/bits/
    cp $M/arch/$a/bits/*.h $I/bits/
    sed -f $M/tools/mkalltypes.sed $M/arch/$a/bits/alltypes.h.in $M/include/alltypes.h.in > $I/bits/alltypes.h
    cp $M/arch/$a/bits/syscall.h.in $I/bits/syscall.h
    sed -n -e s/__NR_/SYS_/p < $M/arch/$a/bits/syscall.h.in >> $I/bits/syscall.h
    python3 ${muslSources} $M $a > $R/musl/$a/sources
  done

  # glibc: headers, crt objects, libc_nonshared.a and the stripped shared
  # objects lld links against; libc.so and libm.so are sysroot-relative scripts.
  for a in x86_64 aarch64; do
    t=$a-unknown-linux-gnu
    case $a in
      x86_64) G=${glibcFor.x86_64}; GD=${glibcFor.x86_64.dev}; LDSO=ld-linux-x86-64.so.2; FMT=elf64-x86-64 ;;
      aarch64) G=${glibcFor.aarch64}; GD=${glibcFor.aarch64.dev}; LDSO=ld-linux-aarch64.so.1; FMT=elf64-littleaarch64 ;;
    esac
    S=$R/glibc/$t/usr
    mkdir -p $S/lib
    cp -rL $GD/include $S/include
    chmod -R u+w $S
    rm $S/include/linux/netfilter/xt_{CONNMARK,DSCP,MARK,RATEEST,TCPMSS}.h \
      $S/include/linux/netfilter_ipv4/ipt_{ECN,TTL}.h $S/include/linux/netfilter_ipv6/ip6t_HL.h
    for f in crt1.o Scrt1.o rcrt1.o crti.o crtn.o libc_nonshared.a; do cp $G/lib/$f $S/lib/; done
    for f in libc.so.6 libm.so.6 libmvec.so.1 libpthread.so.0 libdl.so.2 librt.so.1 libutil.so.1 libresolv.so.2 $LDSO; do
      [ -f $G/lib/$f ] || continue
      $STRIP --strip-all -o $S/lib/$f $G/lib/$f
    done
    for f in libpthread libdl librt libutil libresolv; do
      [ -f $S/lib/$f.so.* ] && printf 'GROUP ( /usr/lib/%s )\n' $(basename $S/lib/$f.so.*) > $S/lib/$f.so
    done
    printf 'OUTPUT_FORMAT(%s)\nGROUP ( /usr/lib/libc.so.6 /usr/lib/libc_nonshared.a AS_NEEDED ( /usr/lib/%s ) )\n' $FMT $LDSO > $S/lib/libc.so
    if [ -f $S/lib/libmvec.so.1 ]; then
      printf 'GROUP ( /usr/lib/libm.so.6 AS_NEEDED ( /usr/lib/libmvec.so.1 ) )\n' > $S/lib/libm.so
    else
      printf 'GROUP ( /usr/lib/libm.so.6 )\n' > $S/lib/libm.so
    fi
  done
  printf 'glibc %s: GNU LGPL 2.1 or later, https://www.gnu.org/licenses/lgpl-2.1.html\n' ${pkgs.glibc.version} > $R/share/licenses/glibc-LICENSE

  # compiler-rt builtins and crtbegin/crtend for every target, built by the
  # pack's own clang against that target's libc headers.
  for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    a=''${t%%-*}
    S=$PWD/sys-$t
    mkdir -p $S/usr
    case $t in
      *-musl) cp -r $R/musl/$a/include $S/usr/include ;;
      *-gnu) cp -r $R/glibc/$t/usr/include $S/usr/include ;;
    esac
    B=$PWD/rt-$t
    cmake -G Ninja -S ${compilerRt}/compiler-rt/lib/builtins -B $B -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_C_COMPILER=$R/bin/clang -DCMAKE_CXX_COMPILER=$R/bin/clang -DCMAKE_ASM_COMPILER=$R/bin/clang \
      -DCMAKE_AR=$R/bin/llvm-ar -DCMAKE_RANLIB=${host.llvm}/bin/llvm-ranlib -DCMAKE_NM=${host.llvm}/bin/llvm-nm \
      -DCMAKE_C_COMPILER_TARGET=$t -DCMAKE_CXX_COMPILER_TARGET=$t -DCMAKE_ASM_COMPILER_TARGET=$t -DCMAKE_SYSROOT=$S \
      -DCMAKE_C_FLAGS="-resource-dir=$R/lib/clang/21" -DCMAKE_CXX_FLAGS="--driver-mode=g++ -resource-dir=$R/lib/clang/21" \
      -DCMAKE_ASM_FLAGS="-resource-dir=$R/lib/clang/21" \
      -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY -DCOMPILER_RT_DEFAULT_TARGET_ONLY=ON -DCOMPILER_RT_BUILD_CRT=ON \
      -DLLVM_ENABLE_PER_TARGET_RUNTIME_DIR=ON -DLLVM_CMAKE_DIR=${compilerRt}/llvm/cmake/modules -DCOMPILER_RT_STANDALONE_BUILD=ON
    ninja -C $B -j''${NIX_BUILD_CORES:-1}
    mkdir -p $R/lib/clang/21/lib/$t
    for f in libclang_rt.builtins.a clang_rt.crtbegin.o clang_rt.crtend.o; do cp $B/lib/$t/$f $R/lib/clang/21/lib/$t/$f; done
  done

  # Every path must pass the Hangar's path law, or no jet could install the pack.
  python3 ${pathLaw} $R

  cp $manifestPath $R/jet-cc-pack.json
  tar --sort=name --format=ustar --owner=0 --group=0 --numeric-owner --mtime=@0 --mode='a+rX,u+w,go-w' \
    --hard-dereference -C $R -cf - . | zstd -19 -T1 -q -o $out/jet-cc-pack-x86_64-linux.tar.zst
  sha256sum $out/jet-cc-pack-x86_64-linux.tar.zst | cut -d' ' -f1 > $out/sha256
''
