# Adversarial review: C carriers and serialization

2026-11-08. Covers D-FFI-CARRIER1 and D-FFI-SERIAL1 for #6689.

Reviewer: ReviewFfiCarrier, GPT family, fresh to these drafts. Author provenance supplied by Main: SchedLead, Claude Opus 5.5. The ballot metadata uses the family name `Claude Opus`; the validator treats the first period as a field terminator.

## Result

Both revised ballots pass `Tools/agent/ballots/Validate.jet`, with `situation` and `reviewPasses.adversarial` filled in. This is a payload pass, **not implementation or design sign-off**. The schema requires `draft:false` for posting; that flag does not discharge the failed claims below.

I do not endorse either original unconditional A recommendation. Carrier A is a conditional candidate if the owner explicitly approves mixed carriers and accepts its remaining costs. B is the strongest direct-call baseline and deserves a fair measured comparison. Serial A is coarse overlap protection, not a safety boundary. I prefer the added serial D: audited wrappers using existing synchronization around the actual operation. It has a runnable example and avoids another meaning of locking.

Added carrier G: one C-compatible worker kind for every native program, naming its conflict with the pure-Jet no-C goal. Strengthened carrier D to move the entire task once to a dedicated C thread, rather than unnecessarily marshal every later call. Added serial D and its exercised `Shared<Int>.guard_edit` wrapper. Kept the author's A labels and recommendations, but made both conditional and recorded the dissent.

Do not post the old claims that A is one mechanism merely because it has one scheduler, costs 3.5 microseconds to enter, has zero per-call cost, gives each task its own C state, never stalls Jet work, or already supports arbitrary callbacks, fork, exit and signals. Do not post that a per-library gate makes unsafe C safe, that recursive entry is harmless, that the formatter usually returns 8 without a lock, or that a generator can infer concurrency from headers. The revised ballots are alternatives with explicit limits, not a passing solution review under AGENTS.md.

## Authority and source checked

Read AGENTS.md first, then the orchestration ballot-review reference and philosophy. Read #6689 and #6693 through the read-only Tower card command; wrote no Tower state. #6689 has no ratified decision attached. Its Q9 log quotes: “lean towards A but need downsides engineered away.” This is conditional owner direction, not approval of the drafts' particular carrier or per-library policy. Its later log says “No second carrier mechanism is built.” New mixed-carrier scope therefore needs explicit approval.

#6693 remains an open no-C pack task. Its body says C-interop programs still link their C closure and that the card decides nothing about that feature. It does not prove a post-cutover executable's startup.

The following references were read in the read-only `.agent-worktrees/Main-dev-collectall` snapshot. Paths below are relative to that snapshot:

- `Compiler/JetRuntime/Source/Thread.jet:33-38,439-476`: pthread imports and the actual create/join/detach path. `:484-506`: the raw clone fixture, without independent C TLS setup.
- `Compiler/JetRuntime/Source/Scheduler.jet:911-925,952-969,1036-1076`: worker start, queueing and fiber execution.
- `Compiler/JetRuntime/Source/Fiber.jet:69-82,97-114`: execution on the fiber and switching out at waits/yields.
- `Compiler/JetIR/Source/BuildForeign.jet:129-157,299-334`: compile-time/build-invocation stop path, direct foreign call, then result conversion and argument cleanup. No proposed carrier move or library gate appears in that path.
- `Compiler/JetRuntime/Source/Thread.jet:135-162`: explicit host attach and its caller's detach obligation. This is not proof of automatic callback entry or correct foreign-thread shutdown.
- `Compiler/JetBackend/Source/Image/StaticLink.jet:1055-1064,1153-1214,3047-3052`: libc startup synthesis and owned-startup qualification. Retained TLS and nonqualifying foreign sections disqualify the owned path.
- `Compiler/JetParser/Source/Parser/Items.jet:1522-1555`: inline FFI reads the first marker argument. `Compiler/JetSema/Source/Sema/Registration/Checked.jet:1720-1765`: the constructed foreign row has no proposed concurrency policy.

Static findings are labeled as such. Exercised behavior below is from a0_040, with `JET_TOOLCHAIN_ROOT` pointing at the inspected worktree, not a newly compiled compiler from that snapshot. Agreement between source and seed is evidence only for the named path, not proof of every pending change or runtime pack.

## Executed evidence

All probe sources and outputs belong under `$JET_SCRATCH_ROOT/lanes/Main/reviews/D-FFI-CARRIER1/`. No repository worktree was modified. Probes ran under `taskset -c 16-31 nice -n 19`, with a 4 GiB address-space limit and one compiler thread. Channel measurements further restricted execution to CPUs 16,17 or CPU 16. Native C witnesses used an installed GCC wrapper after ordinary `cc` was absent. The seed `jet-cc` wrapper separately failed because `bwrap` was absent from its PATH; that failed attempt is not a successful compile receipt.

| Probe | Observed result | What it establishes |
|---|---|---|
| Original carrier and serial example shapes | E0405: task result was Unit, incompatible with `?? -1`; also L1101 for the remaining task | The original displayed examples did not compile. |
| `carrier/main.jet`, `serial/main.jet`, after adding an audited value-returning wrapper | Each printed `8`, exit 0, at O0 with `--allow=FFI.C` | Corrected spelling executes. One racy serial run is not a concurrency proof. Without the invocation grant the probes reported E1803. |
| `parallel/main.jet` | `2`, exit 0 | An atomic rendezvous observed both foreign calls enter before either returned on two workers. This avoids using C undefined behavior as a race detector. |
| `path/main.jet` at `--optimize=none`, `basic`, `full` | Each printed `1` then `2`, exit 0 | A C local was inside the stack reported by public pthread stack APIs from `run`, but outside that stack from a task. This supports foreign calls on task fiber stacks on these native optimization levels. |
| Built `path` executable, then executed it | `1` then `2`, exit 0 | The built O1 image agrees. Disassembly of `_start` showed a call to `__libc_start_main`. C TLS, errno and malloc also executed in this probe. |
| `comptime/main.jet` | E3010: compile-time code cannot call foreign function `seven` | Concrete I9 gap; the current native run path must not be generalized to compile-time execution. |
| `optout/main.jet` with `FFI(c, concurrent)` | `7`, exit 0 | The seed accepts the spelling. Contrary to a tempting correction, it is not simply rejected syntax. Source inspection shows no corresponding proposed gate policy; spelling acceptance does not prove opt-out semantics. |
| `binding-lock/main.jet` | `8`, exit 0 | Serial D's existing `shared`/`guard_edit` wrapper executes for the same static-buffer operation. The guard is used after C returns to cover the whole operation. |
| `raw-tls.c` | `shared_fs=1 shared_errno=1 parent_errno=42 child_ok=1` | A Linux clone sharing VM, without setting new TLS, used its creator's thread pointer and errno address. This was a glibc witness with a waitable clone child, not a musl heap-corruption experiment or a full raw-carrier proof. |
| `reentry.c`, one thread, recursive pthread mutex | `outer_expected=5 outer_actual=3` | Nested entry can overwrite active static state despite recursive serialization, without a data race. Recursive lock ownership is not library reentrancy. |

The corrected stack probe needed an explicit public `pthread_getattr_np` declaration: its inline `_GNU_SOURCE` definition came after generated headers and did not expose that prototype. The final probe compiled and ran; the earlier undeclared-function error was not a stack result.

### Cost

Repeated the author's main-to-task channel experiment: 1 worker, 1,000 warmups, 200,000 request/reply rounds, O2. Individual results:

- Two CPUs: 1,080,706,222 ns total / 5,403 ns per round; 1,085,352,530 ns / 5,426 ns.
- One CPU: 1,246,664,634 ns / 6,233 ns; 1,238,008,088 ns / 6,190 ns.

These are wall-clock channel/park/wake averages using `unix_ns`, not isolated FFI costs or latency distributions. They neither reproduce a universal 7-microsecond charge nor bound an optimized foreign handoff. They do not measure carrier A. Dividing a round trip by two assumes symmetry and the same work on both paths; the probe establishes neither. A's migration path does not exist to time in the inspected implementation. Its cost remains a failed claim, not an estimate approved for posting. Required comparison must separate cold thread creation, warm moves, residency checks, direct-call conversions, contention and return moves against B on matched programs.

## Claim verdicts and fixes

“Fails” includes missing evidence. It does not assert that a proposed implementation could never satisfy the contract. Gap IDs refer to proposed ballot lines below.

| Claim in the drafts | Verdict | Error fixed / remaining boundary |
|---|---|---|
| Jet now has raw workers; Core no longer uses C | fails | Replaced accomplished-fact language with prerequisites. Inspected worker source still uses pthreads; no-C scope must cover every kept dependency. G1. |
| Bare cloning supplies valid ordinary libc thread state | fails | Raw-TLS witness shows aliasing. Serialized access does not create a valid independent C thread. Freestanding C is not categorically the same as a libc-dependent library. G1. |
| Carrier A is one mechanism; B alone violates I8 | fails | Both add/select creation paths; A additionally routes eligible fibers. One worker loop does not waive I8 or Main's direction. Added G as the canonical-worker alternative and named its G1 cost. G1. |
| A has one meaning on every tier | fails | O0/O1/O2 stack probe agrees, but compile-time foreign call stopped with E3010. No owner tier exception was supplied. G2. |
| Every example prints 8 | fails | Fixed Unit-returning task bodies and documented FFI authority. Carrier allocation failures can differ; the TLS counter is not read. Unguarded static-buffer races have undefined behavior, not a stable output distribution. |
| Each caller gets its own C thread/TLS under A | fails | C TLS is worker-local; sequential tasks can share it and a resumed task can change it. Destructor lifetime is the native thread's lifetime. G3. |
| errno can always be read “right after” a foreign call | holds only within stated limits | Read/copy it within the same foreign boundary, before conversion helpers, waits or another native call. G3. |
| A's entry is about 3.5 us; C/D cost about 7 us per call | fails | Removed inferred FFI numbers. Channel measurements are proxies, and D need not marshal each later call. G4. |
| Steady A calls cost nothing | fails | Even proposed residency checks and ordinary conversion have costs. Only repeated moves may be amortized. G4. |
| Blocking C never stalls Jet work | fails | Isolation protects raw workers, not Jet tasks dependent on saturated C capacity, a gate or a pinned callback. A finite pool can deadlock while unrelated raw work runs. G5. |
| A callback is always the same task; existing attach solves it | fails | Distinguished synchronous nested callbacks from callbacks on newly created C threads. Explicit attach APIs alone prove neither automatic entry nor lifecycle correctness. G6. |
| Preferring the last C worker fixes affinity | fails | Preference is not a pin. Dedicated D strengthens the comparison; initial-thread-only APIs need a separate route. Unknown later pin spelling is not delivered functionality. G6. |
| Blocking application signals makes all C handlers safe | fails | Process-directed delivery is only one case. Synchronous faults and thread-directed signals are not rerouted; masks, reserved signals and alt stacks remain contracts. G7. |
| Credential wrappers, bounded fork waits and C-thread exit already settle lifecycle | fails | No composed proof supplied. Include C-created/concurrently created threads, fork's current call and child restrictions, atfork callbacks, exit admission and destructor ordering. G7/G8. |
| Foreign calls use a task fiber stack | holds only within stated limits | Source and executed pthread-stack query agree for the native seed's tested O0/O1/O2 path, not every FFI shape or future runtime. Define stack budget. G9. |
| C programs keep libc startup after #6693 | holds only within stated limits | Current inspected guard and built C/TLS image support initialization. #6693 is still open; its future pack and merely linked dead C were not exercised. Removed universal post-cutover wording. G9. |
| Per-library gates make non-thread-safe C safe | fails | Gates protect participating callers only. Aliases, shared dependencies, separate copies, signal handlers and native background threads break the broad framing. G10. |
| Same-task recursive entry may safely skip the gate | fails | Deterministic recursive-mutex witness returned the overwritten length. Concurrency and reentrancy require separate audits. G11. |
| Call-return lock scope covers native data lifetime | fails | Copy borrowed results before unlocking; multi-call operations and retained pointers need operation-level protection. G12. |
| Automatic gates add only a rare hang | fails | No frequency evidence. Same-gate waits, opposite library lock order and saturated native frames can introduce deadlocks even for otherwise concurrent libraries. G5/G11. |
| A slow library never delays another one | fails | Shared gates/dependencies, cross-library callbacks and exhausted C carriers couple progress. Removed “never.” G5/G10. |
| Global gate C is always safe / cannot be sound | fails | These claims contradict each other and conflate thread setup with serialization. Valid C threads can use a global gate; it still cannot control external native activity or reentrancy. G8/G10/G11. |
| `concurrent` is a working safe opt-out inferred from headers | fails | Accepted seed spelling is not its proposed semantics. No concurrency field in the checked row; no header-to-policy inference proof. Added explicit admission/policy requirement. G13. |
| Existing operation-level synchronization is a viable missing alternative | holds only within stated limits | Added serial D; its `Shared<Int>.guard_edit` example ran. This does not prove arbitrary safe bindings, affinity or callback designs. G12. |
| Stall reports and a few atomics engineer away the losses | fails | Neither performance nor complete diagnostics were exercised for the proposed mechanisms. Reporting a wait cycle does not release a native frame or guarantee detecting it while other tasks run. G4/G5/G14. |

## Strongest peers, fairly compared

These are checked source/specification comparisons, not benchmark wins or executed peer programs.

- **Go cgo, Linux:** [Go 1.25 `newm1`](https://github.com/golang/go/blob/go1.25.0/src/runtime/proc.go#L2897-L2919) starts cgo Ms through `_cgo_thread_start`; [the C start helper](https://github.com/golang/go/blob/go1.25.0/src/runtime/cgo/gcc_linux_amd64.c#L52-L75) uses the pthread path. [`cgocall`](https://github.com/golang/go/blob/go1.25.0/src/runtime/cgocall.go#L5-L28) enters syscall state and switches to g0's OS stack; the callback path switches back. This is much stronger than “B blocks a Jet worker”: B can preserve direct calls and release scheduler capacity. Go does not provide goroutine-local C TLS or automatic per-library safety.
- **Java Loom:** [JEP 444](https://openjdk.org/jeps/444#Scheduling-virtual-threads) describes changing carrier identity between native calls, native pinning, and no compensation for pinning. [JEP 491](https://openjdk.org/jeps/491#Diagnosing-remaining-cases-of-pinning), delivered in Java 24, removes monitor pinning but explicitly retains pinning when native code calls back into Java that blocks. It provides diagnostics; it does not make finite native capacity immune to starvation. Comparing only against pre-491 monitor pinning would be stale.
- **Erlang:** [`erl_nif`, Lengthy Work / Dirty NIF](https://www.erlang.org/doc/apps/erts/erl_nif.html#lengthy_work) recommends ordinary native work return within about a millisecond or split into chunks; dirty CPU and I/O jobs use separate schedulers. Running dirty native work is not stopped merely because its process terminates. This resembles offloaded jobs, not sticky whole-task residency or stable OS identity.
- **Tokio:** [`spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) submits a closure, so many C calls can be batched in one job. Started jobs cannot be aborted; shutdown can wait indefinitely. [Builder source](https://github.com/tokio-rs/tokio/blob/tokio-1.48.0/tokio/src/runtime/builder.rs#L432-L447) specifies a default 512 blocking threads and a queue without backpressure. That is a comparator's explicit limitation, not justification for Jet choosing 512 or promising bounded queue memory.
- **Zig:** [0.14.1 `Thread.zig`](https://github.com/ziglang/zig/blob/0.14.1/lib/std/Thread.zig#L23) selects pthreads when libc is linked; [implementation selection](https://github.com/ziglang/zig/blob/0.14.1/lib/std/Thread.zig#L102-L112) chooses Linux's direct implementation otherwise. This is another strong B-like comparator. Native C ABI calls are not an automatic library-safety or fiber-isolation facility.
- **Swift:** [SE-0306](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0306-actors.md#actor-reentrancy) isolates actor-owned state and permits interleaving at suspension. [SE-0296](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0296-async-await.md#proposed-solution-asyncawait) says synchronous work cannot give up its thread and an async function need not resume on the same thread. Actors do not automatically serialize foreign global state or promise native-thread affinity. Their useful lesson is explicit state ownership, not a library-name gate.
- **.NET:** [P/Invoke documentation](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/pinvoke) describes native calls, callbacks and explicit marshalling. [CoreCLR's stub emission](https://github.com/dotnet/runtime/blob/v9.0.0/src/coreclr/vm/dllimport.cpp#L673-L687) calls the native target and immediately saves last error when requested. This is a useful error-capture contract absent from “read errno after any Jet conversion.” It is not automatic offloading or library locking.
- **Python, for the serial analogy:** [thread-state/GIL documentation](https://docs.python.org/3/c-api/threads.html) explains releasing the GIL around blocking work. [Free-threaded extension guidance](https://docs.python.org/3/howto/free-threading-extensions.html#module-initialization) requires an explicit GIL-support declaration. These protect interpreter integration, not arbitrary C globals or threads outside that lock.

Additional constraints were checked against [pthreads(7)](https://man7.org/linux/man-pages/man7/pthreads.7.html), [signal(7)](https://man7.org/linux/man-pages/man7/signal.7.html), [fork(2)](https://man7.org/linux/man-pages/man2/fork.2.html) and [exit(3)](https://man7.org/linux/man-pages/man3/exit.3.html). In particular, only the forking thread survives; the child of a multithreaded fork is restricted to async-signal-safe operations until exec. [GLFW's thread-safety and reentrancy rules](https://www.glfw.org/docs/latest/intro_guide.html#thread_safety) supply a real counterexample to treating a pinned arbitrary worker or recursive library entry as sufficient.

## Gap lines proposed in the revised ballots

These are proposed acceptance text, not newly created Tower cards or a second work ledger. Main can attach them to #6689 or the chosen owner ballot.

| Gap | Proposed card/ballot line |
|---|---|
| G1: mechanism and scope | “Approve the exact foreign-boundary exception to I8 and Main's no-second-carrier direction, or use one canonical worker kind; prove the complete pure-Jet link closure contains no C.” |
| G2: tiers | “One shared runtime contract covers run/dev O0, build O1, release O2 and compile-time/build-time foreign execution; otherwise obtain an explicit, named tier exception before claiming I9.” |
| G3: native state | “Document worker-local TLS/destructor lifetime and capture errno and borrowed results before any intervening helper, wait or unlock; prove reuse and migration cases.” |
| G4: cost | “Measure cold creation, warm moves, residency checks, direct-call conversion, return moves and contention against B with 1, 8 and 64 callers; no half-RTT or unmeasured zero-cost claims.” |
| G5: progress | “Specify finite pool and queue limits, admission fairness, pinned nested-call behavior and cancellation ownership; test saturation with dependent callbacks. Disclose residual deadlocks rather than substituting a stall report for progress.” |
| G6: callbacks and affinity | “Define synchronous and foreign-thread callback ownership, attach/detach, pin lifetime, initial-thread routing and nonlocal-exit behavior; approve the complete expert affinity surface.” |
| G7: process/thread state | “Cover inherited and mutated masks, reserved signals, alt stacks, synchronous and thread-directed faults, and credential changes during C-created thread churn.” |
| G8: lifecycle | “Define fork admission including the current call, atfork and native threads; no unsupported Jet continuation in the child. Define exit versus _Exit, admission closure, in-flight callbacks, atexit and TLS teardown without self-join.” |
| G9: stack/startup | “Prove native stack budget and overflow behavior; keep C/TLS startup tests against the actual post-#6693 pack, including live and dead foreign closure cases.” |
| G10: gate identity | “Make all aliases of one native state domain share protection; test shared dependencies and separately loaded copies. State that Jet-origin gates do not serialize native background threads or handlers.” |
| G11: reentry | “Do not treat recursive ownership as library reentrancy. Reject unaudited reentry and test same-task, foreign-thread and opposing-library callbacks, with specified diagnostics.” |
| G12: operation lifetime | “Keep result/error capture inside the gate, and reuse existing Shared guards or owner queues for multi-call state and retained memory. Compare audited-wrapper D before introducing another mechanism.” |
| G13: marker policy | “Ratify and implement the opt-out meaning through the existing policy ladder; reject or define extra FFI arguments instead of treating syntactic acceptance as policy support. No automatic header-comment inference.” |
| G14: evidence/tooling | “Exercise composed callback, saturation, shutdown and mixed-stack diagnostics on applicable tiers; measure lock fast paths. Until then, describe these as requirements, not existing features.” |

## Validation receipt

Executed from the repository root with the current seed, `JET_TOOLCHAIN_ROOT` set to the read-only worktree, CPU affinity 16-31, nice 19 and a 4 GiB limit:

```text
jet Tools/agent/ballots/Validate.jet \
  $JET_SCRATCH_ROOT/lanes/SchedLead/ballots/D-FFI-CARRIER1.json \
  $JET_SCRATCH_ROOT/lanes/SchedLead/ballots/D-FFI-SERIAL1.json

ok $JET_SCRATCH_ROOT/lanes/SchedLead/ballots/D-FFI-CARRIER1.json (D-FFI-CARRIER1)
ok $JET_SCRATCH_ROOT/lanes/SchedLead/ballots/D-FFI-SERIAL1.json (D-FFI-SERIAL1)
```

The paths in the receipt are normalized to `$JET_SCRATCH_ROOT`; the command returned exit 0. No implementation tests, peer benchmarks or future-pack proof are implied by that result.
