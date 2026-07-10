# Own Bytecode VM

Card: #10 / c140. Status: future research plan. Depends on JIT/AOT parity work
and the `JitBackend` seam.

## Goal

Replace the Cranelift tier-1 dev backend with a Jet-owned bytecode VM that keeps
the same dev contract: every AOT-runnable program runs under `jet dev` with the
same stdout, stderr, exit code, diagnostics, panics, and side effects. The VM is
a performance tier, not a second language.

## Beginner/Expert/Hybrid Pass

- Beginner: `jet dev` stays one command. Backend choice is invisible unless the
  user asks for timing or explanation.
- Expert: `jet dossier dev-runtime` and JSON traces expose bytecode, coverage,
  fallback tier, heap state, traps, and hot-swap decisions.
- Hybrid: the VM consumes executable TIR through `JitBackend`; no separate AST
  interpreter semantics, no bytecode-only feature surface.

## Current Anchors

- D-JIT1/D-JIT2/D-JITDEP1: production remains AOT; Cranelift is the current
  runtime-side tier-1 and a temporary approved dependency.
- R12 in `architecture.md`: every executable TIR variant has AOT and dev-tier
  consumers.
- `jit-aot-parity.md`: establishes `jet-rt`, `JetVal`, transparent fallback,
  gap manifest, and parity batteries.

## VM Shape

- Input: lowered executable TIR after sema, including monomorphized instances
  for generic functions when needed by the dev tier.
- Bytecode: compact register VM with typed opcodes over `Int`, `Float`, `Bool`,
  `String`, `JetVal`, and control labels.
- Runtime: shared `jet-rt`, same `JetVal`, same trap flag, same panic/report
  policy as Cranelift parity.
- Verification: bytecode verifier checks stack/register type facts before
  execution; failures are internal compiler errors, never user diagnostics.
- Debuggability: bytecode instructions carry Jet spans and semantic labels for
  debugger stepping and replay.

## Implementation Slices

1. Spec bytecode module format and verifier over a tiny TIR subset: literals,
   arithmetic, bindings, branches, calls, print.
2. Add `BytecodeBackend` implementing `JitBackend`, initially delegating to the
   existing dev backend for uncovered TIR.
3. Lower covered TIR to bytecode and run through the VM loop using `jet-rt`.
4. Preserve resident hot-swap: type-stable edits replace bytecode for changed
   functions while keeping compatible runtime state.
5. Widen value coverage in the same order as `JetVal`: strings, lists, structs,
   enums, option/result, maps/sets, closures.
6. Add effect shims and transparent AOT fallback for unsafe in-process cases.
7. Make VM coverage the default tier-1 once batteries prove it at or above the
   Cranelift coverage manifest.
8. Remove the Cranelift runtime dependency only after default `jet dev` parity
   and performance gates pass.

## Test Strategy

- Unit tests for bytecode encode/decode, verifier rejection, trap handling, and
  VM instruction semantics.
- Three-way battery: bytecode VM == interpreter fallback == AOT for covered
  examples.
- Dev battery: stdout, stderr, exit code, diagnostics, panics, side effects.
- Hot-swap tests: state preserved on type-stable bytecode replacement; clean
  restart on type-shape edits.
- Gap manifest ratchet: unsupported TIR operations are listed and shrink over
  time.
- Dependency proof: compiler crates and the VM backend have no external runtime
  dependency replacing Cranelift.

## Ballots Needed

No immediate ballot. Future ballots only if implementation proposes a user-facing
backend selector, bytecode artifact command, permanent limitation, or invariant
carve-out. Default recommendation: keep backend selection internal and expose
inspection through dossier/facts.
