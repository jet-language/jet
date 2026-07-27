# Arrow syntax consistency audit

Date: 2026-07-27

Scope: Tower cards #1207, #1204, #1211, and #1212.

## Authority

The repository now follows the ratified decisions:

- D-ARROW-CONTROL1: `=>` defines callable results. `=[Effects]=>` defines
  effectful callable results. `->` selects branch values and finite loop items.
- D-LOOPEVAL1: effect loops have no arrow. Finite yielding loops use `->`.
- D-LOOPSTATE1: ordinary result loops return through `break value`.
  Named exits use `break(name)`, `break(name, value)`, and `next(name)`.
- D-COMPREHENSION1: a finite yielding loop eagerly returns `List<T>`.
  Maps use ordinary explicit construction, Sets use `Set.from`, and lazy work
  uses existing iterator adapters.

The normative spec, syntax registry, proposal, reference surface, diagnostics,
examples, and compiler paths use these rules.

## Migration inventory

The migration inventory snapshot, excluding research history, archives, and
Tower state, covered 555 tracked files. It removed 736 old function-result
arrow lines, 21 named dot-exit lines, 18 protocol-direction lines, and four
explicit closure-capture prefixes. It added 769 callable-result lines and 32
target-argument exit lines. Added counts include new tests and examples.

Independent review then found and corrected stale embedded sources in the
arena and duration tests, stale conversion snapshots, Canvas scenarios, both
TextMate grammars, and the Zed grammar source and WebAssembly artifact. The
grammar drift test now covers the classic TextMate mirror.

The audit searched authored Jet files, embedded Jet programs, generated-source
owners, UI snapshots, LSP and REPL fixtures, docs, editor grammars, compiler
diagnostics, and syntax registries.

No actionable stale syntax remains. The remaining matches are intentional:

- `tests/ui/effect_arrow_retired.jet` and its snapshot test the retired effect
  arrow diagnostic.
- `tests/ui/protocol_bad_endpoint.jet` and its snapshot test invalid protocol
  syntax.
- Parser negative fixtures in
  `crates/jet-parser/src/Parser/mod.rs` test retired spellings.
- `docs/proposals/yielding-loops.md` keeps a labeled migration `Before` block.
- Research and archive files preserve design history.
- `crates/jet-sema/src/Sema/ApiFreeze.rs` preserves pre-v3 compatibility and a
  frozen plugin signature.
- Rust source and generated Rust fixtures continue to use Rust `->`.
- Ordinary iterator `.next()` calls are not Jet named-loop exits.

Generated Jet text was changed at its generator. Generated Rust and frozen
compatibility evidence were not rewritten as Jet source.

## Shipped behavior

The compiler supports:

- one-line and multiline effect loops;
- finite List-producing loops over sources, ranges, and C-style headers;
- guards, filters, multiple dependent sources, and nested List results;
- partial List results through bare or targeted `break`;
- ordinary loop results through compatible break payloads;
- parser, formatter, sema, TIR, comptime, interpreter, native JIT, and AOT
  agreement, including named exits nested in operators, calls, conditions, and
  C-style loop initializer and afterthought expressions;
- E0072 through E0076 diagnostics with focused UI snapshots.

The executable `loop_values.jet` example and its fuzz mirror cover eager List,
partial break, nested List, explicit Map construction, `Set.from`, lazy iterator
materialization, and a named ordinary result loop.

Collection materialization does not add a second traversal. Sema lowers each
accepted item to an append on one eager List accumulator inside the existing
finite loop. Source evaluation, mutation checks, failures, ownership, cleanup,
and exhaustion therefore stay on the ordinary loop path. The unified-loop
edge test proves one-time source evaluation and normal advancement; the
all-backend loop-value tests prove eager item order and partial exits.

## Verification

Passed:

- `cargo test -p jet-parser` — 28 tests.
- `cargo test --test grammar` — 17 tests.
- `cargo test --test fmt` — 143 tests.
- focused formatter, AOT, interpreter, native JIT, golden, LSP hover, and LSP
  transcript tests;
- focused LSP signature help for callable and effect arrows;
- focused comptime/runtime parity for a yielding loop;
- focused UI snapshots for E0072, E0073, E0074, E0075, and E0076;
- focused refreshed `crypto_secret_name_collisions` snapshot;
- focused E2405 and E2406 conversion snapshots;
- focused arena escape and duration runtime/JIT tests;
- `cargo check -p jet-sema -p jet-codegen -p jet-jit -p jet-comptime`;
- a fresh `cargo build`;
- regenerated Zed Tree-sitter WebAssembly from the authoritative grammar;
- `tower lint --docs`;
- `git diff --check`.

The broad diagnostics-coverage test has unrelated active-tree failures:
E0920, E1109, E2407, and L3101 are registered but not emitted; E0031, E0034,
E0048, E0139, E0320, E0342, E0346, E0350, E0412 through E0418, E0761, E0992,
E0994, E2111, and E3204 lack snapshots. E0072 through E0076 are not among
those failures.

The broad comptime differential battery also exposes an unrelated existing
`String.split` iterator display codegen failure. The exact yielding-loop
comptime parity test passes.

`scripts/agent/jet-env full scripts/agent/verify-full.sh` completed its fresh
build, then stopped in the root library suite because
`Interpreter::tests::resident_jit_safe_task_examples` exhausted the default test
thread stack. The exact test reproduces with the default stack and passes with
`RUST_MIN_STACK=16777216`. It checks the separately owned concurrency examples;
it does not exercise yielding loops, collection materialization, or retired
syntax acceptance.

The four focused Tower cards contain the scoped evidence and independent
review. The unrelated broad-suite failures above remain isolated from this
syntax proof.
