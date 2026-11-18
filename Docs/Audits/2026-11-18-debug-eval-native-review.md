# Adversarial review of the native debug evaluator proposal — 2026-11-18

Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: IfHole-6-1-3.

Reviewed: `Docs/Proposals/2026-11-18-debug-eval-native.md` (#6841 R3 and R8),
as submitted on base `ebce85f295`, against the tree at `d79773d505`
(2026-11-18) and the gate-p293 `jet` image. The review follows AGENTS.md
"Adversarial design review": each claim names the strongest existing
evaluators, is attacked, is checked against code, a probe or a measurement,
and ends with a verdict. Missing context counts as a failure.

## Verdicts

| # | Claim | Verdict |
|---|---|---|
| C1 | One evaluator serves `print`, `watch`, conditions, logpoints, DAP `evaluate`, `debug_view`, dumps and replays (I8) | holds only within stated limits |
| C2 | A closure written at the stopped line gives the compiler's own scope, and its captures are exactly the named locals | fails |
| C3 | The row `-[!Time.Wait, !Unsafe]>` with no positive grant refuses every positive root | fails |
| C4 | Evaluation is refused when the source changed since the build, by the hash the build recorded (c31) | fails |
| C5 | Generic frames instantiate the closure from DWARF template parameters | holds only within stated limits |
| C6 | O2 locals: refused when unlocated, reassembled from pieces, recovered by CFI | holds only within stated limits |
| C7 | Everything is compiled from source at O0, so no image code runs and no image lock is reachable (R8) | fails |
| C8 | The eval object binds image data through the image's symbol table; addresses are fixed (`ET_EXEC`) | fails |
| C9 | A toolchain digest match guarantees matching layouts | holds |
| C10 | Stubbing the runtime's stateful entry points removes the target allocator, scheduler and locks, and a test enforces the list | holds only within stated limits |
| C11 | The injected `clone` writes no byte of the target and leaves it exactly as stopped | holds only within stated limits |
| C12 | The fork cannot deadlock on a runtime lock, cannot block on a recorded wait, and detects the rest at a 2 s budget | holds only within stated limits |
| C13 | Descriptors, signals, timers and shared memory cannot leak the fork's actions to the target | holds only within stated limits |
| C14 | The call runs on the stopped thread's stack copy; an overflow faults and is reported | holds only within stated limits |
| C15 | Results and `debug_view` are rendered without running the target | fails (inherits C7) |
| C16 | Dumps and replays use the same evaluator | fails (deferred, no design) |
| C17 | Latency: caching and batching make watches cheap; R7 shares the compiled condition | holds only within stated limits |
| C18 | I9: an expression computes the same value at O0, O1 and O2 | fails |
| C19 | The alternatives are worse | holds only within stated limits |

Four failed claims (C2, C3, C7, C18) are design failures, not missing
features: each lets the evaluator show a wrong value, run target code, or
perform an effect the design says it refuses. The design stays unbuilt
until they are redesigned and reviewed again.

## The strongest existing evaluators

Each entry states the peer's best case, what it does that this design does
not, and its source.

- **lldb `expression` (Clang plus JIT), and its IRInterpreter.** lldb parses
  the expression with Clang in the frame's scope, lowers it to LLVM IR, and
  first tries `IRInterpreter::CanInterpret`: a simple expression (arithmetic,
  loads, no calls) is interpreted in the debugger, reading target memory,
  and never executes in the target. Only otherwise does it JIT the IR, write
  it into the target and run it on the stopped thread. `--allow-jit false`
  forces interpretation; `--timeout` and `--try-all-threads` (default on)
  run the expression on one thread first and, after the timeout, resume all
  threads so a lock another thread holds can be released; `--unwind-on-error`
  restores the frame after a crash. Sources: lldb `help expression`;
  `lldb/source/Expression/IRInterpreter.cpp`; `lldb/source/Expression/LLVMUserExpression.cpp`.
  Measured here (lldb 21.1.8, a 16-line C program at `-O0`, three rounds,
  the session's wall time minus the same session without the command): an
  interpreted `expr total + 1` adds −4 to +13 ms, a JIT call
  `expr square(total)` adds 6–13 ms, and five distinct call expressions add
  16–24 ms. The noise floor of the method is about 10 ms. **What it does that
  this design does not:** evaluates effectful calls; evaluates simple
  expressions with no target execution at all and no compile of the
  enclosing package; costs milliseconds for a new expression.
- **gdb `print f()`/`call`, `compile code`, and `checkpoint`.** gdb's own
  evaluator interprets C-like expressions over target memory; a function call
  runs in the inferior on the stopped thread (`set unwind-on-signal`,
  `set may-call-functions off` to forbid). `compile code` compiles source with
  GCC against the frame's scope, injects the object and runs it in the
  inferior. `checkpoint` (GNU/Linux) snapshots the inferior by forking it;
  `restart N` switches to the fork. Sources: GDB manual, "Calling Program
  Functions", "Compiling and Injecting Code", "Checkpoint/Restart". **Prior
  art for this design:** `checkpoint` is fork-based state duplication of a
  stopped inferior; `compile code` is "compile in the frame's scope, link
  against the inferior, run". gdb does not combine them, and documents that a
  checkpoint shares file descriptions and that I/O done in a checkpoint is
  not undone.
- **rr diversion sessions.** During rr replay, a gdb `call` is run in a
  `DiversionSession`: rr clones the replay state into a scratch session,
  runs the call there "for its side effects", and discards it, so the
  recording is untouched. Syscalls in a diversion are restricted. Source: rr
  `src/DiversionSession.h`, `src/DiversionSession.cc`. **Prior art:** this
  is the closest existing design to the proposal (run in a disposable copy,
  never in the real process). The proposal does not cite it.
- **Delve `call` (Go `runtime.debugCallV2`).** Delve injects a call through a
  runtime-cooperative protocol: the runtime's `debugCallCheck` refuses at an
  unsafe point (inside the runtime, at a non-preemptible PC), and the call
  runs on the goroutine with the real allocator and scheduler; other
  goroutines may run. Sources: Go `src/runtime/debugcall.go`; Delve
  `Documentation/cli/README.md` (`call`). **What it does:** the runtime
  itself says when an injected call is safe, instead of the debugger
  stubbing the runtime out.
- **JVM JDWP `InvokeMethod`.** `ObjectReference.InvokeMethod` runs a real
  method in the target. By default every thread suspended by the event is
  resumed for the invoke "to prevent the deadlocks that will occur if any of
  the threads own monitors that will be needed by the invoked method";
  `INVOKE_SINGLE_THREADED` opts out and accepts the deadlock risk. Source:
  JDWP protocol specification, `ObjectReference.InvokeMethod`. **What it
  does:** full language semantics, including effects, with a documented
  deadlock answer that perturbs the program.
- **.NET func-eval and Visual Studio `nse`.** `ICorDebugEval` hijacks a
  thread at a GC-safe point and runs managed code; Visual Studio aborts a
  func-eval at a timeout. The C# format specifier `, nse` ("no side
  effects") evaluates an expression only if it can do so without side
  effects and otherwise reports that it cannot. Sources: Microsoft Learn,
  "Format specifiers in C#" (`nse`); `ICorDebugEval` reference.
  **What it does:** a shipped "no side effects" evaluation mode. The earlier
  audit's D4 verdict ("no peer proves side-effect freedom") overstates: VS
  refuses side effects dynamically; Jet's claim is only that it refuses them
  statically.

## Claims

### C1. One evaluator for every surface

**Peers.** lldb uses one expression evaluator for `expr`, breakpoint
conditions, `frame variable` formatters (through its formatter engine) and
DAP; gdb's evaluator serves conditions and `print`. Neither covers core
files with calls (gdb and lldb refuse calls on a core).

**Attack.** The design has two meanings for the same closure: in the fork
with the stub runtime (§5), and inside the program for R7 conditions (§8),
which runs with the real runtime. A condition that allocates allocates from
the target heap under R7 and from the arena in the fork; a condition that
panics stops the target under R7 and prints `panicked:` in the fork. Dumps
and replays (C16) have no design.

**Verdict.** Holds only within stated limits: one source of truth for
parsing and checking, not one execution meaning. R7's in-program execution
must be specified under the same row with its own panic and allocation
semantics (gap G13).

### C2. The overlay closure gives the compiler's own scope

**Attack 1: the overlay changes the program it is checked in.** Jet decides
move versus share by last use (spec "Ownership": "An unmarked use that is
not the last one shares the value instead of moving it"). In the original
program `box := Box{items: xs}` is the last use of `xs`, so the image moves
`xs` into the box, and `box.items.push(4)` then changes that one list. The
overlay closure adds a later read of `xs`, so in the overlay that line is
no longer the last use: sema copies `xs` instead of moving it and accepts the
overlay. Probe `overlay_move.jet`: `jet check` says "no problems", and
`jet run` prints `4` then `3`, so the overlay program's `xs` has 3 items. The
image never made that copy. Its `xs` slot is a moved-from place whose buffer
the box owns, so the fork reads the box's 4-item list or a released slot
([INFERENCE]: which one depends on the O0 slot after a move), never the 3
items the checked program means. The explicit take form is
safe: with `consume: fn(xs: &&[Int])` the overlay is refused with E0209
("`consume` takes `xs`, but `xs` is used again later"), which is the right
answer.

**Attack 2: statement position.** The closure is inserted "at the start of
the stopped line, inside the innermost scope the line opens in". That has no
answer for a stop on a `loop i in …` header line (the line's own scope is
the loop body; the start of the line is outside it, so `i` is not in
scope, yet that is where every iteration's stop lands), for a stop inside a
multi-line expression or a single-line arm (`-> expr`), or for an outer frame
whose pc is a return address in the middle of a line.

**Attack 3: the closure's result type.** The proposal writes
`fn() -[…]> <inferred> { … }`. The probes had to name the result type
(`-[…]> Int`), and the proposal does not say which Jet form infers a result
under a written row. The debugger cannot name the type before the
expression is checked.

**Verdict.** Fails. Gaps G1, G2, G3.

### C3. The row refuses every positive root

**Code.** `JetSema/Source/Sema/Effects/Checks.jet`
`sema_effect_cap_ceiling`: "None when the cap only denies, so its ceiling is
the rungs' above". The spec says the same of function rows ("a `-[!Net]>`
function may call a `-[DB]>` function over a local SQLite file"). A row of
denials only has no ceiling of its own: it inherits the stopped function's
rungs, which for an unannotated program grant everything the package and run
grant. Probe `row_io.jet`: a function `peek: fn(total: Int) -[!Time.Wait,
!Unsafe]> Int` whose body calls `print("side effect")` passes `jet check`
("no problems"), and `jet run` prints `side effect` then `42`.

**Attack.** The §3 table lists `IO`, `FS`, `Net` … as "refused, positive
roots that no row grants". The written row does not refuse them. The second
wall (seccomp) kills the fork at the first `write`, so the user sees a
killed fork, not the promised sema diagnostic naming the call and the
effect. An effect that makes no syscall the filter refuses (an allowed
`mmap`, or output that only fills a buffer, [INFERENCE]) passes both walls.

**Shared state is unreadable.** `SharedAccess.jet`: "Every `Shared<T>`
protocol access blocks on the cell's lock and records `Time.Wait`, except
the wait-free scalar … rail". A debugger expression that reads a field of a
`Shared` struct is refused. Probe `shared_read.jet`: `cell.hits +
cell.names.len()` inside the eval closure is E0747 ("This callback uses the
effect `Time.Wait`, which the parameter doesn't allow"). In the fork the read
is safe whenever the lock word is free. lldb and gdb read such memory
directly.

**Verdict.** Fails. Gaps G4, G5.

### C4. Stale source refusal by the recorded hash

**Code.** `JetBackend/Source/Image/Dwarf.jet` emits no file checksums (no
`DW_LNCT_MD5`; the only DWARF operators it writes are `DW_OP_breg0`,
`DW_OP_bregx`, `DW_OP_reg0`, `DW_OP_reg6`, `DW_OP_regx`,
`DW_OP_stack_value`), and no JetDebug or Jetline source names a source hash.
The build records none today; c31 is open.

**Verdict.** Fails: the claim describes a mechanism that does not exist.
gdb and lldb read DWARF 5 MD5 file entries for this check. Gap G6.

### C5. Generic frames

The proposal states the gap itself: no `DW_TAG_template_type_parameter`
today (confirmed: the gate-p293 `jet` image's `.debug_info` has none), and
evaluation is refused in a generic instance until it lands. Most of a large
Jet program's frames are in Core generics (`List`, `Map`, iterators).

**Verdict.** Holds only within stated limits: generic frames are a refusal
until G7 lands, and the proposal must state how often that refusal hits.

### C6. O2 locals

**Code.** `Dwarf.jet` emits no `DW_OP_piece`, so §3's "a value split into
pieces is put back together" describes nothing the backend produces; an O2
aggregate split across registers is either described as one location or not
at all ([INFERENCE] from the operator list). A located O2 local may still
hold a value from a different source point than the stop (no
`DW_OP_entry_value`, imprecise ranges), which the design treats as exact.

**Verdict.** Holds only within stated limits: refusal of unlocated locals is
sound; the piece and precision claims are untested. Gap G8.

### C7. No image code runs

**Code.** JetIR has `CallIndirect` and `FunctionAddress` (`Build.jet:3052`,
`BuildCore.jet:461`): a function value holds a code address. The spec
(`!Mem.Stack` section) says a call through a function value or a trait value
may run any function the program takes as a value or any implementation it
holds. A function value or trait object the expression reads from target
memory therefore holds an image code address, and calling it
(`print handlers[0](3)`, `print shape.area()` on a trait object, a
`debug_view` that calls a stored callback) jumps into image code compiled
at O2 and linked against the image's own runtime: its allocator (`Alloc.jet`
`alloc_lock`, a futex), its refcounts, its globals. That reintroduces every
lock and heap hazard §6.1 layer 1 rules out.

**Verdict.** Fails. The fix is to map each indirect target address back to
its function through the image's symbols and call the eval object's own
compiled copy, refusing an address with no symbol. Gap G9.

### C8. Binding image data through the symbol table

**Probe.** `nm` on the gate-p293 `jet` image (`ET_EXEC`, confirmed with
`readelf -h`): 43434 symbols, of which 2 are data symbols
(`jet_rt_checks_armed`, `jet_rt_process_ending`). Module-level globals and
the runtime's 145 module-level mutable globals (`git grep '^[a-z_]+ := '`
over `JetRuntime/Source`, for example `Alloc.jet` `alloc_root`) have no
symbol. Function symbols do carry Jet names (`DebugDap.jet_cli_dap_evaluate`).

**Attack.** "Globals whose identity matters" cannot be bound: there is
nothing to bind to. Worse, the eval object links its own copy of the runtime
object, so every runtime global it reads is the copy's initial value, not
the target's. The stub-list test (§5) defines runtime state as "the state
page or the TCB beyond the stack" and misses module globals. The lazy-global
claim ("running its initializer is `Time.Wait` (its once-lock)") has no
backing: no sema source records `Time.Wait` for a lazy global.

**Verdict.** Fails. `ET_EXEC` holds (`StaticLink.jet:1833`: "a static
non-PIE executable"). Gaps G10, G11.

### C9. Toolchain identity

A digest match fixes type layouts and the runtime object's layout. Holds.

### C10. Runtime stubs

**Attack.** The list covers allocation, refcounts, stop polls, panics and
I/O. It misses: stack segments (`StackSegment.jet`: a body that can reach
itself runs `cmp rsp, gs:[STACK_TCB_LIMIT]; jb grow` and maps a segment,
writing TCB words; this works in the fork because it is thread-local and
uses `mmap`, which seccomp allows, but it contradicts §6.3's "a stack
overflow faults"), and module globals (C8). The "touches no runtime state"
test needs a definition that includes globals.

**Verdict.** Holds only within stated limits. Gap G11.

### C11. Fork injection

**Holds:** `orig_rax = -1` and the restore of the `-ERESTART*` result is the
technique gdb uses for inferior calls; `CLONE_PARENT` with exit signal 0
keeps the fork out of the target's `waitpid(-1)`; `jet_sys_wait_raw` waits
with options 0 (`HostProcess.jet:301-302`), confirmed.

**Attacks.** (a) Resuming the selected thread to execute the injected
`syscall` can deliver a signal pending on that thread: the tracer sees a
signal-delivery-stop and must suppress and re-queue it, or the target runs
its handler. The proposal claims "same pending signals" without that step.
(b) `PTRACE_O_TRACECLONE` set for the injection also reports the target's
own clones; it must be restored, or be already set by `jet debug`. (c) The
attach case leaves a zombie only a `__WALL` waiter sees, stated but not
removed.

**Verdict.** Holds only within stated limits. Gap G12.

### C12. Threads and locks

**Holds:** in the fork no runtime lock is reachable through eval code, and a
recorded wait is refused statically. **Attacks:** C7 makes image locks
reachable through function values. Layer 3 detects a spin only at 2 s. lldb's
`--try-all-threads` and JDWP's resume-all answer the same problem by letting
the holder run, which a fork cannot.

**Verdict.** Holds only within stated limits, after G9.

### C13. Descriptors, signals and shared memory

**Attacks.** (a) `MADV_DONTFORK` mappings are absent in the fork (a read
faults) and `MADV_WIPEONFORK` mappings are zero-filled (a read returns zeros
silently). `/proc/<pid>/smaps` `VmFlags` shows them (`dc`, `wf`); the design
must refuse reads there. (b) Under `vm.overcommit_memory=2` a fork of a large
target fails with `ENOMEM`; the design has no fallback. (c) The comptime
filter builder (`ComptimeWire.jet`) is built for the comptime child's cookie
and refusal handler; reuse needs a plain allow-list form, and the action must
be `SECCOMP_RET_TRAP` caught at the signal-delivery-stop (to name the
syscall) rather than letting the inherited `SIGSYS` handler, which is image
code, run. (d) §6.2 injects `rt_sigprocmask` "before the call", but the
filter it installed first does not allow `rt_sigprocmask`; the order must be
mask, then filter.

**Verdict.** Holds only within stated limits. Gap G14.

### C14. The call's stack

Stack checks grow onto a segment (C10), so the eval runs to the thread's
stack budget before it faults; the fault is reported. A stop inside a
function's prologue or inside the `grow` trampoline has a half-built frame
below `rsp`; the 128-byte skip is not enough there only if the trampoline is
mid-switch ([INFERENCE]). **Verdict.** Holds only within stated limits.

### C15. Results and `debug_view`

A `debug_view` that calls a stored function value hits C7. **Verdict.**
Fails until G9.

### C16. Dumps and replays

"That host is specified with R9 under its ballot." **Verdict.** Fails: a
claimed surface with no design is missing context. Gap G15.

### C17. Latency

**Measurement.** lldb: 6–24 ms for one to five new call expressions (above).
Jet: the first evaluation of a new expression costs one check of the stopped
package plus an O0 build of the closure's call graph. Measured here with the
gate-p293 seed compiler, one run each, on a loaded machine:

| Program | Command | Wall |
|---|---|---|
| 9- to 16-line probe files | `jet check` | 60–233 ms |
| the same probes | `jet run` (check, O0 build, run) | 61–69 ms |
| one file of the JetCLI package (`ProveObligations.jet`) | `jet check` | 9.95 s |

For a small program the compile path is in lldb's range. For a large
package (the compiler's own CLI) a new expression costs about 10 s before
the O0 build, three orders of magnitude over lldb's `expr`. A watch costs
one fork per stop; fork time grows with the target's page tables, and no
figure for it exists yet.

**Attack.** The proposal claims no figure, which is honest, but it also
offers no interpreted fast path: lldb answers `x + 1` without compiling
anything. Its cache key includes the line, so a watch stepped through a
loop body is a new key at every new line and pays the package check again.
That contradicts §8's "after a watch's first stop its object is cached, so a
step costs the fork plus the calls".

**Verdict.** Holds only within stated limits: the fork's cost is
unmeasured, and for a large package a new expression or a watch at a new
line costs about 10 s. Gap G16.

### C18. I9: one meaning at every level

**Attack.** The expression is checked once, but what it reads depends on the
level: C2's overlay move problem changes with inlining (an O2 image may have
reused a moved slot, an O0 image keeps it), and C6's imprecise O2 ranges
give values from another source point. "Never what an expression computes"
holds only for captures whose O2 value is exact at the stop, which nothing
checks.

**Verdict.** Fails until G1 and G8 land.

### C19. Alternatives considered

Fair to in-target evaluation, the comptime-child copy and `vfork`. Missing:
lldb's interpret-first hybrid (an interpreter for the call-free subset over
target memory, compiled code only for calls), rr's diversion sessions, and
gdb `checkpoint` as prior art. The "second evaluator" objection to an
interpreter does not apply if the interpreter executes the compiler's own IR
for the expression (one front end, one IR, two back ends), which is what
lldb does. **Verdict.** Holds only within stated limits. Gap G17.

## Gaps for cards

Each needs a card or ballot (this worker cannot write Tower):

- **G1** The overlay must not change sema's last-use decisions: a debugger
  read is not a use for move inference, and a capture of a local the
  original program moved before the stop is refused (with a proof case
  built from `overlay_move.jet`).
- **G2** Insertion points for loop headers, multi-line expressions,
  single-line arms and outer frames' mid-line return addresses.
- **G3** A closure form whose result type sema infers under a written row.
- **G4** An explicit empty ceiling for the eval closure (for example the
  body inside a bare `#FX { … }` block plus the denials), with a proof case
  that `print`, `files.read_text` and a `Net` call are refused by sema.
- **G5** Reading `Shared<T>` contents: a debugger-only read that is not
  `Time.Wait` when the fork finds the lock word free, or an explicit stated
  refusal with its cost.
- **G6** Per-file source hashes in the image (DWARF 5 `DW_LNCT_MD5` or a
  Jet section), then the c31 check.
- **G7** `DW_TAG_template_type_parameter` per generic instance, and a count
  of how many stops in a real program land in generic frames.
- **G8** O2 location precision: a proof that a located O2 capture equals
  the O0 value at the same stop, or a refusal where it cannot.
- **G9** Calls through function values and trait objects read from the
  target: map the code address to its function and call the eval object's
  copy; refuse an unmapped address.
- **G10** Data symbols for module globals (c30), or another table the
  debugger can bind through.
- **G11** The stub-list test counts runtime module globals as state; the
  eval object must bind them to the target's or refuse.
- **G12** Pending-signal handling during clone injection, and restoring
  `PTRACE_O_TRACECLONE`.
- **G13** R7 in-program conditions: one meaning with the fork (allocation,
  panic, `Shared` reads).
- **G14** Fork preparation: `dc`/`wf` mappings refused, strict-overcommit
  fallback, a plain allow-list seccomp builder with `RET_TRAP` caught by the
  tracer, and mask-before-filter order.
- **G15** The dump and replay host.
- **G16** Latency rows with lldb figures, and an interpreted fast path or
  check reuse with a bar.
- **G17** Alternatives: lldb's interpret-first hybrid, rr diversions, gdb
  `checkpoint`.

## Probes

All probes ran with the gate-p293 seed compiler, `NO_COLOR=1`, one at a
time under a memory cap. Sources are given whole so they can be rerun.

`row_io.jet`:

```jet
// Probe: does a row that only denies refuse a positive root (IO)?
peek: fn(total: Int) -[!Time.Wait, !Unsafe]> Int {
    print("side effect")
    total + 1
}

run: fn() {
    print(peek(41))
}
```

`jet check`: "no problems". `jet run`: `side effect`, then `42`.

`overlay_move.jet`:

```jet
// Probe: in the original program `Box{items: xs}` is the last use of `xs`,
// so it moves `xs` into the box. The overlay closure adds a later read of
// `xs`, which turns that move into a copy: the overlay is a different program.
Box: struct {
    items: [Int],
}

run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    box := Box{items: xs}
    box.items.push(4)
    jet_debug_eval_0: fn() -[!Time.Wait, !Unsafe]> Int { xs.len() }
    print(box.items.len())
    print(jet_debug_eval_0())
}
```

`jet check`: "no problems". `jet run`: `4`, then `3`. The same program with
`consume: fn(xs: &&[Int]) -> Int { xs.len() }` and `n: consume(xs)` in
place of the box is E0209.

`shared_read.jet`:

```jet
// Probe: reading a field of a Shared struct inside a closure denied Time.Wait.
Stats: struct {
    hits: Int,
    names: [String],
}

run: fn() {
    cell: Shared{Stats{hits: 1, names: [String]{"a"}}}
    jet_debug_eval_0: fn() -[!Time.Wait, !Unsafe]> Int { cell.hits + cell.names.len() }
    print(jet_debug_eval_0())
}
```

`jet check`: E0747, "This callback uses the effect `Time.Wait`, which the
parameter doesn't allow".

Image probes on the gate-p293 `jet` executable: `readelf -h` (Type `EXEC`),
`nm` (43434 symbols, 2 of them data symbols), `readelf --debug-dump=info`
(no `DW_TAG_template_type_parameter`).

lldb measurement: `cc -g -O0` of a 16-line C program (`square`, a 16-int
`malloc` buffer, a loop), then `lldb -b -o 'breakpoint set --file prog.c
--line 13' -o run [commands] -o kill ./prog` timed with and without the
commands, three rounds.
