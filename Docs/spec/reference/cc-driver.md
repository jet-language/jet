# C and C++ driver

This page defines the hermetic `jet cc` and `jet c++` interfaces. It is for
Make, CMake, and other build-host authors that need a C-family compiler slot.
The executable contract lives in [`Source/main.rs`](../../../Source/main.rs),
the driver implementation in [`Source/CmdCompile.rs`](../../../Source/CmdCompile.rs),
and the focused fixtures in [`tests/cc_driver_hosts.rs`](../../../tests/cc_driver_hosts.rs).

`jet-cc` is the installed alias for `jet cc`; `jet-cxx` and `jet-c++` are
aliases for `jet c++`. The aliases are real binaries, not shell wrappers. They
select one Jetpack toolchain descriptor and pass the resulting action through
Jet's build graph.

## Toolchain admission

A toolchain descriptor names the C compiler, C++ compiler, linker, sysroot,
host, target, ABI, version, and content digests. The action records those facts
and the Hangar bundle identity. An acquisition record is metadata; it does not
vendor a compiler in this repository.

On first use, Jetpack resolves the exact compiler and linker attributes from
the official **signed Nix index**, verifies the signed output paths, admits the
complete signed Nix closure through Hangar, and publishes a content-addressed
descriptor with role digests and a closure receipt. A later invocation reuses
that descriptor. `--offline` is valid only when the signed index and complete
Hangar closure were already loaded; a missing or corrupt object is an error,
not an invitation to use the host compiler.

The selected descriptor is part of the action key. Select its target with
`--target=<triple>` when the target has an acquisition record. An unadmitted
cross-target request fails before graph creation. There is no host fallback and
there is **no PATH fallback** for the compiler, linker, or sysroot.

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
acquisition: production uses the signed Nix index and signed Nix cache.

## Reproducible acquisition checks

A clean-machine check exercises acquisition, reuse, and the offline path with
one compiler command. Use a separately provisioned signed index, public key,
and Hangar closure on each machine:

```sh
time jet cc --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
time jet cc --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
time jet cc --offline --project-root="$project" --build-root="$build" \
  -c -- main.c -o main.o
```

Record the descriptor from `jet cc -v`, the Hangar receipt, and the declared
output digest. A valid comparison uses the same source, target, descriptor,
and action inputs on both machines. A run that uses `--fixtures`, a host
compiler, or an unsigned/unconfigured index is not this proof.

For an air-gapped transfer, export and verify the closure on a connected
machine, transfer the archive and signed index/cache preload, verify again,
then import before the offline build:

```sh
jetpack hangar export <cc-toolchain-entry> --to cc-toolchain.hangar --yes
jetpack hangar verify cc-toolchain.hangar
# transport cc-toolchain.hangar and the signed index/cache preload
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
cc="$repo/target/debug/jet-cc"
cxx="$repo/target/debug/jet-c++"
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
source invalidation; changing the build root checks action identity. A
cross-target request and each invalid input below must fail before creating its
named output:

```sh
"$cc" --offline --project-root="$make_src" --build-root="$make_build" \
  --target=aarch64-unknown-linux-gnu -c -- "$make_src/main.c" \
  -o "$make_build/unsupported.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  --sysroot=/host -c -- "$make_src/main.c" -o "$make_build/host-sysroot.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  -Wl,-z,now -c -- "$make_src/main.c" -o "$make_build/linker-override.o"
"$cc" --project-root="$make_src" --build-root="$make_build" \
  -c -- "$make_src/../outside.c" -o "$make_build/escape.o"
```

The cross-target error names the absent signed acquisition record. The other
errors identify the unsupported override or scoped input. None may be repaired
by consulting a host compiler or host filesystem.
