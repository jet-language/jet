# Examples

`Examples/` is the runnable source library for Jet. The feature tree is the
primary teaching surface; each example is a small program that can be run from
the repository root. The source and its expected output, rather than this
index, define the behavior.

## Run an example

Use the checked-out executable (or the equivalent `jet` on your `PATH`):

```sh
target/debug/jet run Examples/features/basics/hello.jet
```

The feature runner exercises the same layout in one pass and records the
command, result, and output for each project:

```sh
node Tools/agent/run-feature-examples.mjs
```

For the contributor workflow around the executable and its environment, see
[`AGENTS.md`](../AGENTS.md) and [`Tools/agent/jet-env`](../Tools/agent/jet-env).

## Find an example

Start with the smallest source in [`features/basics/`](features/basics/), then
move to a sibling with a longer name or an `expert` suffix when it exists.
[`features/types/`](features/types/), [`features/errors/`](features/errors/),
[`features/effects/`](features/effects/), and
[`features/tooling/`](features/tooling/) provide useful next stops. The
[`features/`](features/) tree is the executable registry; its directory
structure is deliberately more complete than this index.

The `expected/` tree contains output fixtures. Some top-level files and
specialized directories are regression or integration fixtures rather than a
learning sequence; use their neighboring source and expected-output files as
the contract.

[`struct_call_word.jet`](features/basics/struct_call_word.jet) demonstrates
fixed-width FNV results in record fields, direct and wrapper calls, and record
copies. Its [golden](features/expected/basics/struct_call_word.out) preserves all
64 bits independently of the exact `Int` representation.

## Auxiliary golden stream suffix

The feature runner discovers `.jet` files and project directories containing a
`run.jet`. For a source file named `topic/example.jet`, the native stdout
fixture is `expected/topic/example.out`. For a project, the directory name is
used as the stem, so `topic/example/run.jet` maps to
`expected/topic/example.out`. The native checker compares stdout byte for byte;
an example with no native golden is skipped rather than given an invented
result.

The native checker uses `.out`, `.err.out`, and `.stderr.out`. Dedicated web,
harness, test, fuzz, and input-specific checks consume the other suffixes
listed below:

| Fixture | Meaning |
| --- | --- |
| `<stem>.out` | Expected native stdout. |
| `<stem>.err.out` | Expected non-zero execution, including a panic or uncaught error. |
| `<stem>.stderr.out` | Expected stderr while execution exits successfully. |
| `<stem>.web.out` | Web-target output used by the corresponding web fixture. |
| `<stem>.harness.out` | Harness-managed output. |
| `<stem>.seed.out` / `<stem>.greet.out` | Input-specific output for the matching example. |
| `<stem>.test.out` | Test-command output. |
| `<stem>.fuzz.out` | Fuzz-command output. |

The relevant golden checker is the executable definition of each mapping. Keep
source changes and their fixtures together; do not use this README as a second
list of expected output.

## Learning paths

`features/basics/` is the shortest path through expressions, functions, and
output. Continue through the language/data row before choosing a runtime or
application topic. [`Examples/learn/`](learn/README.md) is a separate,
offline curriculum: it teaches prediction, evidence, explanation, a controlled
change, and transfer rather than adding another feature inventory.

## Short path first

Flagship examples teach the magic default first. When manual mechanics are
worth keeping, place them in a clearly labelled `*_expert.jet` sibling rather
than making the long form the only path (D-EXAMPLES-SHORTPATH1=A).

| Teach first | Keep beside it as expert |
| --- | --- |
| `#CLI` typed entry arguments | Raw `process.argv()` walks |
| `para_map` / `task.group` | Hand-rolled channels, tasks, and joins |
| `files.open(...).lines()` streaming | Materializing all file text before processing |

The maintained [`first_hour.jet`](features/basics/first_hour.jet) example and
its [`first_hour_expert.jet`](features/basics/first_hour_expert.jet) sibling
show this rule for typed CLI arguments. The
[`onboarding/run.jet`](features/basics/onboarding/run.jet) project is the next
step after `jet new`; the complete `new` → `run` → `check` → `test` → `fix` →
`explain` workflow is in the [first-hour guide](../Docs/spec/guides/first-hour.md).

New examples should use the beginner default first and keep an expert sibling
only when the manual form still teaches something important.

Call a feature a teaching example: it explains a mechanism, not merely that a
work item closed. If a file exists only as a bare, unexplained card-closure
record, prefix that comment with `ledger` so readers can distinguish evidence
from teaching material. A card cited alongside a decision or mechanism
explanation does not need the marker.

## Current-law teaching manifest

This finite manifest ties selected teaching sources to the rule they show, the
checked-in UI or golden snapshot, and a command that exercises the same source.
The [syntax decisions](../Docs/spec/syntax-decisions.md) page supplies the
normative language contract.

| Source | Rule it demonstrates | UI/snapshot check | Command |
| --- | --- | --- | --- |
| [`features/basics/loop_forms.jet`](features/basics/loop_forms.jet) | One `loop` keyword covers infinite, conditional, source, mixed list/map iterables, and inclusive or exclusive ranges; `next` advances the source. | [`features/expected/basics/loop_forms.out`](features/expected/basics/loop_forms.out) | `target/debug/jet run Examples/features/basics/loop_forms.jet` |
| [`tests/ui/list_loop_mutate.jet`](../tests/ui/list_loop_mutate.jet) | An immutable loop binding cannot mutate the collection it reads; E0507 keeps the traversal domain and repair intent explicit. | [`tests/ui/list_loop_mutate.stderr`](../tests/ui/list_loop_mutate.stderr) | `target/debug/jet check tests/ui/list_loop_mutate.jet` |
| [`features/errors/errors.jet`](features/errors/errors.jet) | `Ok`/`Err`, unmarked propagation, `??` fallback recovery, and an explicit result-pattern handler are distinct routes. | [`features/expected/errors/errors.out`](features/expected/errors/errors.out) | `target/debug/jet run Examples/features/errors/errors.jet` |
| [`features/errors/typed_error_families.jet`](features/errors/typed_error_families.jet) | One declared `impl Source -> Target` rail changes only the error carrier and preserves the success payload. | [`features/expected/errors/typed_error_families.out`](features/expected/errors/typed_error_families.out) | `target/debug/jet run Examples/features/errors/typed_error_families.jet` |
| [`features/io/scope_guard.jet`](features/io/scope_guard.jet) | A scope guard runs once at scope exit in reverse registration order, including plain-call failure propagation before the caller's `??` fallback; it remains distinct from resource-only `defer close`. | [`features/expected/io/scope_guard.out`](features/expected/io/scope_guard.out) | `target/debug/jet run Examples/features/io/scope_guard.jet` |

The adjacent [`list_predicate_ops.jet`](features/collections/list_predicate_ops.jet)
and [`indexed_mutation.jet`](features/collections/indexed_mutation.jet) examples
contrast eager reads with in-place writes. Their source and neighboring golden
files are the evidence; this manifest does not duplicate the collection API.

## Related executable contracts

- [`suites/`](suites/README.md) describes the suite fixtures and their golden
  checker.
- [`jetpack/executable-lease-recovery.md`](jetpack/executable-lease-recovery.md)
  documents the Jetpack lease-recovery command and its safety boundary.
- [`tests/golden.rs`](../tests/golden.rs) and
  [`tests/suite_goldens.rs`](../tests/suite_goldens.rs) are the source of truth
  for the two golden layouts.
