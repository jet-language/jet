# Precompiled libraries: a checked interface, shipped code, and reach-only builds

Status: proposal for owner review (card #5910, 2026-11-05). Nothing here is
ratified by this document. Facts marked **today** come from dev `5f0e5f8d87`
and the admitted compiler `66c2a661a6`. Measurements ran on the team machine
(32 cores) with no other job running unless stated.

It builds on the owner ruling logged on #5910:

1. No incremental compilation.
2. `jet run`, `jet dev` and `jet test` link Core and the runtime as
   precompiled toolchain pieces. They are built once with the compiler and
   named by the compiler identity. They are not a cache.
3. The precompiled piece includes Core's checked interface. Signatures and
   types load without parsing or checking Core again. Non-generic code ships
   as machine code. Generic bodies ship as checked code and are specialized
   per program.
4. `jet build` compiles from the entry points outward and includes only
   reached code. It keeps trait-object methods and runtime roots. `jet dev`
   does no pruning.
5. I9: every path compiles the same Core source, and the tier goldens agree.

Two choices are left to the owner. They are drafted as ballots and listed in
section 9.

## 1. Summary

Every Jet compile today parses and checks Core from source. A program that
uses `core.files` spends about 0.7 s and 550 MiB doing that before it
reaches its own code. Its own code then takes about 30 ms. The test suites
compile thousands of small programs, so this one cost is most of their time.

The fix is a **library pack**, which the toolchain builds once from Core's
source in the same step that builds the runtime pack. It has three parts:

- the **checked interface**: every public name, type, signature and
  checked fact that a caller needs (effects, access marks, failure types,
  contracts, unsafe audit facts, trait impls);
- **checked bodies**: every Core function body after checking, in the form
  the code generator reads. Generic bodies keep their type parameters open;
- **machine code**: every non-generic Core function, compiled for each
  supported target and added to the runtime pack's image and object.

The first two parts are one file in the compiler's existing record format.
The compiler maps it into memory and decodes only the items its program
names. Nothing is parsed or checked again.

Then each command does only the user's work:

| Command | Reads from the pack | Compiles |
| --- | --- | --- |
| `jet check` | interface | the user's modules |
| `jet run`, `jet test`, comptime | interface, generic bodies, machine code | user modules and their generic instances |
| `jet dev` | the same as `run`, with all Core code kept | the same, again on each edit |
| `jet build` | interface and checked bodies | reached user and Core code, at the build's level |
| language server | interface | the open documents |

Expected effect: a Core-using test compile drops from about 0.8 s and
700 MiB to under 0.1 s and under 100 MiB. Section 7 has the numbers.

## 2. Today

### 2.1 What a compile spends

**Today**, one `jet run` of a five-line program that uses `core.files`:

| Phase | Wall | Resident after |
| --- | --- | --- |
| load (parse Core and the program) | 300 ms | 501 MiB |
| register (names, types, impls) | 160 ms | 516 MiB |
| comptime (117 Core comptime applications) | 50 ms | 539 MiB |
| finalize (check bodies, effects, ownership) | 200 ms | 694 MiB |
| lower, emit, link | 30 ms | 694 MiB |

The program's own object is 3,424 bytes. All but about 30 ms of the
0.84 s is Core.

Per Core module, check and run of a program that imports only that module:

| Import | `jet check` | `jet run` |
| --- | --- | --- |
| none (hello) | 0.04 s, 23 MiB | 0.03 s, 36 MiB |
| `core.math` | 0.16 s, 95 MiB | 0.17 s, 122 MiB |
| `core.text` | 0.47 s, 487 MiB | 0.47 s, 500 MiB |
| `core.files`, `process`, `sys`, `time`, `crypto`, `encoding.json`, `testing`, `db`, `service` | 0.67-0.80 s, 545-572 MiB | 0.79-0.91 s, 696-745 MiB |

About 270 MiB of each heavy compile is two table literals, the Unicode name
index and the collation table (#5910 c56).

### 2.2 What is precompiled already

D-EXEC1 says the Prelude and Core are "compiled once per Jet version and
shipped with Jet". **Today** that holds for the runtime layer only. The
runtime pack is built with the compiler and named by a runtime key. It holds
`jet_runtime_jet.o` (10 MB), the startup pack object, a 3 MB in-memory image
for `jet run`, a wasm build for the web target, and an ABI and export list.
Its provenance lists 340 source files, all under `Compiler/JetRuntime` and
`Compiler/JetBackend`. None is under `Core/`.

Core itself (about 170 files) is compiled from source in every program:
parsed, registered, checked, and then the reached functions are lowered into
the program's own object. So the machine-code half of D-EXEC1 exists for the
runtime layer. For Core, neither the checked interface nor the code exists.

### 2.3 What `jet build` does today

`jet build` already links only reached code: the `cli_args` example links to
a 78 KB executable, against the 3 MB runtime image. It still checks all of
Core first, as above.

## 3. Prior art

Each entry: the mechanism, what it does well, its known pain, and what Jet
takes from it.

**Rust: `.rmeta` and `.rlib`.** A crate compiles to metadata (types,
signatures, trait impls, and MIR for generic and `#[inline]` functions) plus
object code. Pipelined builds start a dependent crate as soon as the
metadata exists. Strength: callers never reparse a dependency; generics
instantiate from MIR. Pain: metadata is tied to one exact compiler build.
Each crate instantiates the same generics again, so codegen repeats. The
shipped `std` is fixed at one optimization level and one set of flags, and
changing those needs a rebuild of `std`. Jet takes: interface plus generic
bodies plus code in one artifact, tied to the compiler identity.

**Go: export data, unified IR, and the build cache.** The compiler writes
each package's exported declarations, inlinable bodies and generic bodies
into an indexed binary format inside the package archive. Importers read only
the entries they use. The build cache keys every step by a hash of its
inputs and the toolchain. Strength: fast, deterministic, simple to reason
about; indexed export data is read lazily. Pain: the cache grows until
trimmed; generics use shape stenciling with run-time dictionaries, which
costs speed. Jet takes: indexed, lazily read export data; exact
content identity.

**Zig: lazy analysis.** Only declarations reached from the roots are
analyzed. Strength: unused code costs nothing, and the standard library
compiles from source quickly. Pain: an error in code nobody calls is never
reported, so library authors write tests just to reach their own code. Jet
takes: reach-only code generation in `jet build`. Jet refuses the pain:
reachability decides what is compiled to machine code, never what is
checked.

**Swift: `.swiftmodule` and `.swiftinterface`.** The binary module is a
serialized checked AST, tied to one compiler version. The textual interface
is stable across versions, but importing it means parsing and checking it
again, so Apple ships a prebuilt module cache. Library evolution adds
resilience: non-frozen types are reached indirectly, and only `@inlinable`
code can inline. Pain: slow textual rebuilds, cache mismatches, and the
run-time cost of resilience. Jet takes the lesson, not the mechanism: the
pack is never a stable cross-version format, so there is no resilience tax
and no textual second form.

**OCaml: `.cmi` and `.cmx`.** The compiled interface holds the types of a
module's signature. The `.cmx` file holds cross-module inlining facts.
Digests catch mismatches ("inconsistent assumptions over interface").
Strength: small, exact interfaces. Pain: a change to an implementation's
inlining facts forces dependents to recompile, unless `-opaque` turns
inlining off. Jet takes: digest-checked interfaces. Jet avoids the
recompile pain because `jet build` compiles reached Core from checked bodies
every time, so there is no stale inlining fact to track.

**Haskell: `.hi` interface files.** Interfaces carry types, unfoldings
(inlinable bodies) and per-declaration fingerprints, so GHC recompiles only
what an interface change touches. Pain: optimization changes the interface,
and interface files bloat with unfoldings. Jet takes: per-item hashes as
item identity (useful for provenance and the language server). Jet needs no
recompilation avoidance, because the pack only changes with the toolchain.

**C++20 module BMIs.** A compiled module interface avoids reparsing headers.
Pain: each compiler has its own format; a BMI is valid only for the same
compiler, version and flags; build systems must scan for module
dependencies before compiling anything. Jet takes the warning: the pack must
not depend on any user flag. Only the target may vary it, and Jet ships one
pack per target.

**Lean 4: `.olean`.** An `.olean` is a compacted object graph. Lean maps it
into memory and uses it in place, so importing a huge library takes
seconds. Pain: the format is tied to the exact Lean build, and mapped data
cannot be freed. Jet takes: map the file and pay only for what is touched.
Jet decodes one item at a time from its existing record format instead of
adopting Lean's raw object layout, so it keeps one wire format.

**Unison: content-addressed definitions.** Each definition is named by a
hash of its checked syntax tree. Names are metadata. A definition is checked
once, ever. Pain: the code base is not plain text files. Jet takes: items in
the pack carry a stable content hash. Jet keeps plain source files.

**C# reference assemblies.** A metadata-only assembly holds the public API
with bodies removed. Builds compile against it, and its bytes change only
when the API changes. Compilation is deterministic by default. Jet takes:
deterministic, API-only interface sections.

**Dart kernel `.dill`.** The front end writes a checked program as a binary
kernel AST. The SDK ships its core libraries as a prebuilt platform `.dill`,
and AOT builds shake the tree from it. Jet takes: shipping the standard
library checked, and reach-only builds from the checked form.

**Kotlin `klib`.** A library ships as serialized IR plus metadata. A
native binary compiles all IR whole-program at the final link. Debug builds
link prebuilt native caches of the standard library to skip that work. Pain:
slow release links and complex cache setup. Jet takes: exactly this split,
precompiled code for development commands and whole-program compilation
for the release build, with no user-visible cache setup.

**Also studied.** Java class data sharing maps preparsed class metadata at
startup, the same idea as Lean's mapped files. GraalVM native image does
closed-world reachability; its pain is that reflection must be listed by
hand. Jet has no open reflection, so every root is known to the compiler.
Oberon symbol files were the first compact checked interfaces with
fingerprints; Jet's interface section is their direct descendant.

### 3.1 The combination

| Need | Taken from | Jet adaptation |
| --- | --- | --- |
| Never recheck a library | Rust, Go, Swift, Dart | the checked interface |
| Load in place, pay for what is used | Lean, Java CDS, Go | mapped, indexed sections |
| Generics per program | Rust MIR, Go | checked generic bodies |
| Fast development commands | Kotlin caches, Rust `std` | shipped machine code |
| Small release binaries | Zig, Dart, GraalVM | reach-only `jet build` |
| No stale results | Go, Haskell, OCaml | one pack per compiler identity and target |
| Check all code anyway | (Zig's gap) | the whole library is checked when the pack is built |
| No flag mismatch | (C++ BMI pain) | the pack depends on the target only |
| No cross-version format | (Swift pain) | never shipped apart from its compiler |

## 4. The Jet design

### 4.0 What it reuses

Most of the pieces exist on paper or in code. This design reuses them and
drops what the no-incremental ruling retires.

- **The interface record.** The incremental-checking research note
  (`Docs/research/incremental-checking-design-2026-09-30.md`, section 3.2)
  already specifies `jet.iface/v1`: erased declarations, the trait and impl
  table, constants, effect summaries, failure sets, ownership and memory
  summaries, target facts and templates. The pack's interface sections are
  that record. Its section 5 lists the sema changes needed to check against
  it: registration accepting modules rebuilt from the record, body walks
  limited to the program's own modules, and sealed effect nodes for library
  functions. Those carry over.
- **The one binary record format.** `JetFoundation/Source/Record/RecordCodec.jet`
  is the compiler's single record format (#2517): canonical, sectioned,
  checksummed, with a per-section element index. The pack uses it. It adds
  no second wire format.
- **The runtime pack.** It is already built once per compiler, named by a
  key, digested in its provenance and published with the toolchain. The
  library pack is built in the same step and published beside it.
- **Package keys.** `jet_driver_package_check_key` in the driver computes a
  key from the compiler identity, target facts, source digest and the
  dependencies' interface digests. Nothing calls it today. If the owner
  chooses package packs (section 9), it names them once it also covers the
  build facts the package reads.

What the ruling retires from that note: red/green package reuse, item
memos, usages, priors and the shared record store. No pack is ever looked up
by a changing key during a compile. There is one pack per toolchain and
target, and it is either present and matching or the compile stops.

### 4.1 The library pack

The toolchain builds one library pack per target, in the same step and the
same directory as the runtime pack, under the same key and provenance. This
is I8: one mechanism for precompiled toolchain pieces, not a second one
beside the runtime pack.

The pack is one record in the record format. Its sections:

1. **Header**: schema, compiler identity, target, and the digest of the
   Core source it was built from.
2. **Modules**: module path to item numbers, sorted for binary search.
3. **Interface**: the `jet.iface/v1` sections, one element per item. A
   signature carries its parameters with their access marks (`&`, `^`,
   views and zones), result and failure types, effect row, comptime
   callability and unsafe audit facts.
4. **Contracts**: `#Pre` and `#Post` clauses as checked expressions, with
   their proof status.
5. **Compile-time values**: every Core constant already evaluated, and the
   binary assets (Unicode names, collation) as raw bytes.
6. **Checked bodies**: every function body after checking, in the form
   lowering reads today, one element per function. Generic bodies keep
   their type parameters open. This is the note's "templates" section,
   widened from inlinable and generic bodies to all bodies, because
   `jet build` compiles Core from them.
7. **Locations**: declaration spans, used only to render diagnostics.

The machine code is not in the record. Every non-generic Core function is
compiled into the runtime pack's own outputs: its object, its in-memory
image for `jet run`, and its wasm build. They carry symbols, DWARF line
tables and fault-report rows, as the runtime code does today. So `jet run`
still maps one image, now holding the runtime and Core.

### 4.2 How a compile uses it

The compiler maps the pack at startup and reads its header and section
indexes. That cost does not grow with what the program uses.

Each section has an element index, so the reader decodes one element
without touching the others. When the user's code says
`use files :: core.files` and calls `files.read(path)`, name lookup reads
the module table, then that one item, its signature and the types it names.
Sema builds its usual records for those items only, on first use. A program
that uses ten Core functions decodes about ten items.

The whole-record checksum of the toolchain's own pack is checked when the
toolchain is built and published, and the provenance carries the pack's
digest. That pack ships inside the toolchain, with the same trust as the
compiler binary, so a compile does not rehash it. This holds for the
toolchain pack only. A pack read from any store (a package pack, section 9)
is verified on every hit, as CACHEAUTH1=D requires, and a publisher's
signature proves only who built it, never that its interface is true.

Sema checks user code against those records exactly as it checks user code
against Core today. The facts are the same facts: they were computed by the
same checker, from the same Core source, when the pack was built.

### 4.3 Generics

A generic Core function ships as its checked body with open type parameters.
When a program uses `List<Point>.sort()`, the compiler substitutes `Point`
and runs only the work that depends on the type argument: trait method
selection, layout, and the specialized lowering. It never parses or resolves
names in Core again. The specialized code goes into the program's own
object.

Two programs that both use `List<Int>.sort()` each compile it. That is a
small repeat, and it keeps the rule simple: the pack holds nothing that
depends on a user program.

A generic body that only uses types the pack already knows, such as
`List<Int>` inside Core itself, is compiled once into the pack's code like
any non-generic function.

### 4.4 Machine code for `run`, `dev`, `test` and comptime

These commands link the pack's machine code beside the runtime pack's code.
User code calls a Core function by its symbol. The program's own object
holds only user functions and generic instances.

Comptime uses the same code. A comptime batch is compiled at O0 and bound in
memory to the runtime image and the Core code, then run in the compiler's
child process, as today. So a Core function gives the same answer at
compile time and at run time, by construction.

Comptime always runs on the host. When the build's target is not the host,
comptime links the host pack's code, never the target's, and that code is
given the target's layout and build facts as values. Target code is never
run on the host.

The shipped code is built at the level the runtime pack's code uses. D-EXEC1
already ratifies one shipped Core, compiled once per Jet version, so this is
design and needs no ballot. I9 makes the level invisible in output: every
tier must print the same thing.

### 4.5 Reach-only `jet build`

`jet build` reads the interface and the checked bodies. It starts at the
roots and compiles each reached function, user or Core, at the build's own
level, so Core code is inlined and optimized together with the program. It
never uses the pack's machine code. Unreached Core code is never compiled.

The roots are:

- the program's entry point, and its tests when building tests;
- the runtime roots in the runtime's lifecycle list, startup code and panic
  handling;
- every exported function: C exports, plugin exports and service handlers;
- every function a reached compile-time value names, such as a function
  stored in a constant;
- the drop and `Close` code of every reached type.

Trait objects are exact. When reached code turns a value of type `T` into a
trait object of trait `Tr`, the compiler builds the method table for `T` and
`Tr`, and all its methods become reached. It never keeps every impl of `Tr`.
A method no reached conversion needs is not compiled.

`jet dev` does no pruning. It links all of the pack's code, so an edit can
call any Core function without a relink of Core.

## 5. How the design answers each need

**Effects and effect rows.** Each signature stores its declared and
inferred effect row. A user call site unions the callee's row into the
caller's row from the interface, the same as today. Effect policy, such as
a scope that denies file access, is checked at the user's call sites, so it
needs only the rows.

**Ownership and access marks.** Parameters store their access marks (`&`,
`^`), view lifetimes and zones. Results store which parameter a returned
view borrows from. The borrow checker of the user's body reads these
summaries. It never needed Core's bodies for this.

**`#Unsafe` audit facts.** Each Core function records whether it holds
audited unsafe code and the audit reason. Any report of the unsafe code a
program reaches reads Core's part from the pack, without Core source.

**Comptime.** Core constants are evaluated once, when the pack is built, and
stored as values. Core functions that user comptime code calls run from the
pack's machine code in the compiler's child, as in 4.4. Today each compile
repeats 117 Core comptime applications; that work disappears.

**I9: one meaning on every tier.** There is one Core source and one checker.
The pack is built by checking that source once. `run`, `dev`, `test` and
comptime run code compiled from those checked bodies. `jet build` compiles
the same checked bodies at its own level. Web builds use the same checked
bodies with the wasm code section. The tier goldens prove it on every pack:
the same example must print the same output at O0, O1, O2 and web.

**Generics per program.** As in 4.3.

**`jet prove` claims and contracts.** Contracts are stored as checked
expressions with their proof status. The prover uses a Core function's
`#Post` as an assumption at a user call site, and checks the user's
argument against its `#Pre`. Core's own contracts are proved or tested when
the pack is built, so a user proof never reopens them.

**Diagnostics registry and policy keys.** The pack build checks all of Core
and must finish with zero diagnostics; any Core error or warning fails the
pack build. Diagnostics in user code that name Core items take the item's
name and span from the pack, so messages and `--> Core/…` locations read the
same as today. User policy settings apply to user code only. They cannot
change how Core was checked.

**Trait contracts and the orphan rule.** Every impl records its owning
package. The orphan rule ("an impl needs a local trait or a local self
type") reads trait and type ownership from the pack, so a user impl of a
Core trait for a Core type is still refused with E0902. Trait contracts are
stored with the trait.

**`jet dev` hot reload.** The dev session maps the pack once. An edit
recompiles only the user's program. The swap decision compares user
functions only; Core is never part of a swap.

**Language server.** The server maps the same pack. Hover, completion and
signature help on Core items read the interface. Go-to-definition opens the
Core source shipped with the toolchain at the stored span. Opening a file no
longer checks Core, so the first answer arrives in milliseconds.

**Determinism and reproducible builds.** The pack's bytes are a pure
function of the compiler, the Core source and the target. Sections are
written in a fixed order with sorted tables and no time stamps. The
toolchain's fixed-point check compares the pack between stages, like the
runtime pack.

**Signing.** Jet signs no artifact today. The pack is part of the toolchain,
so whatever signs or digests a toolchain release covers it. A program's
provenance records the pack's digest beside the runtime key.

**Cross-compiling targets.** Core reads one build fact today,
`$build.os`, in one function. So checked Core can differ by target, and Jet
ships one pack per supported target: x86-64 Linux and web today. The pack
build refuses any Core read of a per-program fact (profile, package, stamp).
Then a pack can never depend on a user's flags, the failure C++ module files
have.

**Debug info.** The code section carries DWARF line tables and fault-report
rows, with Core source paths relative to the toolchain's Core directory. A
crash inside Core shows the same `Core/…:line` frames as today, in any
install directory.

**Reachability and binary size.** As in 4.5. Release binaries contain only
reached code and exact trait-object methods. Development binaries are larger
because they link all Core code; that is the cost of not pruning in `dev`.

**Jetpack packages.** Whether installed packages get the same treatment is
an owner choice (ballot `D-PACKAGE-PACKS1`, section 9). The mechanism is the
same either way: a pack is built from checked sources and named by content.
Only when and where it is built changes, and what its name must cover.

**Working on Core itself.** A Jet contributor who edits Core needs a new
pack. The compiler compares the header's Core source digest with the Core
source beside the toolchain. If they differ, it stops with an error that
names the internal rebuild step. It never compiles Core from source as a
fallback, because that would be a second path to the same meaning.

## 6. What it replaces

- Parsing, registering, comptime-evaluating and checking every Core module
  of the import closure in each compile.
- Lowering reached Core functions into every `run`, `dev` and `test`
  program. These link the pack's code instead.
- The demand-driven Core body checking that exists only to make the
  per-program Core check cheaper. All Core bodies are checked when the pack
  is built.
- Core table literals that are parsed as source in every compile. They
  become raw bytes in the pack (#5910 c56).
- The language server's check of Core on each open.
- The incremental-checking plan's cache parts (red/green reuse, item
  memos, usages, the shared record store). The request's `record_store`
  field and its load and publish helpers are always empty today. They go,
  unless the owner chooses package packs, which use them for the package
  store.

`jet build` keeps its code generator and linker. It loses the Core check
and gains the checked bodies as input.

## 7. Expected numbers

The pack costs a few system calls to map. A Core-using compile then pays
for the user's code, the items it names and its generic instances.

| Program | Today | Expected |
| --- | --- | --- |
| `jet check`, imports `core.math` | 0.16 s, 95 MiB | under 0.05 s, under 50 MiB |
| `jet check`, imports `core.files` | 0.7 s, 550 MiB | under 0.05 s, under 60 MiB |
| `jet run`, imports `core.files` | 0.84 s, 700 MiB | under 0.1 s, under 100 MiB |
| golden entry, 3 tiers | about 2.7 s of compiles | under 0.4 s |

The `jet build` levels still compile reached Core code at O1 and O2, so they
gain less than `run`: they skip the 0.7 s check but keep their code
generation. The Rust-era golden harness ran about 0.26 s per example with
its compiler built once (2026-08-13: 133 s for about 504 examples). With the
pack, a Jet golden entry costs about 0.1 s per tier.

The pack adds to the toolchain build and install. A first estimate is tens of
megabytes per target and well under a minute of build time; the first build
measures both.

## 8. Building it

The work splits at the Core boundary of the driver. **Today** Core and user
modules go into one registration graph that holds full syntax trees and
indexes. The steps:

1. Table literals become binary assets (#5910 c56). This is independent and
   lands first.
2. Check Core alone into its own graph, with no user module and no
   per-program fact. Refuse any Core read of a per-program fact.
3. Write the pack from that graph. Read it back through typed accessors that
   build sema's records on first lookup. Generate the writer and reader from
   the sema record declarations, so a new field cannot be forgotten.
4. Switch `check`, `run`, `dev`, `test`, comptime and the language server
   to the pack, all at once. Remove the per-program Core check.
5. Add the code sections and link them in `run`, `dev`, `test` and comptime.
6. Switch `jet build` to compile from the checked bodies, from the roots.

Proof at each step: every golden and ui row gives the same result before and
after (lost = 0); the pack round-trips (written, read back and compared item
by item); two pack builds give identical bytes; the timing tables above are
measured, not estimated.

## 9. Owner choices

One choice changes what users see. It has a ballot draft.

Two earlier answers meet here. D-LIB-REUSE1=B ratified sealed package
objects, keyed by exact identity and restored by later builds. The
2026-10-30 answer on #2517 then said there is no incremental compilation at
all, and it named package-level reuse. So package packs are not settled by
either answer alone.

- **`D-PACKAGE-PACKS1`: whether installed package versions may be a narrow
  exception.** An installed version is immutable, like Core. The ballot asks
  whether such a version may be checked once and its pack restored, and
  whether a pack may come from a trusted store instead of a local build.
  Packages being edited stay outside any exception and compile fresh.
- **What a package pack's name must cover.** Core reads one build fact, the
  target operating system. A package can read much more, and its pack's name
  must cover everything the pack's content depends on, on top of the
  compiler identity, target, source digest and dependency interface digests
  that `jet_driver_package_check_key` hashes today:
  - every build fact the package reads: `$build.profile`, `$build.settings.*`
    and `$package.*`;
  - the hashes of generated and embedded assets (today the identity's source
    digest covers only the loaded Jet text and the manifest, while
    ComptimeInputs records asset hashes apart);
  - the body or artifact digests of dependencies wherever generics, inlining
    or comptime read a dependency's bodies, not only its interface;
  - foreign sources, the foreign toolchain and the link inputs;
  - the CPU, ABI, target features and code-generation flags.

  A pack whose content reads an input outside its name is refused at build
  time.
- **The ballot's recommendation.** It recommends verified installed packs
  (option A): restored only after verification on every hit. Option D keeps
  reuse local-only. Either one is an explicit new exception to the
  2026-10-30 answer, not something already authorized.

Everything else in this note is design: the pack layout, the reader, the
level of the shipped code, the roots of reachability and the rebuild check
for Core contributors.

## 10. Risks

- **The split at the Core boundary is the largest change.** Sema today
  assumes one graph with every module's syntax tree. Lookups that reach into
  Core syntax must move to accessors. A census of those reads comes before
  step 2.
- **Generic bodies may need more per-instance checking than expected.**
  If an instance needs work beyond substitution, that work is measured per
  instance and kept small; it never reopens Core's source.
- **A larger install.** Measured in the first build of step 3.
