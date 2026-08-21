# Card #1974 — User-space compile-time checks over program facts

Date: 2026-08-21. Card: #1974.

## Result

Jet already has the correct user-space check mechanism: the selected root
`fn build` runs in the shared comptime interpreter over a read-only, post-sema
`ProgramInfo` snapshot. It can inspect entities and add diagnostics before
codegen. A new `jet lint` extension or compiler-plugin check runner would be a
second mechanism and is not part of this plan.

One implementation defect remains. `b.error` currently accepts arbitrary
project codes through `Diagnostic::project_error`; the newer I4 law requires a
typed registered row and a UI snapshot for every emitted report. That repair is
separate from adding a new check mechanism.

## Survey

### Fixed lint surface

`jet lint` currently has exactly two fixed categories: `--a11y` and
`--complexity` (`Source/main.rs:2676-2710`). The commands check the source,
then either filter registered accessibility diagnostics or calculate the
fixed complexity report (`Source/CmdDevTools.rs:3162-3313`). They do not receive
`ProgramInfo`, and their `--max` path constructs a fixed `L2902` report at the
CLI boundary (`Source/CmdDevTools.rs:3286-3313`). This is a compiler-owned
lint surface, not a project rule registration point.

### Inspect and compiler facts

The inspect commands are projections, not additional checking engines.

- `jet inspect compiler` mirrors the read-only `core.compiler` API
  (`Source/main.rs:1920-1929`; `docs/spec/architecture.md:421-436`). The API
  returns versioned lex, parse, check, source-map, and semantic-index values;
  it does not expose compiler internals or perform policy checks.
- `jet inspect expand` builds its output from the checked bundle and semantic
  index only (`Source/CmdExpand.rs:1-9,19-75,145-169`). Its lenses are one
  registered CLI projection (`Source/CmdExpand.rs:19-75`), not a promise that
  every CLI line is a comptime field.
- `jet inspect gates` collects an authority/gate ledger from the loaded bundle
  (`Source/CmdGates.rs:24-64`). Gate provenance is not an entity fact and is
  not a new input to project checks.
- `jet inspect facts` enumerates the shared registration table
  (`crates/jet-cli/src/Explain.rs:467-548`). Typed fact reads resolve through
  the same registry (`crates/jet-foundation/src/Registry.rs:739-780`). There
  must be no private check-only fact table.

## Proposed check contract

### 1. Read boundary

The first check contract is closed and typed:

- `b.program.types()`, `.functions()`, and `.packages()` return the existing
  `ProgramInfo` projections (`crates/jet-comptime/src/Comptime/Builtins.rs:2189-2198`).
- `TypeInfo` exposes the existing name/path/layout/span, fields, methods,
  type parameters, markers, states, transitions, facts, dimensions, and
  implementation rows (`crates/jet-comptime/src/Comptime/Reflect.rs:1839-1873`).
  `has_method` and `implements` are projections over those fields
  (`crates/jet-comptime/src/Comptime/Builtins.rs:2215-2223`).
- `FunctionInfo` exposes name/module/identity, parameters, span, effects,
  `reaches_panic`, and registered facts. Effects and reachability come from
  sema facts; they are not recomputed in the check
  (`crates/jet-comptime/src/Comptime/Reflect.rs:2320-2371`).
- A registered `FactInfo` is readable only through the existing fact registry
  and fact readers (`crates/jet-comptime/src/Comptime/Reflect.rs:600-622,1721-1745`).
  Build package/profile/stamp values likewise remain folded snapshot values,
  not engine lookups (`crates/jet-foundation/src/Facts.rs:31-48,109-139`).

The check must not read raw AST nodes, mutable sema state, rustc output,
generated Rust, or scrape the JSON/text output of an inspect command. If a
future rule needs an explicit source query, it must reuse the existing typed
`core.compiler` result; it must not add a second checker or widen this
program-fact snapshot by copying CLI fields.

### 2. Report boundary

The required report shape is one registered diagnostic row:

- one stable code;
- row-owned severity and report moment;
- row-owned What, Why, and Fix templates with named holes;
- the check supplies only hole values and the source span;
- the normal renderer, UI snapshot, `jet explain` projection, and coverage
  guards consume that same row.

This is the current diagnostic contract (`docs/spec/diagnostics.md:42-97`)
and the ratified one-home law (`docs/spec/syntax-decisions.md:7430-7439`).
`Diagnostic::error` enforces a registry row (`crates/jet-foundation/src/Diagnostics.rs:282-310`),
but `Diagnostic::project_error` deliberately makes the row optional
(`crates/jet-foundation/src/Diagnostics.rs:335-373`). The build bridge still
routes ordinary project codes through that optional path
(`crates/jet-comptime/src/Comptime/Build/runtime_bridge.rs:694-717,1006-1013`).

Therefore the implementation plan is to repair the existing `b.error` path to
use a registered row, not to create a second project-diagnostic renderer. A
project-owned code-registration model is the unresolved product choice; until
it is ratified, arbitrary `ORG01`-style codes cannot be called I4-compliant.
The existing reserved compiler-code guard remains a normal registered
diagnostic (`E3530`, `docs/spec/diagnostics.md:1986-2000`).

### 3. Execution boundary

Checks run only in the selected root `fn build`, after sema has checked the
whole bundle and produced semantic facts, and before the build plan reaches
codegen. The shared comptime interpreter documents this boundary
(`crates/jet-comptime/src/Comptime/mod.rs:368-377`) and returns collected
diagnostics with the build evaluation (`:395-477`). The driver constructs the
semantic snapshot, invokes that path, and fails before later build work when
the check reports errors (`crates/jet-driver/src/Driver/mod.rs:2895-2919`).

`comptime {}` remains value computation. Imported modules do not gain an
independent check hook. `jet lint` remains fixed-category reporting. No AOT,
JIT, interpreter, web, or rustc host gets a second copy of check policy; the
compile-time result goes through the shared diagnostic channel.

## Proof plan if the existing path is repaired

1. Keep the current `ProgramInfo` shape assertion and add one rule over types,
   one over functions/effects, and one over a registered fact. Assert that
   sema-computed values are the values seen by the check.
2. Add a `tests/ui/` failing fixture and exact stderr snapshot for the
   row-backed project finding. Run both diagnostic coverage directions and
   assert the source span, What/Why/Fix, severity, and code.
3. Keep the existing root-only and reserved-code tests in
   `tests/build_entry.rs` (`:1386-1400,1537-1545`) as regression proof. Add a
   clean example and a failing example to the executable example/golden set.
4. Verify the same compile-time path through the normal build and default
   `jet run`; no `jit_gaps` entry or AOT-only exception is acceptable.

No new user-typeable syntax, external dependency, or I9 carve-out is needed
for the existing `fn build` path. If the chosen diagnostic-registration model
adds a source declaration, its exact spelling and I7 decision must be raised
before that slice is implemented; this note invents no spelling.

## Decision and follow-up

Decline a new #1974 check mechanism. The ratified `D-METADEPTH2` design and
the shipped `fn build` path already cover user-space checks over program facts;
extending `jet lint` would duplicate that path. Route the registered-row
repair for `b.error` to the existing diagnostics/programmable-build work.

### Narrow ballot: project diagnostic ownership

Question: how does a project-defined finding obtain its I4 row?

- **A — one shared registry, recommended.** Add project-owned rows to the
  existing registration table. A check supplies only the registered code's
  named hole values and span; the row owns severity, moment, What, Why, Fix,
  renderer projections, and snapshot coverage. If a source declaration is
  needed, its exact spelling is an I7 decision.
- **B — compiler rows only.** Project checks use compiler-owned registered rows;
  project identity is data in named holes, not a new diagnostic-code family.
- **C — scoped exception.** Ratify rowless project codes and their own
  snapshot/coverage contract, explicitly amending I4. This preserves the
  current `project_error` behavior but creates a second report ownership rule.

Recommendation: A. Until this narrow ballot is ratified, do not implement new
project-code behavior and do not call the current arbitrary-code path I4
compliant. This research note does not modify compiler behavior or board data.
