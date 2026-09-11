# Agent codegen benchmark — 2026-08-23

Internal measurement for Card #1589. This report does not make a product or marketing claim.

## Run record

- Baseline: `master`, `b5589b105e8bda461c76cfca485709528b22c4ec`.
- Jet: `v1.0.0`.
- Control: JavaScript on Node.js `v24.16.0`.
- Agent: one Codex agent run. Each language/task cell had one initial generation.
- Repair limit: three compiler-feedback rounds. No cell reached the limit.
- Source files: temporary first-pass candidates. They were not copied from the example sources.
- The checkout had unrelated pre-existing changes. The benchmark did not edit them.

## Harness definition

### Task set

The task set uses six existing example slices. Each task has an existing expected-output file. The tasks are small, but they use real Jet syntax and library surfaces.

| ID | Example and feature | Agent task |
| --- | --- | --- |
| `branches` | [`basics/branches.jet`](../../examples/features/basics/branches.jet) | Classify four Celsius values and print the matching message. Then print the no-umbrella message. |
| `fizzbuzz` | [`basics/fizzbuzz.jet`](../../examples/features/basics/fizzbuzz.jet) | Print FizzBuzz for 1 through 15. Then print a countdown from 3 and `liftoff`. |
| `structs` | [`types/structs.jet`](../../examples/features/types/structs.jet) | Define a `Point` with `Float` fields, a squared-distance method, and a unit constructor. Print the two example values. |
| `filter_map` | [`collections/filter_map.jet`](../../examples/features/collections/filter_map.jet) | Parse mixed strings, keep valid integers, collect all-valid input, and report the first invalid input. |
| `pattern_matching` | [`basics/pattern_matching.jet`](../../examples/features/basics/pattern_matching.jet) | Classify HTTP result ranges and map three scores to letter grades. |
| `wordcount` | [`collections/wordcount.jet`](../../examples/features/collections/wordcount.jet) | Count words in `the quick the brown` and print the count rows in the example order. |

Expected outputs are the matching files under [`examples/features/expected`](../../examples/features/expected). The task prompts state behavior, not the source spelling.

### Control and execution

JavaScript is the control because it is dynamically typed, mainstream, and available in the controlled shell. The runner was Node.js `v24.16.0`.

For each candidate:

1. Generate source with no compiler access.
2. Run Jet `check` or Node `--check`.
3. Give the agent the complete compiler diagnostic text after a failed check.
4. Let the agent edit the same candidate and repeat.
5. Stop repair when the check passes.
6. Run the green candidate and compare stdout byte-for-byte with the example output.

The harness treats compiler feedback and semantic feedback as separate signals. It gives no expected-output feedback during repair. This isolates diagnostic-guided repair from test-driven repair.

Commands:

```text
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet check <candidate.jet>
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet run <candidate.jet>
node --check <candidate.js>
node <candidate.js>
```

### Metrics

- `compile-first-try`: initial candidates with a zero exit from `jet check` or `node --check`, divided by six.
- `repair rounds`: edits after the initial candidate until the compile check passes. One round can fix several diagnostics from one check.
- `semantic correctness`: final green candidate stdout equals the task's expected-output file byte-for-byte.
- `green build`: a successful language check. A green build can still fail the semantic check.

## Summary

| Language | Compile first try | Repair rounds, per task | Mean rounds | Green builds | Semantic correctness after green build |
| --- | ---: | --- | ---: | ---: | ---: |
| Jet | 1/6 (16.7%) | `2, 1, 1, 1, 1, 0` | 1.0 | 6/6 | 6/6 (100%) |
| JavaScript / Node | 6/6 (100%) | `0, 0, 0, 0, 0, 0` | 0.0 | 6/6 | 2/6 (33.3%) |

The first-pass end-to-end rate was 1/6 for Jet and 2/6 for JavaScript. The Jet first-pass success was `wordcount`; the JavaScript first-pass successes were `fizzbuzz` and `pattern_matching`.

This one-agent, six-task run is not enough to support a language-quality claim. It measures one prompt set, one model session, one control runtime, and a small example slice.

## Raw task data

`check` is the initial compile result. `green` is the result after compiler-feedback repairs. `semantic` compares final stdout with the existing expected file.

| Language | Task | Check | Repair rounds | Green | Semantic | Raw diagnostic or stdout result |
| --- | --- | --- | ---: | --- | --- | --- |
| Jet | `branches` | fail | 2 | pass | pass | Round 0: `E0003` at each braced `if` body. Round 1: `E0109` and `E0112` for untyped decimal literals passed to `Float`. |
| Jet | `fizzbuzz` | fail | 1 | pass | pass | Round 0: `E0003`: expected a statement, found `for`; fix named `loop item in source`. |
| Jet | `structs` | fail | 1 | pass | pass | Round 0: `E0102`: nothing named `Point` exists at `Point(3.0, 4.0)`; fix suggested typed construction. |
| Jet | `filter_map` | fail | 1 | pass | pass | Round 0: `E0401` for using `Int.parse` without checking its fallible result, followed by `E0305`/`E0107` cascades. |
| Jet | `pattern_matching` | fail | 1 | pass | pass | Round 0: `E0358` says `Http` is spelled `HTTP`; dependent `E0119`, `E0305`, and `E0112` errors followed. |
| Jet | `wordcount` | pass | 0 | pass | pass | No compiler diagnostic. Stdout matched [`wordcount.out`](../../examples/features/expected/collections/wordcount.out). |
| JavaScript / Node | `branches` | pass | 0 | pass | fail | Stdout began `-5 C: bundle up`; expected `-5.0 C: bundle up`. |
| JavaScript / Node | `fizzbuzz` | pass | 0 | pass | pass | Stdout matched [`fizzbuzz.out`](../../examples/features/expected/basics/fizzbuzz.out). |
| JavaScript / Node | `structs` | pass | 0 | pass | fail | Stdout was `25` and `1`; expected `25.0` and `1.0`. |
| JavaScript / Node | `filter_map` | pass | 0 | pass | fail | Stdout was `[]`, `[, , ]`, and `stopped at: not a number: 1`; expected parsed values and the `oops` error. |
| JavaScript / Node | `pattern_matching` | pass | 0 | pass | pass | Stdout matched [`pattern_matching.out`](../../examples/features/expected/basics/pattern_matching.out). |
| JavaScript / Node | `wordcount` | pass | 0 | pass | fail | Stdout order was `the`, `quick`, `brown`; expected `brown`, `quick`, `the`. |

### Exact Jet diagnostic text captured

The wrapper also printed `warning: Git tree is dirty` because temporary candidates lived in the checkout. That wrapper warning was excluded from compiler-diagnostic counts. The candidate diagnostics were:

```text
branches, round 0:
Error [E0003]: expected a call, binding, assignment, or `return`, found `{`
Why: inside a function body, write a call, binding, assignment, or `return`
Fix: e.g. print("hello") or x :: 1

branches, round 1:
Error [E0109]: `-` needs a number, but this is `Decimal`
Why: only Int and Float values can be negated
Fix: use a number here
Error [E0112]: `describe` wants Float (a decimal number) for argument 1, but this is `Decimal`
Why: every argument must match its parameter's type
Fix: use Float (a decimal number) here

fizzbuzz, round 0:
Error [E0003]: expected a statement, found `for`
Why: this position accepts only Jet's current statement grammar; retired foreign loop words are not statement keywords
Fix: write `loop item in source { … }`

structs, round 0:
Error [E0102]: nothing named `Point` exists here
Why: only functions that have been defined (or built in, like `print` / `input`) can be called
Fix: did you mean `print`?

filter_map, round 0:
Error [E0401]: this needs Int (a whole number), but the value is Int ParseError!
Why: a fallible result must be checked before its value is used
Fix: use `?`, `??`, or test with `== Ok(...)` / `== Err(...)`
Error [E0305]: pattern `Ok` doesn't match Int (a whole number)
Why: variant patterns only work on enum values
Fix: test an enum value, or use `Val` / `None` for optionals
Error [E0107]: nothing named `vs` exists here
Why: a name must be declared before it's used
Fix: declare it first: `vs :: ...`
Error [E0305]: pattern `Err` doesn't match Int (a whole number)
Why: variant patterns only work on enum values
Fix: test an enum value, or use `Val` / `None` for optionals
Error [E0107]: nothing named `e` exists here
Why: a name must be declared before it's used
Fix: declare it first: `e :: ...`

pattern_matching, round 0:
Error [E0358]: `Http` is spelled `HTTP`
Why: Jet keeps acronyms fully capitalized inside PascalCase names (D-ACRO-CASE1=A, D-ACRO-LEX1=A)
Fix: write `HTTP` instead of `Http`
Error [E0119]: there's no type called `HTTP`
Why: the types are `Int`, `Float`, `Bool`, and `String` (plus types you define)
Fix: check the spelling, or define the struct or enum first
Error [E0305]: pattern `Good` doesn't match this value's type
Why: `HTTP` is a struct, not an enum
Fix: use a struct field access instead of a variant pattern
Error [E0112]: `classify` wants `HTTP` for argument 1, but this is `Http`
Why: every argument must match its parameter's type
Fix: use `HTTP` here
```

JavaScript `node --check` emitted no diagnostic for any task. Its semantic failures were runtime output mismatches, not compiler failures.

## Finding

This run supports only a narrow observation: Jet's checker exposed actionable syntax, type, and naming faults in five first-pass candidates, and one repair round fixed each except the decimal-literal issue, which needed a second round. JavaScript accepted all six candidates without compiler feedback, but four had wrong output. The result is evidence for a follow-up benchmark, not a product claim that Jet or strict typing produces better agent code.

## Follow-up needed

Repeat with more agents, multiple generations per task, a matched prompt set, at least one additional control, and a fixed semantic-repair policy. Keep compile-first-try, diagnostic repair, and semantic correctness as separate measures.
