# Jet example quality audit

Date: 2026-07-26

## Decision

Jet's authored example corpus now uses the current language surface more
consistently and with less ceremony.

The audit reviewed all 415 authored `.jet` files under `examples/`, all 11
`pkg.jet` manifests, and all three generated `.jet` files under hidden `.jet`
directories. It changed 231 authored files. Authored source fell from 11,304
lines to 10,751 lines, a reduction of 553 lines or 4.9%. No example or
demonstrated behavior was removed to reach that number.

The pass made these broad corrections:

- immutable bindings now use `::` unless later reassignment or stateful
  mutation requires `:=`;
- simple untyped lambdas use the current bare parameter spelling, such as
  `item => item.name`;
- short functions, loops, and conditional bodies stay on one line when that
  shape is easier to scan;
- repeated conditional ladders use Jet's guard dispatch where the cases form a
  decision table;
- iterator and collection operations replace manual accumulation when the
  direct operation states the intent;
- callback expressions stay inline when the call supplies type inference that
  a named helper would lose;
- current `DataTree` constructors replace the obsolete JSON shape in the JSON
  example;
- formatter behavior now preserves these choices instead of expanding or
  flattening them on the next format pass.

This work changes no user-facing syntax. It uses ratified features that were
already present.

## Scope ledger

| Surface | Reviewed | Changed | Result |
| --- | ---: | ---: | --- |
| Authored example source | 415 files | 231 files | 553 fewer lines |
| Package manifests | 11 files | 0 files | Already current |
| Hidden generated Jet source | 3 files | 0 files | Accounted for; generator-owned |
| Differential example mirrors | 64 files | 42 files | 61 fewer lines |
| Valid-corpus mirrors | Existing corpus | 5 files | 23 fewer lines |
| Expected output | Existing corpus | 3 files | Reconciled with corrected examples |

The hidden generated files are:

- `examples/features/tooling/programmable_build/.jet/generated/main/build_message.jet`;
- `examples/features/packages/inline_deps/.jet/inline-deps/textkit/1.4.2/textkit.jet`;
- `examples/features/lowlevel/cbind/.jet/bindings/c/c.jet`.

They were inspected but not hand-edited. Generated source must remain owned by
its generator.

## Quality changes

### Default to immutable bindings

The pass converted bindings to `::` when the value never changes and retained
`:=` when later mutation, resource cleanup, or a demonstrated state transition
requires it. `::` tells the reader that the name is fixed,
narrows the state they must track, and demonstrates Jet's safe beginner
default. `:=` remains where the program reassigns a name or mutates through it.

The retained mutable cases include counted accumulators, buffers, protocol
state, reactive signals, and examples whose purpose is to teach mutation. The
pass did not replace real state with hidden state in a helper or allocate a new
collection merely to avoid `:=`.

### Use the shortest clear body

Simple functions and control-flow bodies now stay on one line when the complete
operation fits the formatter's width limit. For example:

```jet
fn double(n: Int) => Int { return n * 2 }

loop item; items { print(item) }
```

Braces still define scope. Multiline bodies remain multiline when they contain
several steps, comments, cleanup, or state that benefits from vertical layout.
This follows the same rule for functions, loops, conditional bodies, and
dispatch arms: use one line for one clear operation, and use a block when the
reader needs structure.

### Prefer decision tables to ladders

FizzBuzz, conditional-expression, and scoring examples now use subjectless
guard dispatch for ordered predicates:

```jet
label :: if {
    score >= 90 -> "excellent"
    score >= 75 -> "good"
    else -> "keep going"
}
```

This makes precedence visible and avoids repeated `else if` scaffolding. Two
`else if` clauses remain in the basic branch example. The audit tried the
equivalent guard table, but current AOT lowering emitted invalid Rust for the
final arm. The robust executable spelling stays until that compiler gap closes.

Ordinary `if` remains where there is one condition, early return, or
state-dependent sequence. Pattern matching remains preferred when the input is
an enum, option, result, or another closed shape. Guard dispatch is not used as
a substitute for exhaustive variant matching.

### Prefer direct collection intent

The parallel scan example now uses direct operations such as
`text.lines().len()`, `io.args().skip(1).to_list()`, and iterator mapping instead
of manual loops that only counted, skipped, or copied elements.

Manual loops remain where they teach loop syntax, preserve streaming behavior,
update several pieces of state, enforce a bound, or model a low-level
algorithm. A chain was not accepted merely because it was shorter. It also had
to be easier to read and preserve the example's lesson.

### Preserve inference where it carries meaning

The hostile data example has three inline pivot callbacks. The audit tried
extracting them into named functions, but that lost the library call's callback
inference and produced an invalid AOT callback type. The inline form passes its
focused golden test and keeps the row, column, and value projections together.

Names remain preferred for multi-step transformations when extraction preserves
the same type and access semantics. A helper is not an improvement when it
hides required inference or makes an executable example fail.

### Use current data constructors

`examples/features/serde/json.jet` used an obsolete mixture of `JSON` and
`DataTree` constructors. It now constructs the current `DataTree` shape
directly. The example passes `jet check`.

The default JIT still reports an existing E0956 support gap for part of this
path. The source contract is now correct, but complete execution parity remains
compiler work.

### Keep long fixtures coherent

The audit inspected every remaining authored line over 100 characters. There
are 81: five comments and 76 code or fixture lines.

Ordinary collection literals that were difficult to scan, including the
parallel-scan path list, dynamic-array incidents, command arguments, columnar
particles, dashboard cards, the hostile WebAssembly map, structured email
configuration, and nested XML events, now use vertical layout.

The retained long lines are exact cryptographic and binary test vectors,
encoded tokens, SQL/HTTP/text fixtures, policy declarations, or atomic
signatures and calls whose grouping is part of the example. Splitting exact
vectors into one byte per line would make comparison with their source standard
harder. Splitting encoded payloads would change the value. These are measured
exceptions, not unreviewed formatter output.

## Formatter corrections

An example cleanup is not durable if `jet fmt` reverses it. The formatter now:

- preserves bare one-parameter lambdas;
- preserves author-written concise dispatch arms when they fit;
- preserves explicit braces when the author chose a scoped arm;
- keeps empty inline functions as `{}`;
- omits obsolete semicolons from distinct type declarations;
- preserves intentional blank lines between statement sections;
- preserves readable multiline list, map, tuple, typed literal, and struct
  literal layouts.

A dispatch arm needs one special rule. If the next arm begins with a leading
dot variant, an unbraced expression on the previous arm can parse as part of a
dot chain. The formatter adds concise braces to the preceding arm in that
case. The braces are a parse boundary, not ceremony.

Formatter tests cover each behavior and remain idempotent.

## Retained forms

The audit intentionally kept:

- multiline functions with several operations or cleanup;
- explicit types when the example teaches a type or resolves real ambiguity;
- ownership and capability marks when they demonstrate read, edit, take, copy,
  sharing, or an unsafe boundary;
- manual loops for streaming, protocol parsing, buffers, cryptography,
  concurrency, or loop instruction;
- `?? panic(...)` where the example deliberately demonstrates fail-fast
  application policy with context;
- lower-level HTTP and process builders where explicit control is the lesson;
- generated and vendored example files under hidden `.jet` directories.

Deleting these forms would make examples shorter but less truthful.

## Corpus health found by the audit

Every authored file is formatter-clean. A standalone `jet check` sweep passes
395 of 415 files.

The 20 failures are existing product gaps:

| Gap | Files |
| --- | ---: |
| Generated interop binding is absent until its project step runs | 11 |
| Compile-time evaluator does not yet support the demonstrated operation | 5 |
| Missing `View` surface | 1 |
| Missing registered crypto byte-comparison item | 1 |
| Empty-list contextual constructor inference | 1 |
| Generic module type resolution | 1 |

The generated-binding group contains six top-level interop projects, the
programmable-build example, and four low-level polyglot projects.

The deeper valid-corpus-to-rustc test remains red on 11 existing frontend,
code-generation, and stale-mirror failures. The final full golden run also
remains red on existing compile-time evaluation, generic struct construction,
HTTP, UI sendability, serialization ordering, WebAssembly target, and native
toolchain gaps. Focused reruns showed that the email generic-constructor and
effect-callback failures reproduce with their original binding and callback
spellings. These failures are not formatting failures, and they must not be
hidden by weakening the tests.

The example corpus should ultimately reach 415 of 415 standalone checks and a
clean golden run. This pass makes that remaining compiler backlog explicit.

## Verification record

Passed:

- all 415 authored files: `jet fmt --check`;
- formatter suite: 142 tests;
- strict default-JIT/AOT differential example gate;
- focused `jet check` on every semantically rewritten example;
- standalone corpus sweep: 395 passed, 20 known product gaps;
- focused AOT golden tests for the basic branch, resource cleanup, hostile data,
  encoding stream types, XML stream, and headless game examples.

Also run:

- valid corpus to rustc, with a larger test-thread stack: completed and reported
  11 existing failures;
- final full example golden, with a larger test-thread stack: completed in 89
  seconds and exposed the existing compiler/runtime failures described above;
- repository-wide `verify-full.sh`: reached `tests/build_entry.rs`, then stopped
  with 25 existing compile-time evaluator failures, primarily E0956 for
  programmable-build methods;
- independent fresh-context review: formatter and example changes approved;
  stale report metrics found by the reviewer were corrected here.

The first golden run also found three expected-output files affected by source
corrections. Those files were updated before the final run.

## Follow-up standard

New examples should meet the same bar:

1. use immutable bindings by default;
2. choose pattern matching or guard dispatch when it makes the decision shape
   clearer;
3. use a direct Core operation before writing a manual collection loop;
4. keep one clear operation on one line when it fits;
5. give multi-step transformations names;
6. preserve explicit expert control when that control is the lesson;
7. pass formatter idempotence, standalone checking, and the relevant
   JIT/AOT/golden path;
8. update matching mirrors and expected output in the same change.

Line count is a useful pressure test, not the sole objective. The target is the
shortest source that remains safe, explicit about important policy, and easy to
reason about.
