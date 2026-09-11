# C and C++ driver

`jet cc` and `jet c++` use one Jetpack toolchain descriptor.

The descriptor names the compiler, C++ compiler, linker, sysroot, host, target,
ABI, version, and content digests. The build action records these facts. It also
records the Hangar bundle identity.

The driver does not search `PATH`. It does not use a host compiler or linker.
The action declares the tool paths and the read-only Hangar bundle mount. Linux
uses the declared virtual mount path. macOS and Windows use the matching
read-only Hangar path through the sandbox adapter.

## Acquisition record

The production record is:

```text
id       jet-cc-nixos-unstable-e5bdc4a4-x86_64-linux-v1
channel  nixos-unstable
revision e5bdc4a41d4c072fe1e3787eaa0320a384741d44
system   x86_64-linux
target   x86_64-unknown-linux-gnu
compiler gcc       bin/gcc
c++      gcc       bin/g++
linker   lld       bin/ld.lld
sysroot  gcc       nix-support/orig-libc-dev
ABI      gnu
```

The record is an acquisition description, not a compiler payload. On first
use Jetpack resolves the exact `gcc` and `lld` attributes from the official
signed Nix index, verifies the signed output paths, admits the complete signed
Nix closure through Hangar, and publishes a content-addressed descriptor with
the closure receipt and role digests. No compiler is vendored in this
repository. A later invocation reuses that descriptor; `--offline` also works
when the signed index and the complete Hangar closure were preloaded.

The current production record declares one supported target:
`x86_64-unknown-linux-gnu` on `x86_64-linux`. A target without an acquisition
record is rejected before graph creation. Adding a target requires a new
signed-index and signed-cache record; it never falls back to the host.

Use `--target=<triple>` to select a declared target. The selected descriptor
chooses the sysroot. A user cannot replace it with `--sysroot`, `-B`,
`-fuse-ld`, or another host tool override.

`-c`, `-o`, `-MD`, `-MMD`, `-MF`, and `-MT` are supported. The driver records
source files, dependency files, and declared output paths in the common action
graph. The action key includes the toolchain and bundle identity.

An ordinary absolute source, include, library, output, or dependency-file path
is accepted only when its scope is explicit: pass `--project-root` and, for
outputs, `--build-root`; put source operands after `--`; or put them in a
response file under one of those roots. Project and build roots must be real
directories inside the project. Existing symlinks are rejected. Response files
use bounded whitespace, single-quote, double-quote, and backslash parsing; no
shell expansion is performed. Nested response files, cycles, files, and bytes
have fixed limits.

The driver forwards the selected compiler flags, exact dependency target, and
the descriptor's virtual sysroot into the action. Host `--sysroot`, `-B`,
`-fuse-ld`, linker injection, unsupported target-changing flags, and paths that
escape their scope fail before a valid output can be accepted. There is no PATH
fallback.

The fixture manifest remains an explicit test seam only. Production acquisition
uses the signed Nix index and signed Nix cache described above.

## Clean-machine and air-gap proof

Run the same matrix with an empty `JETPACK_ROOT` on two machines. Install the
signed index endpoint and public key as a pair before the first run. The timing
wrapper records first acquisition, a cached invocation, and an offline
invocation without changing the compiler command:

```sh
time jet cc --project-root="$project" --build-root="$build" -c -- main.c -o main.o
time jet cc --project-root="$project" --build-root="$build" -c -- main.c -o main.o
time jet cc --offline --project-root="$project" --build-root="$build" -c -- main.c -o main.o
```

Save the three elapsed times, the `jet cc -v` descriptor, and the Hangar
receipt. Compare the declared output digest and receipt identity from both
machines. A timing run is invalid if it uses `--fixtures`, a host compiler, or
an unconfigured unsigned index.

For an air-gapped transfer, export the verified toolchain closure on the
connected machine, verify the archive before transport, then import it into an
empty Hangar on the disconnected machine:

```sh
jetpack hangar export <cc-toolchain-entry> --to cc-toolchain.hangar --yes
jetpack hangar verify cc-toolchain.hangar
# transport cc-toolchain.hangar and the signed index/cache preload
jetpack hangar verify cc-toolchain.hangar
jetpack hangar import cc-toolchain.hangar --yes
time jet cc --offline --project-root="$project" --build-root="$build" -c -- main.c -o main.o
```

The offline run must use the imported descriptor and complete closure. Missing
or corrupt archive members, index records, or closure objects are failures;
the driver does not repair them by consulting the host filesystem.

## Make and CMake

The fixture projects use ordinary compiler slots and absolute alias paths. The
caller supplies the aliases and scopes; neither build system discovers a host
compiler:

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

The second Make/CMake build is the no-op proof. Touching
`include/config.h` in a copied fixture, editing either source, and changing the
build root are the edit and cross-target invalidation proofs. The fixtures use
`-MMD`, `-MP`, `-MF`, `-MT`, `-I`, `-std`, both languages, and a response file.

The negative cross-target and error proofs are:

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

Each command must fail before creating its named output. The cross-target
command reports that no signed acquisition record exists; the other commands
report a scoped-input or unsupported-override diagnostic.
