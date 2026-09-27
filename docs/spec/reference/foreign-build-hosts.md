# Foreign build hosts

Jet can be a library producer inside a repository whose top-level build is
owned by CMake, Make, Gradle, Bazel, or MSBuild. This page is for build-system
maintainers integrating a Jet package; it describes the executable contract
implemented by the shared runner in
[`tools/foreign-build-hosts/jet-library.sh`](../../../tools/foreign-build-hosts/jet-library.sh)
and exercised by [`tests/foreign_build_hosts.rs`](../../../tests/foreign_build_hosts.rs).

## The shared runner

Every adapter delegates the Jet work to one runner. The runner takes a package
root, an entry module, an output name, a destination directory, and optional
extra source paths. It invokes the equivalent of:

```text
jet build --lib --locked --output <output-name> <entry>
```

The adapter must not compile Jet sources itself or reconstruct the package
closure. `--locked` makes the package lock part of the input contract. The
runner stages outputs in a temporary directory, validates the generated
library set, and publishes the directory only after the complete set is ready.

The published directory has these machine-readable markers:

- `jet-host.receipt` records the package, entry, target, and input digest.
- `jet-host.stamp` is written only after the receipt and artifacts pass
  validation.
- the set format is identified by `jet-library-set-v1`.

A consumer can therefore use the stamp as the commit point and the receipt as
provenance. A failed or interrupted build must not leave a directory that looks
complete. Diagnostics use the stable fields `JET-HOST-TOOL`, `JET-HOST-INPUT`,
and `JET-HOST-ABI` so a foreign build can surface the same failure without
parsing prose.

The POSIX adapter uses an explicitly selected `bash` and invokes the Jet
binary supplied by the integration. A final `${CC:-cc}` fallback is only for
the host-side probe performed by the adapter; it is not a way to locate or
compile the Jet library. Windows uses the corresponding PowerShell process and
termination path. Both adapters bound the child process and clean its staging
area on failure.

## CMake

The CMake integration is provided by
[`tools/foreign-build-hosts/cmake/Jet.cmake`](../../../tools/foreign-build-hosts/cmake/Jet.cmake)
and its toolchain module. Configure the package with the toolchain file rather
than relying on a compiler found through `PATH`:

```sh
cmake -S . -B build \
  -DCMAKE_TOOLCHAIN_FILE=/path/to/tools/foreign-build-hosts/cmake/JetToolchain.cmake
cmake --build build --target app_jet
```

The package declares a Jet target with `jet_library`:

```cmake
find_package(Jet REQUIRED)

jet_library(app
  ENTRY src/package.jet
  OUTPUT app
  LIBRARY app
  STATIC
)
```

`jet_library` supports STATIC, SHARED, and LOADABLE outputs. It creates a
foreign-build target named `<target>_jet`; the host project may depend on that
target and consume the generated header or library set. The adapter validates
the selected `CMAKE_C_COMPILER_ID` and rejects an MSVC/GNU archive mismatch
instead of silently producing an incompatible artifact. `CMAKE_TOOLCHAIN_FILE`
is the reproducible configuration boundary; `find_program` is used only to
locate the adapter helper, not to select a hidden Jet host compiler.

## Make and the C/C++ driver

A Make rule can call the C/C++ driver directly. The driver accepts the same
project/build roots and offline mode as the compiler front end:

```make
JET_CC ?= jet-cc
JET_CXX ?= jet-cxx

build/hello.o: src/hello.jet
	$(JET_CC) --offline --project-root . -c $< -o $@

build/hello-cxx.o: src/hello.jet
	$(JET_CXX) --offline --project-root . -c $< -o $@
```

`jet-cc` is an alias for `jet cc`; `jet-cxx` and `jet-c++` are aliases for
`jet c++`. The driver keeps compiler-style options such as `-I`, `-D`, `-L`,
`-l`, `-MMD`, `-MF`, `-MT`, `-std`, `--target`, and `-o`. It recognizes
cross-target selection without inventing a host search path. The tests cover
both Make-style and CMake-style fixtures, including a clean rebuild and a
no-op rebuild after the inputs are unchanged.

`--offline` is an explicit request not to resolve network inputs. A foreign
build should pass it when its lockfile and package cache are already present.
Editing a Jet source, its lockfile, or an input named by the receipt must
invalidate the output; an unchanged input set may remain a no-op. Missing
inputs, an unsupported target, a host ABI mismatch, and a malformed response
file are errors, not permission to fall back to an arbitrary compiler.

## Gradle

The Gradle plugin wraps the same runner and exposes a `jetLibrary` task. Its
inputs include `package.jet`, `.jet/lock`, the entry module, and declared extra
sources. The task publishes the following properties for downstream tasks:

- `artifactDirectory`
- `staticLibrary` or `sharedLibrary`
- `header`
- `receipt`
- `stamp`

A Java or Kotlin project should depend on the task, not duplicate its command:

```groovy
jetLibrary {
    entry = "src/package.jet"
    library = "app"
    kind = "static"
    inputs = ["src/extra.jet"]
}
```

The exact DSL is owned by the plugin version; the properties above are the
artifact boundary. Gradle up-to-date checks must include the receipt inputs so
that changing a Jet source cannot be hidden by a host-only task cache.

## Bazel

The Bazel rule is `jet_library`. It declares the complete Jet source closure
through `deps`, passes arguments through the action API, and exposes the
resulting set through `cc_import`/`cc_library` for the host graph. The adapter
accepts a static library kind; requesting a non-static kind is an
explicit diagnostic rather than a silent downgrade.

A representative rule shape is:

```starlark
jet_library(
    name = "model",
    entry = "src/package.jet",
    deps = ["//src:jet_sources"],
)
```

The rule's action inputs, including the lockfile and all `deps`, are part of
Bazel's action key. The receipt and stamp remain useful when the output is
consumed by a non-Bazel packaging step.

## MSBuild

MSBuild imports `Jet.Library.targets` and invokes the shared runner in the
normal target graph. The integration must declare the Jet entry and output
properties in the project that consumes them, then make native compilation
depend on the generated artifact. A lifecycle project should keep the native
entry point (for example, `lifecycle.cpp`) in the host graph; it must not run
before the Jet target has published its stamp.

The MSBuild path carries the same `RUSTC`, `RUSTC_LINKER`, `CC`, `NO_COLOR`,
and `PATH` environment boundary used by the Windows runner. Those variables
select the explicitly configured process environment; they do not change the
receipt format or allow an undeclared source to enter the build.

## Failure and lifecycle rules

A foreign build should treat these states distinctly:

1. **Input change:** rerun the runner and replace the old set only after the
   new stamp is valid.
2. **No-op:** reuse a set whose receipt still matches every declared input.
3. **Tool failure:** retain diagnostics and remove or quarantine the staging
   directory; never publish a stamp.
4. **Consumer failure:** report the host compiler/linker error separately from
   the Jet receipt so the next invocation can distinguish a bad host link from
   a bad Jet build.

The acceptance matrix in
[`tests/foreign_build_hosts.rs`](../../../tests/foreign_build_hosts.rs) checks
CMake, Gradle, Bazel, and MSBuild terms, the `--locked` invocation, receipt and
stamp publication, `jet-library-set-v1`, `lifecycle.cpp`, and
`CMAKE_TOOLCHAIN_FILE`. It is the executable cross-adapter contract; this page
explains that contract without making a status or availability claim about an
individual host tool installation.
