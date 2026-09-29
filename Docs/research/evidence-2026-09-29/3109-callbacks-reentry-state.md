# #3109 — host callbacks, re-entry and state ownership (criterion 3 / 4)

Closer07, 2026-09-29.

## Native Library row (criteria 1–2 re-confirmation)
Command:
```
systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax=6G timeout 1200 Tools/agent/jet-env target-integ/debug/deps/library_outputs-8e639e3daf6686f0 guest_embedding_contract_covers_lifecycle_threads_reentry_and_panic --exact --nocapture
```
Result: `test result: ok. 1 passed; 0 failed … finished in 30.68s`.
- Test binary sha256 prefix `616ac4dfa6fa0a07`, built 2026-09-29 01:58.
- It drives the `jet` it finds in the `target-integ` tree; `target-integ/debug/jet` has sha256 prefix `b96bb1755d9faf46` as of 02:26.
- Without `jet-env` it fails immediately: "guest embedding proof requires rustc".

This re-confirms, on a current integration build, the fixture's cells:
- 3 dlopen/dlclose cycles;
- two-thread × 1000 calls;
- host→guest→host re-entry (`reenter(41) == 1042`);
- Text release before unload;
- panic → child status 70 with `Stop [E3001]`.

The Closer07 #3110 retention witness adds 10k re-entrant calls and 256 native-held Text values that stay valid until `jet_text_free` (`Docs/research/evidence-2026-09-29/3110-guest-retention-measurement.md`).

## Component row: cross-thread handle use (criterion 3)
Probe `~/.cache/jet-test-scratch/Closer07/plugin/xthread2.jet`: load mathkit on the owner thread, call `gcd`, then call `mathkit.gcd` from a `task` in `task.group g { … worker.join() }`.
- **JIT:** ICE `Cranelift cannot execute MIR function … MIR call ABI expects 3 arguments, got 2`. Every plugin call ICEs on JIT (see #3102 doc).
- **`--interpret`:** prints `owner thread gcd = 6` / `owner continues`, exit 0. The worker's `print("worker gcd = …")` never appears, yet `worker.join()` succeeds.
- Control without a plugin (`taskprint.jet`: a task that only prints `worker ran`):
  - `--interpret` prints `owner before` / `owner after`. **The task body never runs**, and join still succeeds.
  - JIT prints `worker ran` and then ICEs: `typed drop `K<>` field 0 is unavailable`.
- With the task's `?? { print(err) … }` fallback: E0107 "Nothing named `err` exists here", pointed at the `task {` span (`xthread_err_capture.jet`). The implicit `err` of a `??` block is not visible inside a task body.
- `xthread.jet` uses `mathkit` again on the owner thread after it was captured by the task. Instead of a use-after-move diagnostic, it gives an ICE on both JIT and interpreter: `MIR pass legality-verification rejected the program: … uses moved place … at 402..407` (the later `mathkit.gcd` use).

So the Component handle's cross-thread behaviour is **not observable** on any hosted tier today:
- the interpreter silently skips task bodies;
- the JIT ICEs on plugin calls and on task drops;
- the checker lets the handle move into a task, then ICEs on later owner use rather than rejecting it by ownership law.

## Active-call unload / cancellation
- Not exercised: no runtime path reaches `jet_plugin_close` during an active call.
- Managed-callback `stop` during an active invocation (`FfiCallbacks.rs:543-673`) needs a C driver plus cargo integration test; not attempted.
- Named tests `managed_callback_state_stop_and_cancel_contract` and `plugin_rejects_cross_thread_and_active_close` do not exist (grep over `tests/`, `crates/`).

## Verdict
PARTIAL:
- The native Library re-entry, threads and lifecycle row is re-confirmed on the current integration build.
- Criterion 3's Component and managed-callback cells are blocked by the tier defects below.
- Criterion 4 is not met.

## Defects
1. `--interpret`: a `task { … }` body inside `task.group` never executes, but `join()` succeeds (`taskprint.jet`).
2. JIT: the same program runs the body, then ICEs `typed drop K<> field 0 is unavailable`.
3. The implicit `err` binding of a `?? { }` block is unavailable inside a `task` body (E0107, span on `task {`).
4. Moving a plugin handle into a task and then using it on the owner thread → ICE `uses moved place` (MIR legality) instead of a checker diagnostic.
5. JIT: any `core.plugin` call ICEs (shared with #3102).
