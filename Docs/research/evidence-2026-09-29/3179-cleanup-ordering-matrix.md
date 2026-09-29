# #3179 — Combined deferred-close, guard and exit-handler ordering (READ-F14)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`
(the one source-matched build pinned for this card), source rev `e9c708fa7`.
Author: Closer03 (evidence closer). No compiler, runtime or Core change was made.

## Oracle

- D-SHAPE-RESOURCE1/2 (#557/#647, `Docs/spec/syntax-decisions.md:2442-2457`): `close(^r)`
  is the one consuming close; `defer close(^r)` runs deferred closes in reverse order on
  every scope exit (success, error, panic, cancellation); an immediate close consumes at
  the call, a deferred close when it runs; the move checker rejects later use, an
  immediate close after a deferred one, or a second defer; `finish`/`commit`/`flush`/
  `shutdown` stay ordinary fallible methods.
- D-FAIL-EXIT1 (`Docs/spec/reference/core-library.md:1638-1644`, `Docs/spec/spec.md:799-805`):
  deferred closes in reverse declaration order, then scope guards in reverse
  registration order, then `os.atexit` handlers in registration order; `process.exit` and
  `os.stop` share the explicit exit boundary; work registered after the stop does not
  run; a host kill or abort does not promise these finalizers.
- The e841 guard failures (READ-P06, `mine-for-jet-2026-09-12.md:2451`) are kept
  separate: on frozen binary `e841…` AOT closed in declaration order and default omitted
  closes. They are historical; this card re-observes on snapshot14 only.

## Method

- Witness `Examples/features/io/cleanup_matrix.jet`: the parent relaunches itself through
  `process.cmd([os.executable(), <case>])` for return, error, panic, cancel (race loser at
  `time.sleep`), `process.exit(0)` and `os.stop(3)`; each child interleaves two deferred
  closes with two scope guards and registers two `os.atexit` handlers.
- Move/contract probes in `~/.cache/jet-test-scratch/Closer03/`: `close_use_after.jet`,
  `close_fallible.jet`; UI fixture `tests/ui/defer_close_twice.jet`.
- Default `jet run --allow=Exec,IO,Env,Time.Wait`; interpreter where it can run.

## Evidence

**The matrix cannot compile on snapshot14.** Default run:

```
Error [E1004]: `core.sys` has no item `atexit`
  --> cleanup_matrix.jet:68:12
```

This agrees with `core-library.md:1500-1501` ("The `core.sys` module exports neither
`on_interrupt` nor `atexit`") and contradicts the cleanup law in the same document
(:1638-1644), `spec.md:803` (`os.atexit` handlers run in registration order) and the
existing goldens `io/process_exit_cleanup` and `io/os_stop_cleanup`, which both call
`os.atexit` and so cannot compile on this binary either (inferred from the same E1004;
not re-run). Earlier attempts also hit E2404: `os.stop` has an inferred `!Err` contract
that cannot flow into a typed-error function. The interpreter cannot run the self-relaunch
at all: `core.sys.executable()` is E0956.

Move and contract checks (criterion 4), `--interpret`:

- Use after an immediate close: `close(^resource)` then `resource.name` →
  `E0121 resource was consumed by close, so it can't be used here`.
- Fallible close: `impl Writer.Close { fn close(^self) FlushFail! {…} }` →
  `E0907 close doesn't match the Close contract` (write `fn close(^self)`). The `Close`
  contract is infallible, so finish/flush failures cannot hide behind automatic cleanup;
  they must stay ordinary fallible methods, as D-SHAPE-RESOURCE2 requires.
- Second close after `defer close(^r)` (`tests/ui/defer_close_twice.jet`): diagnostic not
  observed (not run).

## Verdict

PARTIAL. Criterion 1 is met: oracle pinned, e841 kept separate. Criterion 6 is met: guard
lifetime stays with READ-F05, and domain lifecycles are untouched. Criterion 4 is partly
met: later use is rejected (E0121) and fallible close is impossible (E0907); the
second-close case is unobserved. Criteria 2, 3, 5 and 7 are not met: the matrix cannot
compile because `os.atexit` is not exported.

The next step is an owner decision: either restore `os.atexit` (spec.md:803,
core-library.md:1640) or remove it from the law, `spec.md` and the two goldens
(core-library.md:1500). The matrix example must then drop or keep its atexit cases to match.
