# Examples

`canon.jet` is the compiling syntax showcase. `features/` groups every feature
example by topic (D-REPO-EXAMPLES1); `features/expected/` mirrors the tree with
each example's golden output. `suites/` contains complete user-story programs;
its [README](suites/README.md) lists the stories and their goldens. Run any
example directly:

```
jet run examples/features/basics/hello.jet
```

Walk every executable under `features/` with `jet run` and report passed,
failed, and output mismatches:

```
scripts/agent/run-feature-examples.mjs
```

A path substring selects a subset: `scripts/agent/run-feature-examples.mjs basics`.

The [executable lease recovery example](jetpack/executable-lease-recovery.md)
shows the read-only audit and explicit Hangar recovery boundary for a stale
process-tree lease.

For the complete new-project workflow, use the [first-hour guide](../docs/spec/guides/first-hour.md).
The explicit path above is the standalone-example form.

## Short path first (D-EXAMPLES-SHORTPATH1=A)

Flagship examples teach the **magic default** first. Manual mechanics stay in a
clearly labelled expert sibling (`*_expert.jet`) beside the flagship — never as
the only path.

| Teach first | Keep beside it as expert |
|---|---|
| `#CLI` typed entry args | raw `process.argv()` walks |
| `para_map` / `task.group` | hand-rolled channels + `task` + join |
| streaming `files.open(…).lines()` | materializing `String.lines()` for file-scale work |

New examples should land the same way: beginner default in the named flagship,
long path only as an expert variant when the manual form is still worth teaching.

Start with the [onboarding example](features/basics/onboarding/run.jet) after
`jet new`: it is a normal `run.jet` project entry and teaches the same
`new` -> `run` -> `check` -> `test` -> `fix` -> `explain` workflow as the first-hour
guide.

## Genre marker (one rule)

Every example under `features/` is a teaching example first — it explains a
mechanism, not just a fact that a card closed. An example whose header is
nothing but a bare `#NNNN: <what shipped>` note, with no ratified decision ID
and no explanation of the mechanism, is a closure ledger entry wearing a
teaching example's clothes. Mark it: prefix the card number with the word
`ledger` (`// ledger #1477: remaining List surface after #1410/#1479.`). A
reader then knows at a glance this file exists to prove a ledger item closed,
not to teach — citing a card number *alongside* a decision ID or real
explanation (the norm across this corpus) needs no marker; only the bare,
unexplained case does.

## Current-law teaching manifest (READ-F08 / READ-F12)

This finite manifest covers only the selected examples whose local contracts
are easy to lose in a short name or snippet. The current spellings come from
the [Syntax decisions](../docs/spec/syntax-decisions.md); retained research
captures, including the corrected `loop_forms`, `bounded_workers`, and
`all_failfast` source identities, remain historical evidence rather than
replacement source files.
The local-contract criteria come from [READ-F12](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f12);
the example boundary and source-integrity rules come from [READ-F08](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f08).

| Source | Local contract made visible | Normative decisions |
|---|---|---|
| `features/basics/loop_forms.jet` | One `loop` keyword; source and range evaluation is eager, `next` advances the current source, and iteration reads rather than mutates its source. | S19, S22, S23; D-LOOP-IN1, D-RANGE-EXCL1, D-LOOP-CONTROLWORD1 |
| `../tests/ui/list_loop_mutate.jet` | A mutable binding does not authorize changing a collection while that collection is being read. E0507 leaves the traversal domain and repair intent explicit. | D-BIND1, D-MEM1; E0507 |
| `features/errors/errors.jet` | `Ok`/`Err` exits, unmarked propagation, `??` fallback recovery, and an explicit result-pattern handler are four different routes. | S34, S35, S36, S7; D-FAILURE-FOUNDATION1, D-FAIL-EXIT1 |
| `features/errors/typed_error_families.jet` | One declared `impl Source -> Target` rail changes the error carrier while preserving the success payload; it is not an implicit logging or alias mechanism. | D-ERR-CONV, D-FAIL-CONV1, D-FAIL-CONV2 |
| `features/io/scope_guard.jet` | A registered guard callback is deferred, runs once at scope exit in reverse registration order, and remains distinct from resource-only `defer close(^resource)`. | D-DEFER1, D-SHAPE-RESOURCE2, D-FAIL-EXIT1 |

The adjacent collection examples (`collections/list_predicate_ops.jet` and
`collections/indexed_mutation.jet`) distinguish an eager read from an
in-place write and show the observed storage update. Collection API
completeness belongs to [IteratorAstra's iterator work](../docs/research/mine-for-jet-2026-09-12.md#topic-iterators);
this manifest does not create a second API inventory. Identity and refactoring
claims belong to [BigCodeAstra's checked identity work](../docs/research/mine-for-jet-2026-09-12.md#topic-bigcode).
Runtime scope cleanup is covered separately by [READ-F05](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f05),
and formatter preservation by [READ-F07](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f07);
this patch keeps their receipts distinct from teaching-source conformance.
Current-law example conformance is the boundary of
[READ-F08](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f08).

### Source identity and output shape

`features/basics/values.jet` uses the current interpolation form. An output
line is not a source-search key: source wrapping does not add a newline, while
the explicit `\n` escape does. E0109's existing interpolation diagnostic and
its source-specific operands teach the one text-composition mechanism; this
does not authorize a logging or localization framework. The
[message-identity investigation](../docs/research/mine-for-jet-2026-09-12.md#finding-read-f11)
keeps source identity, event keys, trace context, and message text separate.

### Full-state C contrast

The visible C control `while (*dest++ = *src++);` stores the assigned byte,
uses the copied NUL byte as its termination condition, and post-increments
both pointers. The reviewed expanded form can copy the same bytes, including
the NUL terminator, under ordinary valid nonvolatile source/destination
storage, but it leaves the final pointers at different positions. A truthful
comparison therefore exposes postconditions, capacity/bounds, alias
obligations, and the nonvolatile assumption; matching printed bytes alone is
not equivalence. This is a teaching control, not an executed enclosing
program or proof of the earlier unsafe rewrite. See
[READ-C015](../docs/research/mine-for-jet-2026-09-12.md#claim-c-copy-refactor-termination-and-capacity-read-c015)
and [READ-C028](../docs/research/mine-for-jet-2026-09-12.md#claim-c-copy-refactor-observable-state-read-c028).

## Auxiliary golden stream suffixes (one rule per meaning)

`features/expected/<stem>.out` always holds the plain `jet run` stdout. Every
other suffix under `features/expected/` names a distinct proof, never an ad
hoc pick:

- `<stem>.err.out` — the example is expected to fail (panic, exit 70, or an
  uncaught `Err`, exit 1). Its presence tells `tests/golden.rs` to require a
  non-zero exit.
- `<stem>.stderr.out` — the example is expected to **succeed** (exit 0) but
  still writes incidental stderr (warnings, progress). Optional; add it only
  when a passing example's stderr must stay pinned.
- `<stem>.web.out` — stdout captured running the example under the web/wasm
  target instead of native. Read by `tests/web_build.rs` and
  `tests/web_examples_doc.rs`.
- `<stem>.harness.out` — output from the DOM click/interaction test harness
  for a web example, not plain stdout. Read by `tests/web_build.rs` and
  `tests/web_examples_doc.rs`.
- `<stem>.seed.out` / `<stem>.greet.out` — an example with more than one
  named `#Job` job (D-JPK-TASKRUN1) uses the job name as the suffix,
  keyed per job rather than per stream. Read by `tests/golden.rs` and
  `tests/dev.rs` for `devloop/job_runner`.
- `<stem>.test.out` — the pinned report from running `jet test` on the
  example itself, not the example's own stdout. Read by `tests/jet_test.rs`.
- `<stem>.fuzz.out` — the pinned report from running `jet fuzz` on the
  example. Read by `tests/jet_test.rs`.

Never add a suffix ad hoc: reuse one of the meanings above, or extend this
list in the same commit that adds a new one.

Suggested learning order:

| Topic | What lives there |
|---|---|
| `basics/` | hello, functions, values, branches, loops, closures, pattern matching |
| `types/` | structs, enums, traits, generics, distinct types, typestate, tuples |
| `errors/` | error families, `?` propagation, panic, rollback, discard rules |
| `collections/` | lists, maps, sets, deques, iter adapters, parallel iteration |
| `text/` | strings, regex, unicode, hex/base64, streaming file parse |
| `math/` | numeric floor — libm, checked/saturating/wrapping integer families |
| `modules/` | imports, module files/dirs, packages, visibility, re-export |
| `comptime/` | comptime blocks, splice, reflect, embed, doctests |
| `effects/` | access sigils, taint, pure, effect prohibition, grants |
| `memory/` | ownership, arenas, stored refs, rawptr, uninit, zero-copy, GC |
| `serde/` | json/csv/toml/yaml, derives, schema migrations, fidelity |
| `io/` | cli, files, stdin, paths, logging, terminal |
| `net/` | http client/server, routes |
| `concurrency/` | tasks, channels, select, race/cancel, deadlines, scheduler |
| `crypto/` | envelope, signing, key migration |
| `ui/` | view tree, styles, component kit, motion, a11y, reactive TUI |
| `web/` | hybrid JS DOM + Wasm compute — see `docs/spec/reference/web-backend-wasm.md` for the full example index, build commands, and unsupported-breadth list |
| `lowlevel/` | ffi, c layout, simd, freestanding, MMIO board writes, cross-compile |
| `tooling/` | tests, measurement, debug, property tests, build profiles |
