# C and C++ driver

This page defines the hermetic `jet cc` and `jet c++` interfaces. It is for
Make, CMake, and other build-host authors that need a C-family compiler slot.
The `jet` front door forwards `cc` and `c++` to Jetpack in
[`CommandOutput.jet`](../../../Compiler/JetCli/Source/Cli/CommandOutput.jet);
the driver implementation is in [`Jetpack/CLI/Source/CcDriver`](../../../Jetpack/CLI/Source/CcDriver/),
and the focused fixtures are in [`tests/guards/cc_driver_hosts.jet`](../../../tests/guards/cc_driver_hosts.jet).

`jet-cc` is the installed alias for `jet cc`; `jet-cxx` and `jet-c++` are
aliases for `jet c++`. The aliases are real binaries, not shell wrappers: each
release ships them as hard links of `jet` in the same directory, and `jet`
selects the verb from the name it was invoked as (`argv[0]`). They select one
Jetpack toolchain descriptor and pass the resulting action through Jet's build
graph.

## Toolchain admission

A toolchain descriptor names the C compiler, C++ compiler, linker, sysroot,
host, target, ABI, version, and content digests. The action records those facts
and the Hangar bundle identity. No compiler is vendored in this repository.

Each jet release publishes one C **toolchain pack** per host: static clang,
lld and llvm-ar, compiler-rt for every covered target, the musl sources, and
glibc headers with link stubs. `Tools/agent/cc-pack.nix` builds it
reproducibly from pinned sources on the release machine, and the release
refuses a pack whose SHA-256 differs from the one embedded in jet
(`CC_PACK_ASSETS` in
[`CcToolchainPack.jet`](../../../Jetpack/Provider/Source/Toolchain/CcToolchainPack.jet)).
Nix is used only there; a user machine needs no Nix. Because the hash lives in
jet, updating jet updates the pack.

On first use (`first use: fetching the C toolchain for jet <release> (111 MB)`)
jet takes the pack from beside the running jetpack, which is where the combined
`jet-<release>-<platform>-cc.tar.gz` archive puts it, else from the URL
`JET_CC_PACK_URL` names (`https://`, or `file://` for a local mirror or test
double, which also works offline), or else downloads it from the release. It
verifies the hash before unpacking; a copy with the wrong hash is refused and
deleted, and the error names the expected hash. The verified pack is unpacked
and recorded in the Hangar (`first use: unpacking the C toolchain into the
Hangar (first time only)`), and a Hangar hit never touches the network.
`jet env fetch-toolchain` fetches the pack ahead of time. With `--offline`, or
when network access is denied, a missing pack is one error that names both
fixes: `jet env fetch-toolchain`, or the combined archive. A missing or corrupt
object is an error, not an invitation to use the host compiler.

The per-target libc is built lazily into its own Hangar entry: a glibc target
copies the pack's headers and link stubs, and a musl target compiles `libc.a`
with the pack's own clang the first time it is used (`building musl for
<target> (first time only)`). The pack carries only libc. Any other library,
including the C++ standard library, is a package dependency.

The selected descriptor is part of the action key. Select its target with
`--target=<triple>`. The covered targets are `x86_64-linux-gnu`,
`aarch64-linux-gnu`, `x86_64-linux-musl`, and `aarch64-linux-musl`, each also
spelled in full (`aarch64-unknown-linux-musl`). The default is the host's
`-gnu` target. An uncovered target is a usage error that names the covered
targets, before graph creation. glibc targets link against glibc 2.42 headers
and stubs and run on glibc 2.34 or newer; a dynamic glibc program names the
target's standard loader (`/lib64/ld-linux-x86-64.so.2`,
`/lib/ld-linux-aarch64.so.1`). musl targets link statically. There is no host
fallback and there is **no PATH fallback** for the compiler, linker, or
sysroot.

`--sysroot`, `-B`, and `-fuse-ld` cannot replace the descriptor's sysroot or
linker. The target descriptor owns those choices. `-c`, `-o`, `-MD`, `-MMD`,
`-MF`, and `-MT` are supported and become explicit action inputs or outputs.

## Scoped paths and response files

An absolute source, include, library, output, or dependency-file path needs an
explicit scope. Pass `--project-root` for source and input paths and
`--build-root` for outputs. Put source operands after `--`; a response file
must itself be under one of those roots.

Project and build roots must be real directories inside the project. Existing
symlinks and paths that escape either scope are rejected. Response files use
bounded whitespace, single-quote, double-quote, and backslash parsing. They do
not perform shell expansion. Nested response files, cycles, file size, and byte
count each have fixed limits.

The driver forwards selected compiler flags, dependency targets, and the
virtual sysroot into the action. Target-changing flags, linker injection,
host sysroot overrides, and out-of-scope paths fail before an output is
accepted.

`--fixtures` is an explicit fixture seam for tests. It is not production tool
acquisition: production uses the toolchain pack.

## The native C link

`jet run`, `jet build` and `jet build --release` compile a program's `c@path`
sources and `#FFI(c)` inline bodies with the same tools on every tier
(`Compiler/JetCli/Source/Cli/ForeignLink.jet`). Unless `CC` or `AR` names a
host tool, those tools are this pack's `bin/clang` and `bin/llvm-ar`, so the
machine needs no host `cc` or `ar`. A Jet program's C closure compiles for
`x86_64-unknown-linux-musl` against the musl sysroot and links statically
with its `libc.a` and the pack's compiler-rt builtins; `jet run` of a C or C++
file builds for `x86_64-unknown-linux-gnu` with the pack's glibc sysroot.
jet asks the `jetpack` engine of its own version for the pack:
`jet env fetch-toolchain --json` puts it in the Hangar as above, realizes the
host's musl target (the first time in a Hangar, its sysroot is built from the
pack's musl sources, about half a minute; later runs reuse it) and prints
`{"root": …, "musl_sysroot": …, "llvm": …, "musl": …, "glibc": …}`.
`JET_CC_PACK_ROOT` names an unpacked pack to build C and C++ files with as
is; a Jet program's C closure still takes the musl sysroot from the engine.
Only a jet with no such engine beside it falls back to `cc` and `ar` on
`PATH`, and a C closure then links the musl `libc.a` on LIBRARY_PATH. A golden
test run with a jetpack takes the pack once and names it to every row through
`JET_CC_PACK_ROOT`, so no tier's fresh Hangar unpacks its own copy for a C
file; a row that links a C closure realizes the pack and its musl sysroot in
its own Hangar.

## Reproducible acquisition checks

A clean-machine check exercises acquisition, reuse, and the offline path with
one compiler command. The machine needs no Nix and no host compiler:

```sh
time jet cc --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
time jet cc --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
time jet cc --offline --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
```

Record the descriptor from `jet cc -v`, the Hangar receipt, and the declared
output digest. A valid comparison uses the same jet release, source, target,
descriptor, and action inputs on both machines. A run that uses `--fixtures`
or a host compiler is not this proof.

For an air-gapped machine, either install jet from the combined
`jet-<release>-<platform>-cc.tar.gz` archive, or fetch the pack on a connected
machine and move the Hangar entries:

```sh
jet env fetch-toolchain
jetpack hangar export <cc-toolchain-pack-entry> --to cc-toolchain.hangar --yes
jetpack hangar verify cc-toolchain.hangar
# transport cc-toolchain.hangar
jetpack hangar verify cc-toolchain.hangar
jetpack hangar import cc-toolchain.hangar --yes
jet cc --offline --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
```

## Make and CMake integration

The driver does not discover `cc` or `c++` from `PATH`. The caller supplies the
aliases and path scopes to the ordinary compiler slots. The fixtures cover a
clean build, a no-op rebuild, an edit, and an error path for both C and C++.
They also cover dependency files, include paths, language standards, and a
response file.

```sh
repo="$(git rev-parse --show-toplevel)"
jet="${JET:-$(command -v jet)}"
cc="$(dirname "$jet")/jet-cc"
cxx="$(dirname "$jet")/jet-c++"
make_src="$repo/tests/fixtures/foreign_build_hosts/make-cc"
make_build="$make_src/build"

make -C "$make_src" CC="$cc" CXX="$cxx" BUILD_ROOT="$make_build" clean
make -C "$make_src" CC="$cc" CXX="$cxx" BUILD_ROOT="$make_build" all
make -C "$make_src" CC="$cc" CXX="$cxx" BUILD_ROOT="$make_build" all
"$make_build/cc-driver"
"$make_build/cxx-driver"
```

```sh
cmake_src="$repo/tests/fixtures/foreign_build_hosts/cmake-cc"
cmake_build="$cmake_src/build"
cmake -S "$cmake_src" -B "$cmake_build" \
  -DCMAKE_C_COMPILER="$cc" \
  -DCMAKE_CXX_COMPILER="$cxx" \
  -DCMAKE_C_FLAGS_INIT="--project-root=$cmake_src --build-root=$cmake_build" \
  -DCMAKE_CXX_FLAGS_INIT="--project-root=$cmake_src --build-root=$cmake_build" \
  -DCMAKE_EXE_LINKER_FLAGS_INIT="--project-root=$cmake_src --build-root=$cmake_build"
cmake --build "$cmake_build"
cmake --build "$cmake_build"
cmake --build "$cmake_build" --target clean
cmake --build "$cmake_build"
"$cmake_build/cc-driver"
"$cmake_build/cxx-driver"
```

The second Make/CMake build is the no-op case. Touching
`include/config.h` checks header invalidation; editing either source checks
source invalidation; changing the build root checks action identity. An
uncovered cross-target request and each invalid input below must fail before
creating its named output:

```sh
"$cc" --project-root="$make_src" --build-root="$make_build" \
  --target=riscv64-unknown-linux-gnu -c -- "$make_src/main.c" \
  -o "$make_build/unsupported.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  --sysroot=/host -c -- "$make_src/main.c" -o "$make_build/host-sysroot.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  -Wl,-z,now -c -- "$make_src/main.c" -o "$make_build/linker-override.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  -c -- "$make_src/../outside.c" -o "$make_build/escape.o"
```

The uncovered-target error (E2102) names every covered target. The other
errors identify the unsupported override or scoped input. None may be repaired
by consulting a host compiler or host filesystem.
