# D-HOST-LEAF1 adversarial review (2026-11-06)

Ballot D-HOST-LEAF1 (card #6151) asks how Jet source, Core included,
declares a function the Jet runtime provides. This review attacked each
strength the first draft claimed. Probes ran on dev 97ec15f323 with the s1
compiler built from 6c15482301.

## Measured facts

- `Compiler/Prelude/Core.jet` has 1057 `dispatcher_row` lines naming 575
  distinct runtime symbols. The draft estimated about 2,000 rows.
- The runtime sources under `Compiler/JetRuntime/Source` define 3216
  `#Export(c)` functions, so most exports are internals that no Core function
  binds.
- `HostTime.jet:214` exports `jet_std_time_sleep_duration_ns(nanos: I64)`;
  `Core/Time/Time.jet` reaches it only through a dispatcher row.
- `parser_marker_is_c_guest_import` (`Compiler/JetParser/Source/Parser/Items.jet`)
  accepts only the argument `c`. A file declaring
  `#Import(jet) sleep_ns: fn(nanos: Int) -[Time.Wait]> Unit = "…"` fails with
  E0081 and an invalid-statement error, so option A needs parser work as
  well as sema work.
- `Docs/spec/safety.md` makes `#Import(c)` a gated region whose sentry wraps
  each call.

## Peer practice

Rust (`extern` blocks with `#[link_name]`), Zig (`extern fn`) and Swift
(`@_silgen_name`) bind a declaration to a symbol without checking it against
the provider. Rust makes every such call `unsafe`. Go's `//go:linkname` let
packages reach runtime internals; Go 1.23 began restricting it because those
internals could no longer change. Peers either trust the declaration or fence
it off as unsafe. None checks it against the provider's signature and effects.

## Claims

### Claim 1: a leaf declaration is safe in any package

**Attack.** In the draft, a declaration stated its own effects and any
package could bind any runtime symbol. A package could therefore drop
`FS.Read` from a file leaf and bypass its authority line. It could also bind
a handle-release internal and free or forge handles from safe code, which
violates I1.

**Verdict: failed as drafted. Holds after redesign.** The runtime now
publishes its Jet-facing surface as `#Export(jet)`, with effect rows and a
Jet-safe boundary law (typed handles, no raw pointers or bare ids). The
package's `#Import(jet)` must match it exactly, effects and failure
included. Unpublished symbols are reachable only through `#Import(c)` inside
`#Unsafe`, which the ratchet counts.

### Claim 2: one mechanism (I8)

**Attack.** A separate host marker (option B) would duplicate the meaning
`#Import` already has. Reusing `#Import` adds one argument spelling, `jet`,
which needs I7 registration.

**Verdict: holds.** The ballot names `jet` as the only new spelling.

### Claim 3: no drift between declaration and runtime

**Attack.** Hand-written declarations can drift from the runtime. The draft
did not name a check, and it missed option D, a generated runtime module with
no hand-written declarations.

**Verdict: holds within stated limits.** The export manifest is generated
when jet builds its runtime pack, embedded in jet, and drift-checked in the
chain. Option D is added to the ballot and rejected on its merits: C symbol
names in callers, and #6151 c1/c3 lose their subject.

### Claim 4: one meaning on every tier (I9)

**Attack.** The draft did not cover web or compile-time code.

**Verdict: holds within stated limits.** Every tier links the same published
symbols from the shared runtime, and a target that lacks an export reports
it through the existing target-capability rule. Whether the web tier provides
every published export has not been measured; that is a #6151 build
criterion.

### Claim 5: runtime-side effects are correct

**Attack.** The runtime author writes effects on `#Export(jet)`, and nothing
proves them.

**Verdict: holds within stated limits.** This is the vetting duty Core
carries today; runtime code is vetted standard-library code under I1.

## Gaps carried to the build card

- Measure web-tier provision of every `#Export(jet)` symbol.
- Register the `jet` marker argument in the syntax registry.
- Register diagnostics for each mismatch (symbol, types, effects, failure,
  unpublished symbol), each with a UI snapshot.
