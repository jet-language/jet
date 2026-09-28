# Library reuse and linking

This reference explains exact-match reuse of compiled dependency work and the
`Library` output boundary. It is for package authors, build engineers, and
people integrating a Jet library into another language. The executable
contracts are the driver implementation in
[`crates/jet-driver/src/LibraryExport.rs`](../../../crates/jet-driver/src/LibraryExport.rs),
the code generator in
[`crates/jet-codegen/src/Codegen/Library.rs`](../../../crates/jet-codegen/src/Codegen/Library.rs),
and the library tests. Ratified decision IDs below are the durable law; build
receipts and diagnostics are the operational proof.

## Vocabulary and scope

- **Artifact** — a file a build produces and a later build can reuse.
- **Sealed package object** — one dependency compiled once, stored under a key,
  and restored instead of recompiled.
- **Artifact identity** — the exact key. It covers package sources, dependency
  artifact digests, compiler identity, target, and profile.
- **ABI** (application binary interface) — the fixed byte layout and calling
  rules separately compiled programs must agree on to link.
- **Public stable ABI** — a promise that this layout never breaks, so a library
  compiled with an old version remains linkable under that stable promise.
- **Version-keyed reuse** — no layout promise. Two artifacts link only when
  their keys match exactly, so layout may change between compiler versions.
- **Static link** — the library's code is copied into the program at build time.
- **Dynamic load** — the program opens the library file while it runs.

## Version-keyed reuse

**D-LIB-REUSE1=B** is the governing rule. Each dependency compiles into an
artifact keyed on exact identity. Later builds restore that artifact. A compiler
identity change invalidates the key and causes the dependency to rebuild; no
cache path skips parsing, semantic checking, policy, or diagnostics. Generic
function bodies may travel inside the typed artifact and instantiate at the use
site, so generic use does not require distributing source.

A Jet program may also load a Jet library at run time. Both sides carry compiler
identity and the relevant ABI version. A mismatch is a checked error before the
file is mapped, never an unchecked layout assumption or a crash.

This intentionally differs from a public stable ABI:

| System | Compatibility promise | Jet's boundary |
| --- | --- | --- |
| Rust | No stable layout promise for arbitrary crate internals. | Reuse exact artifacts instead of requiring a permanent ABI. |
| Swift | Public stable ABI for its declared compatibility boundary. | No permanent Jet layout promise without a new owner decision. |
| Nix | Content identity determines whether an object can be reused. | Apply the same identity principle to sealed package objects. |

The accepted cost is explicit: an artifact does not survive a compiler identity
change. It is rebuilt or fetched again. Correctness never depends on layout
luck.

## Cache tiers and invalidation

Sealed objects use the existing cache tiers rather than introducing a second
reuse protocol:

- **Local** — the machine's own store; the first build fills it and the next
  matching build restores it.
- **Shared** — a configured store whose writer authority and namespace policy
  still apply.
- **Remote** — a content-addressed mirror; a hit is usable only when its
  identity and receipt match the requested object.

`D-JPK-CACHEAUTH1=D` remains the writer-authority rule: shared namespaces accept
only objects with the required provenance. `D-JPK-REMOTE1=C` remains the
remote-read rule: a remote object is a candidate, not a reason to skip local
validation. `D-JPK-STORE1=A` keeps store identity separate from package source
identity.

A restored package object is a link-and-restore layer. It complements module
semantic dirty sets: restoring an unchanged dependency never skips checking the
package being edited. The same identity and restore path serve `jet build`,
`jet run`, and `jet dev`; the lens changes how the current package is compiled,
not which dependency object is trusted.

## The `Library` output boundary

`Library` is the single output kind for the three audiences below. Its fields
select the artifacts a build emits (**D-LIB-NAME1=A**); a new output kind is not
needed.

### Jet calls Jet

Sealed package objects are linked statically. There is no public ABI and no
foreign export surface. This is the **D-LIB-REUSE1=B** case.

### Another language calls Jet

Under **D-LIB-EXPORT1=C**, a `Library` build can produce a native static or
shared library, a C header, and generated bindings for each language named by
the package. The entry module's `pub` items form the exported surface. The
API-freeze check is the single source for the header, bindings, and version
check. Supported binding names are selected by the library backend rather than
invented by a host build.

A package declaration has the following shape (the exact output path is chosen
by the build):

```jet
# package.jet
name: "flightlog"
outputs: {
    core: .Library{
        native: true,
        entry: Flightlog,
        bindings: [c, python, swift],
    },
}
```

The generated native boundary owns its ownership rules. An exported surface
states who frees a returned buffer and what a foreign caller may hold across
calls; semantic checking rejects an incompatible boundary. A C calling
convention at this edge does not make Jet's internal layout stable.

### A Jet program loads Jet at run time

Under **D-LIB-DYNTRUST1=A**, a loadable `Library` produces a `.jetlib` file
pinned to one compiler identity. Its header records the host-native target, ABI
version, library name, checked top-level `pub fn` export table, and payload
length. The loaded library declares its effects like any package; the host
states a grant at the load site. A library asking for more is refused before it
is mapped.

The canonical Jet-side loader is:

```jet
use core.mod as library

loaded :: library.load(
    ".jet/build/loadable.jetlib",
    grant: { read: [".jet/build"] },
)
```

The loader verifies declared symbols, maps through the platform bridge, and
releases the handle before deleting staged payload. JIT and interpreter
teardown release their load tables. The grant is explicit: loading a package
does not silently grant arbitrary filesystem or process access.

## ABI stance

Jet makes **no public stable ABI promise** by ratified decision. Reuse is
exact-match only. The native export under **D-LIB-EXPORT1=C** uses the C calling
convention at the edge, but promises nothing about Jet's internal type layout
across compiler versions.

Enforce these consequences:

- A compiled Jet artifact records compiler identity. A mismatch is refused with
  a registered diagnostic before anything is linked or mapped.
- A closed-source Jet package cannot be treated as a binary that outlives the
  compiler identity recorded in it.
- A request for a stable Jet ABI needs a fresh owner decision and an explicit
  compatibility baseline under greenfield law.

## Beginner and expert paths

**Beginner:** the first build compiles and the next matching build restores
work. A compiler identity change rebuilds dependencies. No cache flag or clean
step is required, and an untrusted artifact is rebuilt rather than offered with
a warning.

**Expert:** reuse remains inspectable and controllable:

- `jet cache bind` sets mirror order, roles, and credential providers.
- `jet build` prints the namespace of each cache write.
- `jet explain --lens cache` explains why a hit was trusted or refused.
- `jet prove --lens reproducibility` names the first differing path between two
  builders.
- `--offline` takes precedence over mirrors.
- Exporting a library is opt-in and declared in the package, never an accidental
  side effect of building.
- The load site names its grant, so the host code shows what the library may do.

## Ratified answers

| Decision | Outcome |
|---|---|
| **D-LIB-REUSE1=B** | Sealed package objects, plus pinned Jet libraries loaded at run time; no public stable ABI. |
| **D-LIB-EXPORT1=C** | `Library` emits native static/shared output, a C header, and generated bindings for each named language. |
| **D-LIB-DYNTRUST1=A** | A loaded library declares effects; the host grants a set at the load site; anything more is refused before mapping. |
| **D-LIB-NAME1=A** | `Library` is the ratified output field, not a new output kind; loadable files use `.jetlib`. |
| **D-LIB-CALLGRANT1=A** | `library.load("./mods/f16.jetlib", grant: { read: ["./mods"] })?` keeps the grant at the load site; loaded exports are typed members such as `library.on_tick(dt)`. |

The implementation and its tests are authoritative for artifact names and
backend-specific files. This page defines the compatibility and trust rules,
not a promise that an arbitrary host toolchain can consume every binding.
