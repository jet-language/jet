# #3116: debugger and safe inspection for scripts and guests (SCRIPT-F26)

Closer10, 2026-09-29. Binaries:

- DAP runs: `~/.cache/jet-dev/safe-jet.sh` → `jet-debug-snapshot14`.
- Test runs: the prebuilt `target-integ/debug/deps/debug-baaa3fa51bba9e15`
  (built 2026-09-29 01:53). Its `CARGO_BIN_EXE_jet` is `target-integ/debug/jet`
  (2026-09-29 00:10).
- lldb 21.1.8 from `/nix/store/cnm8dkpvbiyjww622dn0ckmypax1jks3-lldb-21.1.8/bin`.
  It is not on PATH inside `Tools/agent/jet-env`, so it was prepended
  explicitly.

Verdict: **FAIL**.

## Method

1. Full debug suite with lldb present, so the DAP tests cannot return early:
   ```
   systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax=6G env TMPDIR=… PATH=<lldb>/bin:$PATH \
     Tools/agent/jet-env sh -c 'command -v lldb; lldb --version; target-integ/debug/deps/debug-baaa3fa51bba9e15 --test-threads=2'
   ```
   Log: `~/.cache/jet-dev/scratch/Closer10/debug_tests.log`. The first lines
   are `/nix/store/…lldb-21.1.8/bin/lldb` and `lldb version 21.1.8`.
2. A DAP session on a script. The driver is `~/.cache/jet-dev/scratch/Closer10/dap/drive.py`,
   adapted from ProveOther's. It runs initialize, launch, setBreakpoints at
   line 10, configurationDone, stackTrace/scopes/variables, a mutating
   `evaluate total = 999`, and then continue. The target is
   `dap/target.jet`; the log is `dap/log-target.jet-10.json`.

## Evidence

### Test suite (criterion 4): 25 passed, 14 failed

Passing: the interpreter session tests.

- `break_then_continue_stops_at_breakpoint`
- `step_stops_at_first_line_with_caret_and_locals`
- `finish_returns_to_caller`
- `next_steps_over`
- `paused_session_stops_at_a_live_command_boundary`
- `quit_ends_session_with_e2204`
- `unsupported_feature_stops_at_e2203_boundary`
- `canvas_debug_stop_requires_matching_source_revision_and_tier`
- `canvas_debug_session_replays_one_source_bound_session`

Failing:

- All seven `dap_cli_*` production-wire tests. Each fails in one of four
  ways:
  - "production DAP process exited before writing its debug map";
  - "couldn't create the `.jet/build` folder safely: web output directory
    escapes the working directory";
  - a launch-response timeout;
  - "production DAP pause did not settle".
- Native tier: `native_session_steps_and_shows_locals`,
  `line_markers_resolve_every_statement_to_its_source_line`,
  `debug_build_carries_line_markers_a_normal_build_does_not`, and
  `canvas_debug_native_tier_never_falls_back_to_interpreter`. All four fail
  with "native debugger could not find a source-mapped entry statement".
- Interpreter stepping:
  - `locals_and_print_show_jet_values`: stepping at a `loop i in 1..n {`
    header stops on the header again and never enters the body, and the
    program then finishes.
  - `loop_next_edges_are_step_visible_and_line_mapped`: "plain next stop".
  - `step_into_call_and_backtrace`: step-into stops on the callee signature
    line (`fn double(x: Int) -> Int {`, line 1), not the body. The `bt`
    output itself is correct: `#0 double() at …:1 / #1 main() at …:7`.

### DAP script session (criteria 1-3)

```
>> setBreakpoints line 10  << verified=true
<< event process {"name":"Jet","startMethod":"launch","isLocalProcess":true}
<< event stopped {"reason":"breakpoint","threadId":1,"hitBreakpointIds":[1]}
>> stackTrace  << success=false E2234 "the requested Jet task is no longer stopped"
>> continue    << success=false E2234 (same)
<< timeout (no terminated/exited event within 60 s)
```

This is the same failure the 2026-09-29 overnight run saw on an older
snapshot. The only difference: `continue` now answers E2234 rather than
killing the adapter.

- No message carries a source revision, process id or guest identity. The
  launch body is `{}`, and the process event has only name, startMethod and
  isLocalProcess.
- The frames, values and mutating-evaluate checks were never reached.

### Embedded guest

- `Examples/features/packages/library_loadable/host.jet` cannot be compiled
  on any tier: an internal compiler error, see
  `3114-embedding-two-adapter-delivery.md`.
- `sandbox_mathkit/run.jet` fails at check with E1257, a stale interface
  snapshot.
- So a guest debug session is unavailable. The cause is the build failure,
  not the debugger.

## Capability matrix (observed)

| Operation | Script, interpreter (`jet debug`) | Script, native (`jet debug`, lldb) | Script, DAP | Guest (native Library / Component) |
|---|---|---|---|---|
| Breakpoint | supported | fails: no source-mapped entry | verified and stops, but the stop is lost (E2234) | unavailable: host does not build |
| Step / next | supported; loop bodies not entered | fails | not reached | unavailable |
| Step into | lands on the signature line, not the body | fails | not reached | unavailable |
| Frames / backtrace | supported (`bt`) | fails | E2234 | unavailable |
| Locals / print | supported (`locals`, `p total`) | fails | not reached | unavailable |
| Mutating evaluate | not probed (no `set` verb seen) | n/a | not reached | unavailable |
| Attach | n/a | n/a | `dap_cli_attaches…` fails | unavailable |
| Revision/process identity in receipt | canvas debug binds the source revision (test passes) | n/a | absent from launch/process/stopped events | unavailable |

## Criteria

1. **Fail.** Only the interpreter tier binds stops to Jet source and revision
   (canvas tests). DAP and native lose the stop.
2. **Not reached.** No mutating operation could be exercised on the DAP
   path.
3. **Fail.** Unavailable operations do carry explicit codes (E2234, E2203),
   but the receipts hold no process, guest or revision identity.
4. **Fail.** `debug` has 14 of 39 tests failing with lldb present.

## Follow-up cards Pip should file

- DAP stop lost after `stopped`: E2234 on stackTrace/continue. This is also
  the 7 `dap_cli_*` failures.
- Native debugger "could not find a source-mapped entry statement" (4
  tests).
- Interpreter step does not enter `loop` bodies, and step-into lands on the
  signature line (3 tests).
- DAP launch/stopped events carry no revision, process or guest identity.
- `Tools/agent/jet-env` does not put lldb on PATH, so the plan's proof
  command silently skips the DAP tests unless PATH is extended.
