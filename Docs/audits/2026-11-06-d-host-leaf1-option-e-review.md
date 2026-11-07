# D-HOST-LEAF1 option E: adversarial review

Date: 2026-11-06. Author model family: Claude. Adversarial model family:
Claude (Main, reviewing by owner direction). Subject: option E of
D-HOST-LEAF1, which the owner ratified the same day: the runtime package
`jet_runtime` becomes an ordinary library that Core and any package use with
`use rt: jet.runtime.time` and `rt.sleep_ns(duration.ns)`. The review of
options A to D is `2026-11-06-d-host-leaf1-review.md`.

Facts checked against the dev tree at bee9c15480:

- `Compiler/JetRuntime/package.jet` declares the package `jet_runtime` with
  one native library output. No runtime source imports `core` or `jet`, so
  Core can sit above the runtime without a cycle.
- The runtime is 311 source files and 149,564 lines.
- Today a Core call such as `core.time.__sleep_ns` is bound by three
  compiler tables: the `dispatcher_row` lines in `Compiler/Prelude/Core.jet`
  (line 1286 for this call), the `CoreCallRecord` rows in
  `Compiler/JetFoundation/Source/Registry/CoreCallRows.jet` (line 1025), and
  the `IRRoute` carrier rows in `Compiler/JetIR/Source/RuntimeRoutes.jet`
  (line 710). `RuntimeStopExports.jet` lists the exported symbols again.
- The runtime body is `#Export(c) jet_std_time_sleep_duration_ns: fn(nanos:
  I64)` in `HostTime.jet:214`. It carries no effect row. The effect leaf
  `Time.Wait` lives only in the `CoreCallRecord` (`effect_leaf`).
- The web tier implements the same symbol in JavaScript
  (`Compiler/Prelude/Web/Core/Realtime.js:18`), one of 31 host files.
- `pub` is Jet's visibility marker (spec.md, "Struct and enum bodies" and
  "Imports and visibility"). A dependency package is checked as its own
  package, from source.

## Strongest existing solutions

- **Go.** `runtime` is an ordinary package written mostly in Go, imported
  like any other. Privileged access goes through `internal/` packages and
  `//go:linkname`. Go 1.23 restricted "pull" linkname use because packages
  had reached runtime internals through it (golang/go#67401). Best case:
  there is no boundary, and the runtime is just Go.
- **Rust.** `std` is built from ordinary crates (`core`, `alloc`) over
  `libc`. The compiler knows a small, attribute-marked set of lang items and
  intrinsics. Best case: everything except a few lang items is an ordinary
  crate with checked signatures.
- **Swift.** The standard library binds the C++ runtime with
  `@_silgen_name`, which nothing checks. That is the pattern E avoids.
- **Zig.** `std` calls the OS directly, and compiler builtins are a fixed
  `@` set.

E follows the Go and Rust model. What they do that E must match: the runtime
interface is compiled once and read by every importer, not re-checked from
source on each build (Rust `rlib` metadata, Go export data).

## Claims and attacks

### 1. No markers, symbol strings or manifest; nothing can drift

Attack: the binding today is three tables, not one. If any survives, drift
survives. A precompiled interface is also a manifest under another name.

Check: the three tables and the export list above. Each `CoreCallRecord` row
also carries metadata beyond the signature: `borrow_mask`, `fallibility`,
`effect_kind`, `effect_leaf`, a capability contract (`NativeRequired`, a
provider and the failure `core.capability.unavailable`), `pure_route`,
`coverage`, `aot_direct`, `native_int_result`, `frame_accesses`,
`sink_class`, `ui_capabilities`, `marker` and `authority`.

Verdict: **holds only within stated limits.** It holds for Jet callers only
if all three tables and the export list are deleted, and every metadata
field either gets a home in the runtime's Jet signature or is proven unused
and deleted. Parameter borrows, result types and `Int` results have obvious
homes. The capability contract, `pure_route`, `coverage`, `aot_direct` and
`frame_accesses` need a census. The interface the compiler reads must be
generated from the runtime source when the runtime is built, never written
by hand. "No manifest" therefore means "no hand-written manifest".

### 2. Real Jet signatures and effects; effects come from the runtime's own code

Attack: runtime bodies are low-level code over `I64` and the scheduler. They
do not perform effects the checker can infer. A `Time.Wait` leaf inferred
from `jet_task_sleep_duration_ns_defaulted` would be empty, or `FFI`, not
`Time.Wait`.

Check: `HostTime.jet:214` has no effect row, and the leaf exists only in the
compiler row.

Verdict: **fails as stated.** Redesign: a declared effect row on a `pub`
function of `jet_runtime` introduces those effects. This is the vetted
standard-library trust point that I1 already allows, and it is written next
to the code. The cutover must prove that every declared row equals today's
`effect_kind` and `effect_leaf` for that function, by a generated
comparison over all bound functions. Re-review of the redesign: callers
cannot drop or widen the effects, because the row is part of the signature
the compiler reads. The runtime author can still declare a wrong row, as a
compiler row can be wrong today. The comparison catches every row at
cutover, and review of the runtime package catches later ones. With this
amendment the claim **holds**.

### 3. Internals stay hidden by ordinary visibility

Attack: the runtime exports 3,216 `#Export(c)` functions, and any of them is
linkable.

Check: `pub` exists. C symbols are reachable from Jet only through
`#Import(c)`, which safety.md makes a gated, unsafe-only region.

Verdict: **holds.** Safe code sees only `pub` runtime functions. Linking a
C symbol directly requires an audited `#Unsafe` block, which is the
expert path.

### 4. Every tier runs the same code

Attack: the web tier provides runtime symbols from JavaScript.

Check: `Realtime.js:18` reimplements `jet_std_time_sleep_duration_ns`.

Verdict: **fails on web today.** This is a pre-existing I9 defect that E
neither causes nor fixes. E must not be described as giving every tier one
runtime until the web tier runs `jet_runtime` compiled for the web or the
JavaScript host shrinks to marshalling only.

### 5. The cost is about 575 renames

Attack: the stated cost omits the interface and the metadata.

Check: a source dependency of 149,564 lines, re-checked on every build,
would add a large cost to every compile.

Verdict: **fails as stated; holds once amended.** The full cost is:

1. Renaming the bound runtime functions to Jet names and types.
2. Generating the runtime interface with the runtime pack, embedding it in
   `jet`, and checking it against the runtime source.
3. The metadata census in claim 1.
4. Deleting the three tables and the export list.

Generated code keeps calling C symbols for the runtime functions it emits
itself (reference counting, allocation), which is unchanged.

## Verdict

Option E **holds within stated limits**, after two amendments that this
review makes part of E: declared effect rows on `pub` runtime functions are
the introducing leaves, proven equal to today's rows at cutover; and the
runtime interface is generated from source with the runtime pack and
embedded, never hand-written. The web tier's JavaScript implementations are
an existing I9 defect, tracked separately. Public text may say E removes the
compiler's runtime tables and keeps effects checked. It may not say every
tier runs one runtime until the web gap closes.

Gaps, each on the E build card: the metadata census, the generated
interface and its compile-time measurement, the effect-row comparison, the
deletion of the three tables and the export list, and the web-tier runtime.
