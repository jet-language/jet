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

Public CLI scenarios live in `tests/cases/**/*.case` and run once in fresh
scratch directories through the same step parser as golden sidecars:

```sh
jet build Tools/testkit
testkit cases --jet /absolute/path/to/jet --scratch /outside/checkout
```

Each scenario may stage a `fixture:` directory relative to its `.case` file;
exact stdout, stderr, and file snapshots live beside the scenario. Program
fixtures belong outside `Tools/testkit`, whose `.jet` files are package members.

Label a process step with `step NAME:` and compare normalized streams with
`stdout: == NAME` or `stderr: == NAME`. Labels must be unique, and stream
comparisons name a prior step. `file: A == B` and `file: A != B` compare raw
bytes, including non-UTF-8 files. Untransformed file substring, prefix,
line, count and absence predicates also inspect raw bytes before UTF-8
decoding. `copy: A B` preserves the source permission bits; destinations and
`remove: A` stay work-relative. Failed file operations report `ERROR`.
Setup belongs to kit actions rather than platform shell commands. `mkdir:`
creates parents, while link and rename actions preserve filesystem identity.
Directory selection uses OS listing order unless the assertion explicitly
compares sorted names. `await: file PATH timeout 30s` bounds readiness without
joining the peer process; an expired readiness predicate is `FAIL`, and a
failed filesystem operation is `ERROR`. A missing sparse-resize seam is
`REFUSED`, never an emulated sparse file.

An `env: NAME=VALUE` before the first step applies to the case. After a step,
it applies only to that step; `unset: NAME` removes a variable after all
environment overlays. `cwd: PATH` sets that step's working directory under
`{work}`. `built` invokes the most recent successful `jet build` artifact;
using it without a preceding build is a parse error.
In argv only, `{env:NAME|default}` reads the effective environment after
those overlays and unsets; unset or empty values expand to `default`.

`env-clear: true` starts a process step without the caller's environment or
kit defaults; explicit case and step `env:` overlays still apply, followed
by `unset:`. On Linux, `isolate: no-nix` hides the host Nix store and disables
networking for that step. Use `isolate: no-nix network=enabled` for an online
phase. Both modes stage the executable's runtime closure outside `/nix`,
hold the ELF loader on `/proc/self/fd/9`, and re-exec the resolved argv after
mounting an empty tmpfs over `/nix`. Missing Linux or `unshare` is `SKIP-HOST`;
namespace or runtime-staging failure is `ERROR`, never an unisolated run.

The no-Nix cached-entry and signed-index repair cases use isolated offline
steps. The repository compiler dogfood case is opt-in: set
`JETPACK_DOGFOOD=1` and `JETPACK_DOGFOOD_TARGET_DIR` to an existing absolute,
disk-backed target directory outside `/tmp`. It uses a fresh kit scratch
project and Hangar, then replays the same lock without network or catalog
discovery; it never treats a warm package store as the cold proof.

`limit: time=SECONDS memory=KiB` bounds a step, with an explicit limit hit
reported as `FAIL`. `elapsed: B <= FACTOR * A` compares captured monotonic
durations for labelled steps. `normalize: version` replaces the exact
version reported by the supplied compiler with `VERSION`; an unreadable
version banner reports `ERROR`, not a guessed version.
`elapsed: median [A,B,C] <= K * median [D,E,F]` compares the middle
nanosecond receipt from each three-step list using saturating integer
multiplication; `<` is strict and `elapsed: median [A,B,C] > 0ns` requires
a positive median. Labels must already exist.

Captured text supports `cap:X: is-f64 # original predicate` and
`cap:A: f64 < {cap:B} # original predicate` (also `>`, `<=`, `>=`, `==`,
`!=`). Parsing failures fail, and comparisons preserve IEEE floating-point
semantics, including NaN. `capture: N = size file PATH` captures byte length;
`cap:A: delta {cap:B} <= K # original predicate` bounds absolute integer
difference. `.bit_and(0o777)` masks each integer line; `.skip_lines(N)` uses
Rust line splitting and joins the remainder with LF, without a final LF.
The journey and source-arrow normalizers strip CRLF carriage returns while
preserving whether the original text ended in LF.

`requires: path-exists:PATH` skips absent host paths. A header such as
`requires: first-existing TOOL = "A" | "B"` captures the first present path
as `{cap:TOOL}`, or reports `SKIP-HOST` when no alternative exists.


Structured assertions use `json: stdout|stderr doc|ndjson PATH OP VALUE`, or
`json: file FILE doc|ndjson PATH OP VALUE`. Paths support `$`, `.key`, `[i]`,
and `[*]` (every selected record must match, and an empty selection fails).
Operators are `=` for typed JSON equality, `:` for a type, `#=` for a count,
and `has` for an array element containing all keys in a nonempty JSON object,
or exactly equal to a scalar. `any ~ "TEXT" # original predicate` instead
looks for a string array element containing the substring; nonstrings do not
match. Same-document `!= $.OTHER` compares deeply, and `#== $.OTHER`
compares array/object counts with the other path in the same document.
Object key order is not significant; array order is significant for `=`.
Every partial predicate needs a nonempty same-line `#` reason, including
every `: TYPE` assertion. Use `# original predicate` for an original
partial assertion, or `# volatile: WHAT` for nondeterministic output.
Subject transforms belong on the key (`stdout.trim: = result.out`), not
in the value. JSON data assertions have the single `json:` spelling.

`normalize: json stdout|stderr PATH : TYPE # reason` replaces each selected
typed value with the JSON string `"<<TYPE>>"` before snapshot comparison.
All other bytes remain unchanged; malformed JSON, a missing path, or a wrong
type fails. Before the first step this applies case-wide; after a step it
attaches only to that step. Typed `json:` assertions still inspect the
original values. No stream is silently skipped merely because it is not JSON.

`json: SELECTOR doc $ valid # original predicate` checks only that the
subject is one valid JSON document. Every JSON type is accepted; empty or
invalid text fails, without requiring an object or canonical formatting.

For `normalize: replace FROM "TO"`, an unquoted FROM expands runtime
tokens; a quoted FROM stays literal and matches the token-normalized stream.
TO is always literal, including braces that resemble capture or path tokens.
Text captures may contain an empty string. Optional byte captures retain
their byte identity (including absent versus empty) and cannot be expanded
as scalar argv or environment text.

Persistent `session NAME raw|line|content-length:` steps keep stdin open;
`stdin: null` closes it immediately. `close NAME` closes stdin and joins the
child, while `wait NAME` joins without closing stdin. Both publish the final
unread stdout/stderr remainder and the labelled receipt. `signal NAME: KILL`
records an expected stop rather than an unexpected signal error. Framed
`recv` predicates, including accumulated `collect until` conjunctions, are
validated before reporting `REFUSED read_bytes` when the raw-read seam is
unavailable; buffered final output is never presented as a live frame.

Bounded `await: file|dir|entry` polls filesystem readiness. `poll NAME:`
reruns its command until all `until` conditions match on the same attempt,
or fails at its deadline. Conditions are not accumulated across attempts.

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

[`view_return_lifetimes.jet`](features/memory/view_return_lifetimes.jet) shows
borrowed returns from named owners and direct operations on the returned view.
The caller keeps the owner alive; binding the returned view first is optional.

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
