# Foreign build hosts

Jet can be consumed by an existing CMake, Gradle, Bazel, or MSBuild project.
Each adapter invokes the same native Library path:

```text
jet build --lib --locked --output <manifest-output> <entry.jet>
```

The host project names the checked manifest output and the native Library name;
the adapter records both and fails if the requested artifact is absent. `ENTRY`
must be the manifest output's checked `entry` selector when that field is
present. The adapter declares `package.jet`, `.jet/lock`, the entry file, and
every extra Jet source in the host dependency graph. The generated header and
archive/shared object are copied into the host build directory.
`jet-host.receipt` is escaped JSON, schema 2. It records the exact locked Jet
command, Jet identity, manifest output, profile, compiler/linker/toolchain and
target identities, lock digest, input closure digests, and published artifact
digests.

The runner rejects symlink/reparse roots and paths, removes the old host
outputs before starting, serializes direct builds of one Jet project, and
stages the complete new set. It renames each artifact and the receipt, then
writes `jet-host.stamp` last. The stamp contains the receipt digest and is the
commit marker: a host must verify it before using the set. A failed, cancelled,
or timed-out build leaves no valid-looking host output. Stale locks owned by a
dead process are recovered; a live lock has a bounded wait.

## CMake

Pass `cmake/JetToolchain.cmake` as `CMAKE_TOOLCHAIN_FILE`, then use
`find_package(Jet REQUIRED)`. Set `Jet_EXECUTABLE` to an absolute path when
Jet is not installed on `PATH`. The toolchain file only makes discovery
available during the first configure pass; CMake still selects the host C/C++
compiler.

```cmake
find_package(Jet REQUIRED)
jet_library(jet_loadable ENTRY library.jet OUTPUT core LIBRARY loadable DEPENDS src/extra.jet LOADABLE)
target_link_libraries(app PRIVATE jet_loadable)
```

`STATIC` is the default. Use `SHARED` when the host wants the shared native
artifact. On Windows, the current Jet Library export is a GNU `.a`; the module
rejects an MSVC ABI before a misleading link error. The adapter passes every
argument as a separate process argument and checks the Jet completion marker
before copying any artifact.

### C/C++ driver mode

The Library adapter above is for Jet sources. An existing C/C++ project can
also put the hermetic driver in CMake's normal compiler slots. Pass absolute
`jet-cc` and `jet-c++` paths, and pass the explicit project/build scopes in
the language and linker flags:

```sh
cmake -S tests/fixtures/foreign_build_hosts/cmake-cc \
  -B tests/fixtures/foreign_build_hosts/cmake-cc/build \
  -DCMAKE_C_COMPILER="$PWD/target/debug/jet-cc" \
  -DCMAKE_CXX_COMPILER="$PWD/target/debug/jet-c++" \
  -DCMAKE_C_FLAGS_INIT="--project-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc --build-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc/build" \
  -DCMAKE_CXX_FLAGS_INIT="--project-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc --build-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc/build" \
  -DCMAKE_EXE_LINKER_FLAGS_INIT="--project-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc --build-root=$PWD/tests/fixtures/foreign_build_hosts/cmake-cc/build"
cmake --build tests/fixtures/foreign_build_hosts/cmake-cc/build
```

This mode does not call `find_program`, does not select `cc`/`c++` from
`PATH`, and keeps CMake's generated absolute source/output paths inside the
declared roots. See `docs/reference/cc-driver.md` for the clean, no-op, edit,
offline, cross-target, and failure matrix.

## Make C/C++ driver mode

The direct Make fixture uses the ordinary `CC` and `CXX` variables. It rejects
missing or non-absolute compiler paths, so the caller must provide the
installed aliases:

```sh
make -C tests/fixtures/foreign_build_hosts/make-cc \
  CC="$PWD/target/debug/jet-cc" \
  CXX="$PWD/target/debug/jet-c++" \
  BUILD_ROOT="$PWD/tests/fixtures/foreign_build_hosts/make-cc/build" all
```

The fixture exercises both languages, `-MMD`/`-MP`/`-MF`/`-MT`, a bounded
response file, and explicit project/build roots. It is a build-system
integration of the same `jet cc` action graph, not a host compiler wrapper.

## Gradle

Apply `gradle/jet-library.gradle`. Set `toolsDir` when the adapter is outside
the Gradle root. The `jetLibrary` task is incremental over its declared inputs;
host compile tasks should depend on it and declare their own source/output.

```groovy
apply from: file("gradle/jet-library.gradle")
jetLibrary { entry = "library.jet"; output = "core"; library = "loadable"; loadable = true; inputs = ["src/extra.jet"] }
```

The extension exposes `artifactDirectory()`, `staticLibrary()`,
`sharedLibrary()`, `header()`, `receipt()`, and `stamp()` for a host task.

## Bazel

Make the adapter directory available as the `jet_hosts` local repository, load
`jet_library`, and pass the complete Jet source closure in `deps`.

```python
load("@jet_hosts//bazel:jet_library.bzl", "jet_library")
jet_library(name = "loadable", entry = "library.jet", output = "core", library = "loadable", deps = ["src/extra.jet"], loadable = True)
cc_binary(name = "app", srcs = ["host.c"], deps = [":loadable"])
```

The rule uses Bazel's action sandbox and passes every Jet argument through
`ctx.actions.args()` as a separate argv element. It exports a normal
`cc_library` backed by the checked Jet archive and generated header. The current
rule intentionally exports `STATIC`; a request for another kind is an explicit
ABI diagnostic.

## MSBuild

Set `JetLibrary*` properties before importing `msbuild/Jet.Library.targets`.
The target exposes
`JetLibraryStatic`, `JetLibraryShared`, `JetLibraryHeader`,
`JetLibraryReceipt`, and `JetLibraryStamp`. Set `JetLibraryInputs` to the
complete Jet source closure and make the host compile target depend on
`JetLibrary`.

```xml
<Import Project="path\to\Jet.Library.targets" />
<Target Name="Build" DependsOnTargets="JetLibrary">
  <!-- compile/link the host with $(JetLibraryHeader) and $(JetLibraryStatic) -->
</Target>
```

The targets use PowerShell for Windows-native locking and staging. A host using
MSVC against the current GNU `.a` export must fail with `JET-HOST-ABI` and use
a GNU-compatible C/C++ toolset instead. `lifecycle.cpp` is a Windows-compilable
fixture that loads the shared library, resolves `on_tick`, calls it from the
main thread and a worker thread, and repeats load/unload.

## Proof matrix

Run these on a machine with the named host tool. The repository's focused test
checks the adapter contracts and fixtures without pretending that absent host
tools are a passing integration.

```sh
cmake -S tests/fixtures/foreign_build_hosts/cmake -B build/foreign-cmake \
  -DCMAKE_TOOLCHAIN_FILE="$PWD/tools/foreign-build-hosts/cmake/JetToolchain.cmake" \
  -DJet_EXECUTABLE="$PWD/target/debug/jet"
cmake --build build/foreign-cmake --target host
build/foreign-cmake/host
cmake --build build/foreign-cmake --target clean
cmake --build build/foreign-cmake --target host

gradle -p tests/fixtures/foreign_build_hosts/gradle clean host
tests/fixtures/foreign_build_hosts/gradle/build/host
gradle -p tests/fixtures/foreign_build_hosts/gradle host

bazel --output_user_root=build/foreign-bazel build //:host
bazel-bin/host
bazel --output_user_root=build/foreign-bazel clean --expunge
bazel --output_user_root=build/foreign-bazel build //:host

msbuild tests/fixtures/foreign_build_hosts/msbuild/host.proj /t:Clean,Build
tests/fixtures/foreign_build_hosts/msbuild/obj/host.exe
tests/fixtures/foreign_build_hosts/msbuild/obj/lifecycle.exe tests/fixtures/foreign_build_hosts/msbuild/obj/libloadable.dll
```

For each host, also change `library.jet`, change the fixture's declared
`extra.jet` input, remove the Jet executable from `PATH`, run two builds
concurrently, cancel one build, and corrupt the requested ABI/kind. The
expected result is a rebuild or a `JET-HOST-TOOL`, `JET-HOST-INPUT`, or
`JET-HOST-ABI` failure with no new stamp. Corrupting a JetText pointer/length
pair or its UTF-8 must fail before dereference, and a missing exported symbol
or target/ABI identity must fail before mapping or publication.
