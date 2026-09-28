# What I built

I built a Jet package that fills a caller-owned `[Float#256]` buffer, schedules a fake 48 kHz/256-frame callback for 10 seconds, and measures callback time, jitter, and deadline misses. It also runs a nominal 1 kHz control loop beside the callback and checks each loop start against its absolute due time. Separate probes test audio-module availability, allocation denial, blocking/channel behavior, shared locks, atomic/mutex names, sub-millisecond sleep, and CPU work under a context deadline.

Files: `pkg/realtime.jet`, `pkg/audio-missing.jet`, `pkg/alloc-denial.jet`, `pkg/atomic-attempt.jet`, `pkg/block-denial.jet`, `pkg/busy-deadline.jet`, `pkg/callback-channel-attempt.jet`, `pkg/channel-deadline.jet`, `pkg/channel-denial.jet`, `pkg/lock-free-attempt.jet`, `pkg/shared-hot.jet`, `pkg/subms-sleep.jet`, `pkg/timer-resolution.jet`, and `pkg/probe.jet`.

# What worked

- Native package build works: `scripts/agent/jet-env jet build .../pkg/realtime.jet` reached `Built build/realtime in 14.3s` after code generation and budget verification.
- Fixed-buffer callback works: `realtime.jet:28-32` fills all 256 caller-owned samples; the initial run printed `buffer=0.25,0.25,0.25,0.25`.
- Allocation denial works: `alloc-denial.jet` with `-[!Mem.Alloc]>` rejects `.push` with E0921 at line 3. A bounded loop over the fixed buffer passes the same denial.
- A ten-second fake-device/control run completes and emits measurements:

```text
audio.callbacks=1892
audio.missed=0
audio.jitter_us=min:-95286,max:-5219,sum:-83369436
audio.max_callback_us=944
control.ticks=9307
control.missed=9295
control.jitter_us=min:-888,max:692124,sum:3298975242
```

- Blocking is observable at runtime: a full capacity-one channel inside `#Context(deadline: time.now() + 5)` emits E3003, `Deadline exceeded while waiting in channel send`.
- The C-callback rule is a useful boundary when an external ABI is available: the local spec requires foreign-thread callbacks to be C-safe, with no heap allocation, mutable static/thread-local state, scheduler access, or panic path (`docs/spec/syntax-decisions.md:3113-3120`). This does not provide a device registration API.

# Gaps

## G1 — `impossible`, blocks

Jet has no standard callback/device boundary that registers a fixed-rate audio buffer callback or reports underruns/overruns. `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/audio-missing.jet` returns `Error [E1001]: There is no core module \`core.audio\`` at `audio-missing.jet:7`; the ten-second program therefore uses a timer fake even though `/dev/snd` exists on the host. This affects games, embedded controllers, and GUI/audio applications. A separately authored C/FFI backend can supply a device and pass a fixed buffer to a manually checked callback, but Jet provides no first-party registration or underrun receipt.

## G2 — `impossible`, blocks

Jet cannot preserve fractional-millisecond waits through its scheduler boundary. `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/subms-sleep.jet` called `time.sleep(999_999ns)` 100 times and printed `requested_total_us=99999` but `elapsed_total_us=245`. `crates/jet-codegen/src/Prelude/CoreLib/Top/TimeSleep.rs:53-68` converts the nanosecond carrier with `saturating_div(1_000_000)` before scheduler sleep. A busy-spin on `time.instant()` or external timer FFI is possible, but neither is a portable hard-real-time wait guarantee. The same limitation affects games, embedded controllers, and GUI media loops.

## G3 — `impossible`, blocks

There is no checked lock-free atomic or bounded nonblocking shared-state primitive for callback state. `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/atomic-attempt.jet` returns E0102 (`Nothing named \`atomic\` exists here`, line 2), while `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/lock-free-attempt.jet` returns E0041 (`\`mutex\` is not in Jet`, line 2). Conversely, `shared-hot.jet:11-12` passes `-[!Mem.Alloc, !Time]>` while using `Shared<State>`; the spec defines `Shared<T>` as lock-guarded (`docs/spec/spec.md:587-600`). `callback-channel-attempt.jet:1-5` also passes the same denials while calling `tx.send(1)`, although a full send can block and E3003 catches it only at runtime. This affects games, embedded controllers, and GUI/audio applications. Today the author must keep state in caller-owned buffers, move communication outside the callback, or use a separately audited native atomic/ring-buffer backend.

## G4 — `impossible`, blocks

`#Context` does not enforce a CPU deadline or emit an overrun receipt. `busy-deadline.jet:10-15` runs 10,000,000 integer iterations under `#Context(deadline: time.now() + 5)`; `scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/busy-deadline.jet` passes with zero diagnostics and `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-realtime/pkg/busy-deadline.jet` prints both `cpu-finished=49999995000000` and `context-returned`. The spec limits inherited E3003 observation to wait/IO points (`docs/spec/spec.md:2664-2668`). The ten-second program can manually detect misses (`control.missed=9295`), but it cannot interrupt or prove a CPU callback deadline. This affects games, embedded controllers, and GUI/audio applications. An external watchdog plus manual checks is the only current workaround.

# Friction

- Fixed-buffer initialization in `realtime.jet:35` repeats 256 zero literals. This is library-author boilerplate, not user-facing ceremony.
- The first `Shared<State>` run needed an explicit `Mem.Rc` authority grant (E1803); after adding it, `shared-hot.jet` ran and printed `ready=1`.
- The realtime accounting loop triggered four L2510 `hidden_cost_in_loop` warnings on exact-`Int` jitter sums (`realtime.jet:49,52,86,89`). No incumbent timing comparison was run, so this is recorded as friction rather than a `slow` claim.
- Valid timing code repeatedly emits L0520 (`RangeError` has no `Display` impl). The warning points at `callbacks += 1` in the ten-second program and at a blank final line in the small timer probe, rather than at the printed duration expression.

# Defects

- Diagnostic location defect candidate: `jet run pkg/timer-resolution.jet` reports L0520 at blank `timer-resolution.jet:16`, while `jet check pkg/realtime.jet` reports the same warning at `realtime.jet:59` (`callbacks += 1`). Both programs still produce their measured output; no runtime panic or wrong numeric result was observed.
- No internal compiler error or wrong callback-buffer result was observed.

# Battery notes

Not applicable: this is a primitive probe, not a critical-area battery probe.

# Verdict

A fixed-buffer, allocation-denied callback simulation is buildable today. A library author cannot build a complete hard-real-time device path with today's Jet: the device callback boundary, fractional-period scheduler, lock-free shared state, and CPU deadline enforcement are missing. Manual timing checks and external C/FFI code provide partial workarounds but no portable guarantee. The result is not buildable as a hard-real-time audio/game/embedded runtime until the four listed primitives exist.
