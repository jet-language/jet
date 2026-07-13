# Bootstrap Readiness Gate

Card: #217 / c0949cz5. Status: required precondition for self-hosting work.

## Goal

Define the evidence Jet must have before any compiler port to Jet begins. The
gate protects I1/I2/I3 and the beginner/expert identity: the compiler may be
ported only after the core language is stable enough to reason about cold, and
the memory model has adversarial proof comparable to Rust's safety bar.

## Beginner/Expert/Hybrid Pass

- Beginner: self-hosting must not change how `jet run file.jet` works or expose
  compiler-stage jargon.
- Expert: every stage, subset, bootstrap binary, generated artifact, and test
  battery is inspectable and reproducible.
- Hybrid: one compiler semantics path remains. Stage0/stage1/stage2 are build
  stages over the same language law, not dialects.

## Evidence Required

1. Core language locked-happy: syntax law reconciled, no provisional syntax, no
   retired spellings in docs/examples/tests, and every ratified feature either
   implemented or explicitly staged with a named gate.
2. Dogfood portfolio: real Jet projects covering compiler-shaped code, arena and
   index-handle ASTs, formatter/interpreter code, long-lived services,
   comptime-heavy code, FFI-heavy code, UI/web/game/data code, and large
   multi-package workspaces.
3. Readability review: each portfolio project reviewed by a cold agent/human for
   local reasoning, API clarity, diagnostics, and maintainability.
4. Memory-model soundness: D-MEM1 stress suite, fuzzing, differential generated
   Rust checks, escape attempts, aliasing writes, use-after-take, arenas, string
   views, `Shared<T>`, `Pool<T>/Id<T>`, tasks, and unsafe gates.
5. Coverage sweep: every ratified surface feature appears in portfolio examples
   or gets a follow-up plan/ballot.
6. Toolchain determinism: builds avoid timestamp/hash-order leaks; generated
   outputs are reproducible enough for stage comparison.
7. Owner sign-off: final ballot records that port work may start.

## Implementation Slices

1. Build a readiness matrix with rows for language law, diagnostics, examples,
   docs, memory model, TIR/dev parity, packages, and tooling.
2. Select portfolio projects and define acceptance criteria for each.
3. Add adversarial memory-model fixtures and fuzz/differential harnesses.
4. Run syntax and docs reconciliation against `Syntax.rs`, syntax decisions,
   examples, diagnostics, reference docs, and editor grammar output.
5. Prove deterministic build inputs and output comparison strategy for stage1
   and stage2.
6. Queue final readiness sign-off ballot when evidence is assembled.
7. Only after sign-off, queue self-hosting charter ballots and implementation
   cards.

## Test Strategy

- Dedicated memory-model test suite plus fuzz corpus for ownership, views,
  arenas, shared handles, pools, tasks, and unsafe gates.
- Full `cargo test`, golden examples, diagnostic snapshots, dev/AOT parity
  batteries, syntax reconciliation, grammar drift checks.
- Portfolio CI runs real projects under `jet check`, `jet test`, `jet run`,
  `jet dev` where relevant, and docs generation when available.
- Determinism tests compare repeated outputs and later stage1/stage2 artifacts.

## Ballots To Queue

### D-BOOTSTRAP-GATE1 - Port readiness sign-off

Group: architecture.

Gist: decide whether evidence is strong enough to begin self-hosting.

Story: Walter wants to port the lexer next week. Before that starts, he needs
the owner to confirm the language, examples, and memory model are solid enough
that porting will not freeze unstable semantics into the compiler.

In wild:

```text
jet bootstrap readiness --report readiness.json
```

Options:

- A: Approve port work now. The readiness report passes every required evidence
  row; self-hosting charter ballots may open.

```text
jet bootstrap readiness --report readiness.json
result: ready
next: queue D-SELFHOST1
```

- B: Hold for named evidence gaps. The report lists missing portfolio,
  memory-model, determinism, or syntax-lock evidence; port work stays blocked.

```text
jet bootstrap readiness --report readiness.json
result: blocked
missing: memory-model fuzz corpus, stage determinism proof
```

- C: Approve only isolated prototype ports. Allows experiments that cannot
  replace compiler stages or affect release artifacts.

```text
jet bootstrap prototype lexer --stage0 target/debug/jet
result: allowed prototype
release: unchanged
```

Comparisons:

- Rust bootstraps through staged compilers and snapshot trust.
- Go keeps a clear bootstrap toolchain boundary.
- Zig has staged self-hosting but keeps bootstrap constraints explicit.

Rec: B until the readiness report is complete; A once every evidence row is
green. The decision should be evidence-bound, not schedule-bound.

### D-SELFHOST1 - Self-hosting charter

Group: architecture.

Gist: choose what "self-host Jet" means for the first port.

Story: Dana starts the compiler port. She needs to know whether stage1 still
emits Rust through rustc, whether native backend work must happen first, and
which compiler subsystems may stay Rust.

In wild:

```text
stage0 jet build compiler.jet
stage1 jet build compiler.jet
compare stage1 stage2
```

Options:

- A: Jet compiler in Jet, still emitting Rust. Stage0 is the pinned Rust-built
  Jet; stage1 and stage2 are Jet-built compilers that preserve the hidden rustc
  verifier backend.

```text
stage0 jet emit --rust compiler.jet
stage1 jet emit --rust compiler.jet
compare stage1 stage2
```

- B: Native backend first. Port waits until Jet can build the compiler without
  rustc in the chain.

```text
jet build compiler.jet --backend native
compare stage1 stage2
rustc: not in bootstrap chain
```

- C: Piecewise port through stable ABI seams. Port lexer/parser/formatter first
  and link them into the Rust driver until enough subsystems are Jet-owned.

```text
jet build compiler/lexer.jet --target plugin
jet build compiler/parser.jet --target plugin
JET_COMPILER_SEAMS=lexer,parser target/debug/jet check examples/canon.jet
```

Comparisons:

- TypeScript self-hosts while still emitting JavaScript.
- Rust self-hosts but retains LLVM as backend.
- Zig is moving toward a self-hosted compiler and backend in staged form.

Rec: C for the first port wave after D-BOOTSTRAP-GATE1, with A as the explicit
stage target. It gives real Jet compiler code early while preserving the current
I2/rustc-hidden backend contract until a native backend is separately ratified.
