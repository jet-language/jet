# Examples

`Examples/` is the runnable source library for Jet. Each lesson is a small
program that runs from the repository root, and its expected output sits beside
it. The source and that output, rather than this index, define the behavior.

## Areas

Read the areas in this order. Each area's `README.md` lists its lessons, and
each lesson teaches the safe default first.

| # | Area | What it covers |
| --- | --- | --- |
| 1 | [Basics](Basics/) | Values, output, loops, branches and functions: the shortest path through Jet. |
| 2 | [Types](Types/) | Structs, enums, optionals, traits, patterns and units of measure. |
| 3 | [Collections](Collections/) | Lists, maps, sets, queues and the iterator adapters over them. |
| 4 | [Text](Text/) | Strings, formatting, Unicode, regular expressions and checked text. |
| 5 | [Math](Math/) | Numbers, exact arithmetic, operators, randomness and numeric compute. |
| 6 | [Memory](Memory/) | Ownership, views, shared state, allocation and the unsafe tier. |
| 7 | [ErrorsAndEffects](ErrorsAndEffects/) | Failures and how they travel, effects and authority, and contracts. |
| 8 | [Metaprogramming](Metaprogramming/) | Compile-time code, reflection, generics, derives, attributes and build facts. |
| 9 | [Modules](Modules/) | Modules, packages, imports and the Jetpack package manager. |
| 10 | [Concurrency](Concurrency/) | Tasks, channels, cancellation, parallel work and services. |
| 11 | [Systems](Systems/) | Files, processes, the terminal, time and command-line programs. |
| 12 | [LowLevel](LowLevel/) | Sized numbers, layout, SIMD, inline assembly, freestanding and embedded targets. |
| 13 | [Data](Data/) | JSON and other formats, binary data, dynamic values, databases and analysis. |
| 14 | [Network](Network/) | HTTP clients and servers, sockets, URLs and browser automation. |
| 15 | [Crypto](Crypto/) | Hashing, signing, sealed envelopes, keys and authentication. |
| 16 | [Interop](Interop/) | Calling C and Rust, foreign projections, and programs in other languages. |
| 17 | [Apps](Apps/) | Interfaces, web apps, games and larger programs. |
| 18 | [Tooling](Tooling/) | Tests, debugging, the dev loop, builds, performance and developer tools. |

Start with [`Basics/Hello.jet`](Basics/Hello.jet), then
[`Basics/FirstHour.jet`](Basics/FirstHour.jet) and the
[`Basics/Onboarding/`](Basics/Onboarding/) project, which is the next step
after `jet new`. The complete `new` → `run` → `check` → `test` → `fix` →
`explain` workflow is in the [first-hour guide](../Docs/spec/guides/first-hour.md).

Regression and crash-reproducer programs do not live here; they are in
[`tests/run`](../tests/run/) and [`tests/cases`](../tests/cases/).

## Run an example

Use the admitted compiler (`jet` on your `PATH`, or `Tools/agent/JetEnv.jet jet`):

```sh
jet run Examples/Basics/Hello.jet
```

The golden suite exercises every lesson in one pass and records the
command, result, and output for each:

```sh
jet test Compiler --profile golden
```

It runs each golden at every optimization level: `none` is `jet run`, and
`basic` and `full` check the same golden through `jet build` and the built
binary (same stdin, arguments and working directory). `--optimize=none`
narrows a run to the first, `--filter=Basics/` to one area.

The suite reads the central known-failing ledger (`tests/known-failing.tsv`)
and a `# KNOWN-FAILING` header in an example's `.case`. An owned failure
reports `KNOWN-FAILING`; an owned example that passes reports
`UNEXPECTED-PASS` and fails the run until its row is removed.

For the contributor workflow around the executable and its environment, see
[`AGENTS.md`](../AGENTS.md) and [`Tools/agent/JetEnv.jet`](../Tools/agent/JetEnv.jet).

## Layout and goldens

A lesson is either one source file, `Area/Lesson.jet`, or a project directory,
`Area/Lesson/` with a `run.jet`. Its expected stdout sits beside it as
`Area/Lesson.out` in both cases. A directory that holds only a `package.jet` or
a `workspace.jet` is a package the lessons beside it use, not a lesson. The
golden suite (`Compiler/JetTestKit/Source/Golden.jet`) walks all of
`Examples/`, so every lesson needs its golden.

The native checker compares stdout byte for byte and uses `.out`, `.err.out`,
and `.stderr.out`; the testkit golden suite also checks `.test.out`. Dedicated
web, harness, fuzz, and input-specific checks consume the other suffixes:

| Fixture | Meaning |
| --- | --- |
| `<stem>.out` | Expected native stdout. |
| `<stem>.err.out` | Expected stderr for failing execution; ratified runtime faults exit 70, source-proved unhandled/fatal entries exit 1. |
| `<stem>.stderr.out` | Expected stderr while execution exits successfully. |
| `<stem>.web.out` | Web-target output used by the corresponding web fixture. |
| `<stem>.harness.out` | Harness-managed output. |
| `<stem>.seed.out` / `<stem>.greet.out` | Input-specific output for the matching example. |
| `<stem>.test.out` | `jet test` stdout, checked by the testkit golden suite on every tier after the run passes. |
| `<stem>.fuzz.out` | Fuzz-command output. |
| `<stem>.case` | Steps, environment, and host requirements for a golden that needs more than `jet run`. |

Failing runs compare stdout as well as stderr and the exact exit code. A
failure's sibling `.out` is authoritative; without one, stdout must be empty.
Unknown failure contracts and contradictory exit claims in an oracle are
missing exit proofs, not permission to accept an arbitrary nonzero status.
These rules apply to the testkit golden suite.

A lesson that needs a host tool, such as a foreign compiler, declares
`requires: tool:<name>` in its `.case`; the golden is skipped where the tool is
missing. A lesson that binds a fixed TCP port declares `host-port: N`; the
testkit holds that port's host-wide lock while it runs, so concurrent runs on
one host take turns instead of failing to bind.

A lesson that reads files by their repository path, such as
[`Systems/Files/Path.jet`](Systems/Files/Path.jet) walking `Examples/Systems`,
declares `repo-cwd: <reason>` once, before any steps, in its `.case`. The
testkit then runs the built program from the root of its staged source copy,
which keeps the repository layout, instead of from the project directory.
The checkout is never written, and `file:` assertions stay relative to the
project. Only goldens take the header; a `tests/cases` case refuses it.

Examples that depend on environment or process identity should print checked
relationships instead of raw paths or process IDs. For example,
[`Systems/Process/SystemProjection.jet`](Systems/Process/SystemProjection.jet)
compares the home directory with its environment source and checks child exit
statuses, preserving the system calls while keeping the stdout contract
independent of the host.

The golden checker is the executable definition of each mapping. Keep source
changes and their goldens together; do not use this README as a second list of
expected output.

## Short path first

Each lesson teaches the safe default first. When the manual mechanics are
worth keeping, place them in a clearly labelled `*Expert` sibling rather than
making the long form the only path.

| Teach first | Keep beside it as expert |
| --- | --- |
| `#CLI` typed entry arguments | Raw `process.args()` walks |
| `para_map` / `task.group` | Hand-rolled channels, tasks, and joins |
| `files.open(...).lines()` streaming | Materializing all file text before processing |

[`Basics/FirstHour.jet`](Basics/FirstHour.jet) and its
[`Basics/FirstHourExpert.jet`](Basics/FirstHourExpert.jet) sibling show this
rule for typed CLI arguments. New examples should use the beginner default
first and keep an expert sibling only when the manual form still teaches
something important.

A lesson explains a mechanism, not merely that a work item closed. Programs
kept only as evidence that a card closed belong in `tests/run` or
`tests/cases`.

## Current-law teaching manifest

This finite manifest ties seven teaching sources to the rule each shows, the
[syntax decisions](../Docs/spec/syntax-decisions.md) entries that make that
rule normative, the checked-in UI or golden snapshot, and a command that runs
the same source.

| Source | Rule it demonstrates | Normative decisions | UI/snapshot check | Command |
| --- | --- | --- | --- | --- |
| [`Basics/LoopForms.jet`](Basics/LoopForms.jet) | One `loop` keyword covers infinite, conditional, source, list-pair, map-pair and index-only loops, inclusive and exclusive ranges, strides and countdowns, and eager value-position collection; `next` advances the source and `break` leaves. | S19 loops (D-LOOP-HEADER2, D-LOOP-ADVANCE2, D-LOOPEVAL1, D-COMPREHENSION1, D-LOOP-COMMA1, D-LOOP-IN1), D-LOOP-SUBJECT1, S22/S72 ranges with D-RANGE-EXCL1 and D-RANGE-BACKWARD1, S23 loop control | [`Basics/LoopForms.out`](Basics/LoopForms.out) | `jet run Examples/Basics/LoopForms.jet` |
| [`tests/ui/list_loop_mutate.jet`](../tests/ui/list_loop_mutate.jet) | While a loop reads `nums`, its body may not change `nums`; E0507 asks for one intent: a separate result, a fixed snapshot, or an explicit work queue. The recorded repair [`list_loop_mutate.fixed.jet`](../tests/ui/list_loop_mutate.fixed.jet) is the legal form: a fixed index domain computed once. | S19 (the source is evaluated once and read for the whole body, D-LOOPEVAL1); E0507 `jet.CollectionChangedWhileRead` | [`tests/ui/list_loop_mutate.stderr`](../tests/ui/list_loop_mutate.stderr) | `jet check tests/ui/list_loop_mutate.jet` |
| [`ErrorsAndEffects/Errors/Errors.jet`](ErrorsAndEffects/Errors/Errors.jet) | `Ok`/`Err`, unmarked propagation, `??` fallback recovery, and an explicit result-pattern table are distinct routes. | S7 propagation, S34 failure-returning return, S71 `??` fallback, D-FAIL-MODEL1, D-FAIL-CARRIER1, S68 `if` arm tables | [`ErrorsAndEffects/Errors/Errors.out`](ErrorsAndEffects/Errors/Errors.out) | `jet run Examples/ErrorsAndEffects/Errors/Errors.jet` |
| [`tests/ui/fallible_entry_unhandled.jet`](../tests/ui/fallible_entry_unhandled.jet) | An unmarked fallible call propagates only into a caller whose failure type it can reach; in a `run` that names no failure type, `ParseError` has no conversion into the default `Err`, so E2402 rejects the call. The recorded repair [`fallible_entry_unhandled.fixed.jet`](../tests/ui/fallible_entry_unhandled.fixed.jet) recovers locally with `??`. | S7 propagation, S71 `??` fallback, D-FAIL-CONV1; E2402 `jet.MissingErrorConversion` | [`tests/ui/fallible_entry_unhandled.stderr`](../tests/ui/fallible_entry_unhandled.stderr) | `jet check tests/ui/fallible_entry_unhandled.jet` |
| [`ErrorsAndEffects/Errors/TypedErrorFamilies.jet`](ErrorsAndEffects/Errors/TypedErrorFamilies.jet) | One declared `impl Source -> Target` rail changes only the error carrier and preserves the success payload. | D-FAIL-CONV1, S7 propagation, S71 `??` fallback | [`ErrorsAndEffects/Errors/TypedErrorFamilies.out`](ErrorsAndEffects/Errors/TypedErrorFamilies.out) | `jet run Examples/ErrorsAndEffects/Errors/TypedErrorFamilies.jet` |
| [`Memory/ScopeGuard.jet`](Memory/ScopeGuard.jet) | A scope guard runs once at scope exit in reverse registration order, including plain-call failure propagation before the caller's `??` fallback; it remains distinct from resource-only `defer close`. | S63 / D-SHAPE-RESOURCE1 / D-SHAPE-RESOURCE2 resource cleanup, S7 propagation, S71 `??` fallback | [`Memory/ScopeGuard.out`](Memory/ScopeGuard.out) | `jet run Examples/Memory/ScopeGuard.jet` |
| [`tests/ui/scope_guard_discarded.jet`](../tests/ui/scope_guard_discarded.jet) | A `ScopeGuard` left as a bare statement is dropped at once, so E0419 rejects it. The recorded repair [`scope_guard_discarded.fixed.jet`](../tests/ui/scope_guard_discarded.fixed.jet) binds it, so the callback runs when the scope exits. | S63 resource cleanup; E0419 `jet.MustUseIgnored` | [`tests/ui/scope_guard_discarded.stderr`](../tests/ui/scope_guard_discarded.stderr) | `jet check tests/ui/scope_guard_discarded.jet` |

[`Basics/StructCallWord.jet`](Basics/StructCallWord.jet) demonstrates
fixed-width FNV results in record fields, direct and wrapper calls, and record
copies. Its [golden](Basics/StructCallWord.out) preserves all 64 bits
independently of the exact `Int` representation.

[`Memory/Views/ViewReturnLifetimes.jet`](Memory/Views/ViewReturnLifetimes.jet)
shows borrowed returns from named owners and direct operations on the returned
view. The caller keeps the owner alive; binding the returned view first is
optional. [`Memory/Views/ViewSubranges.jet`](Memory/Views/ViewSubranges.jet)
shows subranges of declared views: `view[a..<b]` keeps the view's owner, never
copies, and writes through a `ViewMut` subrange land in the owning list.

The adjacent [`Collections/ListPredicateOps.jet`](Collections/ListPredicateOps.jet)
and [`Collections/IndexedMutation.jet`](Collections/IndexedMutation.jet)
examples contrast eager reads with in-place writes. Their source and
neighboring golden files are the evidence; this manifest does not duplicate the
collection API.

## Programs

[`Apps/Programs/`](Apps/Programs/) holds eight small end-to-end programs, each
a top-level `.jet` with its `.out` beside it: expense report, support queue,
inventory reorder, dispatch, failure, finite state, ownership, and wire output.
Each opens with the anti-pattern it replaces and the rule it follows. The case
[`tests/cases/suite_goldens/user_story_suite_matches_expected_output.case`](../tests/cases/suite_goldens/user_story_suite_matches_expected_output.case)
runs each with `jet run` and compares stdout byte for byte.

## Related executable contracts

- [`Modules/Jetpack/ExecutableLeaseRecovery.md`](Modules/Jetpack/ExecutableLeaseRecovery.md)
  documents the Jetpack lease-recovery command and its safety boundary.
- [`Compiler/JetTestKit/Source/Golden.jet`](../Compiler/JetTestKit/Source/Golden.jet)
  is the source of truth for the golden layout.
