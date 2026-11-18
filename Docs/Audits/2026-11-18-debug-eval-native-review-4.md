# Adversarial review of the native debug evaluator proposal, revision 4 — 2026-11-18

Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: IfHole-6-1-3.

Reviewed: `Docs/Proposals/2026-11-18-debug-eval-native.md` revision 4 (#6841
R3 and R8), as submitted on base `ebce85f295`, with its committed probes in
`Docs/Proposals/DebugEvalNative20261118/Probes/`. Probes ran against the
tree at `4171547dc0` and the gate-p293 `jet` image. The runtime sources
(`Compiler/JetRuntime/Source`) and `Compiler/JetBackend/Source/OS/Linux.jet`
are the same at `4171547dc0` and `ebce85f295`. It follows
`Docs/Audits/2026-11-18-debug-eval-native-review.md`,
`Docs/Audits/2026-11-18-debug-eval-native-review-2.md` and
`Docs/Audits/2026-11-18-debug-eval-native-review-3.md`, and AGENTS.md
"Adversarial design review".

Method, as in review 3:

- each answer P1-P8 is attacked and gets a verdict;
- each claim C1-C19 and each gap G1-G17 the third review left failed or
  open is checked again against code or a probe;
- the proposal's probes are rerun with its runner, `run_r4.py`;
- new findings are numbered Q1-Q4, so they do not collide with N1-N10 or
  P1-P8.

The peers are the earlier reviews' peers: lldb with its IRInterpreter, gdb
`compile`/`checkpoint`, rr diversions, Delve `debugCallV2`, JDWP
`InvokeMethod`, and .NET func-eval with `nse`. Missing context counts as a
failure.

## Verdicts

| # | Claim | Review 3 | Revision 4 |
|---|---|---|---|
| C1 | One evaluator for every surface, R7 included | holds only within stated limits (P7) | holds only within stated limits (Q4) |
| C2 | The expression gets the compiler's own scope and ownership state | fails (P1) | holds only within stated limits (Q1) |
| C3 | The row refuses every positive root and the escapes | holds only within stated limits (P3) | holds only within stated limits (Q2) |
| C4 | Evaluation is refused when the source changed since the build | holds only within stated limits (P6) | holds |
| C5 | Generic frames | holds only within stated limits | holds only within stated limits |
| C6 | O1 and O2 locals | holds only within stated limits (P2) | holds only within stated limits |
| C7 | No image code runs | holds only within stated limits (P5) | holds |
| C8 | Module globals bind to the image | holds only within stated limits (P8) | holds only within stated limits |
| C9 | A toolchain digest match guarantees matching layouts | holds | holds |
| C10 | Runtime stubs and the test that enforces them | holds only within stated limits (P3, P4, P5) | holds only within stated limits (Q2, Q3) |
| C11 | The injected `clone` leaves the target as stopped | holds | holds |
| C12 | No deadlock, no blocking wait, a budget for the rest | holds only within stated limits (P3) | holds only within stated limits (Q2) |
| C13 | Descriptors, signals, timers and shared memory | holds | holds |
| C14 | The call's stack | holds | holds |
| C15 | Results and `debug_view` run no target code | holds only within stated limits (P5) | holds only within stated limits (Q3) |
| C16 | Dumps and replays | holds | holds |
| C17 | Latency and R7 | holds only within stated limits (P7) | holds only within stated limits (Q4) |
| C18 | I9: one meaning at every level | fails (P1; P2 unproven) | holds only within stated limits (Q1) |
| C19 | Alternatives considered | holds | holds |

No claim fails. C2 and C18 move from fail to holds only within stated
limits: call-site rows keyed by return address identify the call of a chain
an outer frame is in, which is what P1 asked for. C4 and C7 move to holds.
The four new findings are each a wording or table error with a mechanical
fix, not a hole in the mechanism:

- Q1: the call index is defined by sema's walk, which has no entry for the
  calls lowering makes;
- Q2: the third check, and the probe for it, read only literal syscall
  numbers, so three of the five entry points it should refuse slip past it;
- Q3: "every function of `Trap.jet`" stubs the runtime's raw word helpers
  and the debug text escape table along with the stop report;
- Q4: the R7 arena word is per TCB, and every task runs on a fiber with a
  TCB of its own, so the arena is per fiber, not per thread.

By this table #6841 c40's verdict condition is met. Q1-Q4 should be
answered in revision 5 or carried as cases by the plan steps that §12
already has (steps 1, 2, 3 and 4).

Every revision 4 probe reproduced. The differences come from the trees and
the compiler build and do not change a verdict (see Probes).

## Answers P1-P8

| Answer | Verdict | Why |
|---|---|---|
| P1 call-site rows | holds only within stated limits | `chain` and `chain_later` reproduce. A row per call instruction keyed by its return address, with (function, index), tells `start.add(xs)` from `….add(ys)`, and a local named by (name, line, column) separates two same-named locals on one line. The index is defined only for calls sema walks (Q1). |
| P2 move-row parity | holds | `take_arg` at O2 reproduces: one frame, `n` and `xs` `<Unavailable>`. The parity test now compares moved state, and until it passes O1 and O2 say "may have been moved" unless the pc range maps one to one. That is a stated limit, not an unproven claim. |
| P3 no blocking syscall | holds only within stated limits | `linkchecks` reproduces: `alloc_lock` and `sync_futex_wait_shared` pass the first two checks and wait. The check's wording and the probe see only literal syscall numbers on `os_syscall_i64` (Q2). |
| P8 instance identity | holds only within stated limits | `genmod` reproduces: `__jet_small_dbump` adds 1 to `const 8`, and the run prints 9, 65, 2. `genmod_mutable` reproduces E0003. So today every instance global is an immutable constant, which the eval object compiles in for the frame's instance. The bound case has no fixture until the compiler takes a mutable module global in a generic module, as §4.3 says. |
| P5 transfer scan | holds | `linkchecks` reproduces the 22 bodies: the 20 register calls, the fiber switch's `ret` on a loaded stack, and `wrgsbase`. The Linux.jet census agrees: every `call` is through `r11` or `rax`, and the only `ret` is the fiber switch's. |
| P6 options, host and script dependencies | holds | `bounds` reproduces: one source digest `e056b6ab32d21633`, two images, and two behaviors (E3010 stop against `went on` and a bounds report). The table now records what the store key hashes, the eval object is built with those rows at O0, and a mismatch is refused naming the option. A build option that removes a check, like `--bounds=report`, is one of those rows, so the O0 eval object removes the same check. |
| P7 writes-no-memory mark, per-thread arenas | holds only within stated limits | `linkchecks` and `r7scan.py` reproduce: 834 entry points pass the three checks, 63 reach a store, and `jet_char_len` writes through `rt_text_cache_store`; R7 places 3 of 6. The arena is per TCB, so it is per fiber (Q4). |
| P8 instance identity | holds within stated limits | `genmod` reproduces: `__jet_small_dbump` adds 1 to `const 8`, and the run prints 9, 65, 2. `genmod_mutable` reproduces E0003. So today every instance global is an immutable constant, which the eval object compiles in for the frame's instance. The bound case has no fixture until the compiler takes a mutable module global in a generic module, as §4.3 says. |

## Gaps G1-G17

| Gap | Status | Why |
|---|---|---|
| G1 | closed within limits | Unchanged, plus P1's (name, line, column) local identity. |
| G2 | closed within limits | Outer frames of a chain are covered by call-site rows. Calls made by lowering need Q1's rule. |
| G3 | closed | Unchanged. |
| G4 | closed | Unchanged. Q2 is about the link's syscall check, not the row. |
| G5 | closed | Unchanged. |
| G6 | closed | P6. |
| G7 | closed as a plan | Unchanged. |
| G8 | closed within limits | P2: moved state is in the parity test, and O1 and O2 hedge until it passes. |
| G9 | closed | P5. |
| G10 | closed within limits | P8. The mutable-instance case waits on the compiler. |
| G11 | closed | Unchanged. |
| G12 | closed | Unchanged. |
| G13 | closed within limits | P7, with Q4's per-fiber correction. |
| G14 | closed | Unchanged. |
| G15 | closed | Unchanged. |
| G16 | closed | Unchanged. |
| G17 | closed | Unchanged. |

## Findings

### Q1. The call index has no entry for calls that lowering makes (C2, C18, G2)

**Claim** (§3.1). Each call instruction's call-site row holds "the call's
index in the order sema's final walk of that function finishes walking each
call's callee and arguments. Lowering attaches the index to the IR call as
it attaches the span … The index needs no source token, so it also covers
the calls a statement makes with none: a loop's per-pass call, an
interpolation's builder calls."

**Attack.** The index is defined by sema's walk, but sema walks only the
source's calls. A call that lowering emits for a construct has no entry in
that walk, so there is no index for lowering to attach. The text names two
such calls as covered without saying which index they get.

Probe `loop_pull` (below) stops in a lambda that `xs.map(...)` calls. `map`
is lowered inline, so `run` calls the lambda directly through a
`call_indirect` that has no source call of its own. sema walks one call
there, `xs.map(fn …)`. `bt` shows `#1 run() at loop_pull.jet:5`, and `f`
shows `5 | total := 0`, the statement before the `map` call on line 6.
So the line rows already place this lowering-made call on the wrong
statement today, and §3.1's index would have nothing to name it by. A user
frame below a lowering-made call is ordinary: a lambda given to any
collection method that lowering expands, and any user code that a built-in
form calls.

**Verdict: holds only within stated limits.** The rows are the right
mechanism, and the case of a plain call or a chain is covered. The index
rule is undefined for every call that has no sema call node.

**Fix.** Define the index over source nodes, not calls: each IR call carries
the index of the source node whose lowering emitted it (the call itself for
a source call; `xs.map(…)` for the lambda calls its lowering makes; the loop
header for a per-pass call; the interpolation for its builder calls). The
probe for a return address sits after that node's operands. Add `loop_pull`
to step 1's cases: the frame's row names the `map` call on line 6, and the
probe sees `total` and `xs`, not `v`. The parity test can assert that every
call instruction of an O0 image has a call-site row.

### Q2. The third check reads only literal syscall numbers (C3, C10, C12)

**Claim** (§5). "Nothing they reach, in IR or asm, issues `futex` with a
`FUTEX_WAIT*` operation, or any syscall outside §6.2's allow-list." Probe
`linkchecks` finds that "two reach a syscall outside the allow-list:
`jet_fd_created_raw` … and `jet_term_write_stdout_line_direct`".

**Attack.** `linkchecks.py` reads a syscall number only where a body writes
`os_syscall_i64(I64{N}, …)`, or where an asm body loads a literal into
`rax`. The runtime issues syscalls in two other forms:

- `os_syscall_stack4_i64(I64{N}, …)` (`Linux.jet:1982`), for example
  `rt_sigaction` (13) at `Alloc.jet:3307`, `Fault.jet:88` and
  `CompilerMapped.jet:443`;
- seven wrappers whose syscall number is their own parameter:
  `host_call` (`HostProcess.jet:4`), `host_call4` and `host_call5`
  (`RouteAFiles.jet:522`, `:528`), `net_socket_call` (`HostSockets.jet:10`,
  the number is its second parameter), `observe_syscall`
  (`ObserveLive.jet:272`), `thread_setxid` (`Thread.jet:1017`) and
  `trap_syscall` (`Trap.jet:20`).

Some numbers no literal fixes at all: `thread_synccall_run`
(`Thread.jet:903`) loads its syscall number from a record in memory, and
second-level wrappers pass a `number` parameter on (`FilesPathOps.jet:616`,
`HostDescriptors.jet:155`, `HostSockets.jet:36`, `HostSpawn.jet:856`,
`RouteSys.jet:573`).

Probe `syscall_wrappers.py` (below) adds both forms to `linkchecks.py`'s
facts and reruns its first two checks. It finds 429 wrapper calls with a
literal number and 17 with a number no literal fixes. Of the 836 exported
entry points that pass the first two checks, 5 reach a syscall outside the
allow-list, not 2:

- `jet_fd_created_raw` (`statx`, 332), as the proposal says;
- `jet_term_write_stdout_line_direct` (`writev`, 20), as the proposal says;
- `jet_rt_host_program_start` (`rt_sigaction`, 13, the stack4 form);
- `jet_socket_listen_raw` (`listen`, 50, through `net_socket_call`);
- `jet_socket_shutdown_raw` (`shutdown`, 48, through `net_socket_call`).

No futex call site has an operation that is not a literal, and no futex
call goes through a wrapper, so the futex half of the check holds as the
probe states it.

The IR test would see the stack4 form if it lists that primitive, but the
check's wording does not say what a syscall whose number is a parameter or
a loaded word counts as. Read per function, the wrapper's syscall has no
number, and the caller's literal is only an argument. Either the test
follows numbers across calls, which the text does not describe, or it
admits the wrapper and misses the syscall. The fork's seccomp filter
(§6.2) is the second wall and would end the fork, so this is a refusal
that comes late, not an escape.

**Verdict: holds only within stated limits.** The proposal's count of
entry points the check refuses is low, and the check is not defined for
non-literal numbers.

**Fix.** State the check over syscall sites: a syscall's number, and a
futex call's operation, must be a constant at the site, or a parameter
whose every caller passes a constant, followed through wrappers. Any other
number (a loaded word, a second-level parameter the check does not follow)
fails the check. Name `os_syscall_stack4_i64` beside `os_syscall_i64` as a
syscall primitive. The test's cases should include the three entry points
above.

### Q3. "Every function of `Trap.jet`" stubs success-path helpers (C10, C15)

**Claim** (§5's table). "panic and the whole stop report: `jet_rt_panic_*`,
`jet_rt_unreachable`, every function of `Trap.jet`, `TrapMessages.jet` and
`TrapDefectMessages.jet` … | Format the message with the eval runtime's own
text code into the result block, and `exit_group`."

**Attack.** `Trap.jet` is not only the stop report. Its first functions are
the runtime's raw word helpers, `trap_word`, `trap_byte`, `trap_store` and
`trap_put` (`Trap.jet:12-18`), which success paths call: outside the three
Trap files, 31 runtime files call `trap_word`, 29 call `trap_store`, 17 call
`trap_byte` and 12 call `trap_put`. `Collections.jet:23-46` reads and grows
a list header through them. `trap_debug_escaped` and `trap_scalar` are the
escape table that `TextDebug.jet:28-32` uses to render text for debug
output. `trap_map`, `trap_unmap` and `trap_syscall` are general mapping and
syscall wrappers.

A stub that formats a stop message and calls `exit_group` in place of
`trap_word` ends the evaluation at the first list header read. A stub in
place of `trap_debug_escaped` ends it when a result's text has a scalar the
debug form escapes. Both evaluations are correct expressions. The probe
treats every function of a stubbed file as passing, so its "refuses 0 of
the 17" does not test whether those 17 reach a Trap helper on their success
path.

**Verdict: holds only within stated limits.** The row is right for the stop
report, but as written it names the wrong set.

**Fix.** List the stop report by function, not by file: the functions a stop
path reaches from `jet_rt_panic_*`, `jet_rt_unreachable` and the `*_stop`
entry points (`trap_stop`, `trap_exit`, `trap_defect_prepare`,
`trap_defect_exit`, `trap_header`, `trap_footer`, the message functions and
the rest). The helpers above must pass the three checks themselves, or get
eval bodies of their own: `trap_word`, `trap_byte`, `trap_store` and
`trap_put` have no runtime state; `trap_syscall` falls under Q2;
`trap_map` and `trap_unmap` are mappings. Rerun `linkchecks` with the
narrowed table.

### Q4. The arena word is per TCB, and every task has a TCB of its own (C1, C17, G13)

**Claim** (§8.1). "Each thread has its own arena. The runtime reserves one
TCB word for the debugger (`TCB_DEBUG_ARENA`). With every thread stopped,
the debugger maps one arena per thread and writes its address into that
thread's word … A thread created after the breakpoint was set has 0 there,
so its hit traps to the debugger, which maps its arena."

**Attack.** `Fiber.jet`'s header says the scheduler "runs every task on a
fiber, a stack of its own with a TCB of its own", and the switch "points GS
at the fiber's TCB". `fiber_new` (`Fiber.jet:511-564`) places that TCB in a
fresh mapping, so its words start at 0. So `gs:[TCB_DEBUG_ARENA]` is the
current fiber's word, not the thread's:

- With every thread stopped, the debugger can reach only the TCBs that the
  threads' `gs_base` names, the fibers running at that moment. Ready,
  parked and pooled fibers keep 0.
- Every task that runs later is on a new or pooled fiber with 0, so its
  first hit traps to the debugger. In a program that runs one task per
  request, with a condition in the handler, that is one trap per request,
  which is the per-hit cost R7 exists to avoid (C17).

It is not a correctness hole: a trap maps an arena, and no two running
fibers share a TCB.

**Verdict: holds only within stated limits.** "Per thread" is per fiber,
and the cost claim does not hold for task-per-request code.

**Fix.** Either say the arena is per fiber, and count the trap per new
fiber in §8.1's costs, or key the arena by carrier: a placed lambda never
parks (it is `-[]>`, with no wait), so the fibers that run one after
another on one carrier can share that carrier's arena. The second form
needs a way for placed code to find its carrier from the fiber's TCB, and
that word is runtime state the "writes no memory" mark must allow. Step
4's case with two threads should have a third hit on a fiber made after the
breakpoint was set.

## Claims rechecked

- **C1.** Every surface goes through one path. R7 has Q4.
- **C2.** Holds within limits. Call-site rows keyed by return address
  answer P1. `chain` shows the states the rows must tell apart: at the
  second stop the outer frame shows `xs = [3, 4]`, a moved list whose
  storage the callee changed. Q1 is the lowering-made case.
- **C3.** Unchanged in the row. The link's syscall check has Q2.
- **C4.** Holds. `bounds` shows why the options belong in the table.
- **C5.** Unchanged: refused until template parameters land.
- **C6.** Holds within limits, with the "may have been moved" hedge until
  the parity test of §3.6 passes (P2).
- **C7.** Holds. The transfer scan covers the two rails review 3 named, and
  both are refused by name.
- **C8.** Holds within limits (P8). The mutable-instance case waits on the
  compiler defect §0.1 names.
- **C9.** Holds.
- **C10.** Q2 and Q3 are each a gap in the test's wording or the stub table,
  not in the allow-list idea.
- **C11.** Holds.
- **C12.** Layer 2 now has a rule (P3), with Q2's gap. Layer 3, the budget,
  and the seccomp filter catch what slips.
- **C13.** Holds.
- **C14.** Holds.
- **C15.** Holds within limits: debug text rendering passes through
  `trap_debug_escaped` (Q3).
- **C16.** Holds.
- **C17.** Q4. The fork cost figures are the proposal's (not rerun).
- **C18.** Holds within limits at O0 (Q1). O1 and O2 are hedged until the
  parity test passes (P2).
- **C19.** Holds.

## Cards

Each finding needs a card or a revision of the proposal (this worker cannot
write Tower):

- Q1: call-site index over source nodes, with lowering-made calls carrying
  their node's index (§12 step 1, `loop_pull` as a case);
- Q2: the no-blocking-syscall check over syscall sites, through wrappers and
  the stack4 primitive, refusing non-constant numbers (§12 step 3);
- Q3: the stop-report stub row by function, not by file (§12 step 3);
- Q4: R7's arena keyed per fiber or per carrier, with its cost stated
  (§12 step 4).

The two compiler defects §0.1 names (a generic module instance member's
breakpoint never hits; a mutable global in a generic module body is
refused) each need their own card, as the proposal says.

## Probes

All probes ran one at a time with the gate-p293 seed compiler,
`NO_COLOR=1`, at `nice 19`, under a 4 GB memory cap (`loop_pull`, after the
owner's order, under 2 GB). The repository root given to the runner was the
tree at `4171547dc0`.

**The proposal's runner.** `python3 run_r4.py <jet> <repo>` ran in the
proposal's `Probes/` directory. Every result matches `run_r4.log`. The
differences come from the trees and the compiler build and do not change a
verdict:

- `chain` names the type `module:14:source-root:Builder`, not `Builder`
  (review 3 saw the same with `Box`);
- `linkchecks.py`: 7297 runtime functions, 3269 exported entry points and
  139 module globals (the log has 7369, 3270 and 147); 101 functions issue
  a syscall outside the allow-list directly (the log has 110); the stub
  tables cover 254 and 548 functions (the log has 260 and 554). Every
  other line, the 22 transfer bodies, the 836, 834 and 63 counts and the
  17 entry points included, is the same. Since this tree's runtime equals
  `ebce85f295`'s, the log was made on another tree;
- `bounds`: the same source digest, `e056b6ab32d21633`, with image digests
  `731be6cc9481d4ee` and `b5af68e77e48eaf6` (the log has `789a73872f4cd3ec`
  and `2c6c1d4adc175d9d`). Both images behave as the log says.

**`loop_pull.jet`** (Q1). `jet debug --break loop_pull.jet:7 loop_pull.jet`
with `bt`, `f`, `locals` and three `c`:

```jet
// Review 4 probe (Q1): an outer frame whose call has no source call of its
// own. The lambda runs when the loop takes its next item.
run: fn() {
    xs: [Int]{1, 2, 3}
    total := 0
    loop v in xs.map(fn(n) {
        n * 2
    }) -> total += v
    print(total)
}
```

```text
Breakpoint hit  loop_pull.jet:7  in run.lambda@206()
7 |         n * 2        <- here
Locals:  closure = <Unavailable>   n = 1
(Jet) bt
#0  run.lambda@206()  at loop_pull.jet:7
#1  run()  at loop_pull.jet:5
(Jet) f
5 |     total := 0        <- here
Locals:  total = 0   xs = [1, 2, 3]
```

The program then stops at `n = 2` and `n = 3`, three times at line 8, and
prints `12`. `jet emit --ir loop_pull.jet` shows the lambda called from
`run` by `call_indirect s33 <- 32 7 2 (s17, s31)` inside the inlined `map`.

**`syscall_wrappers.py`** (Q2). `python3 syscall_wrappers.py
<proposal>/Probes/linkchecks.py <repo>`. It runs `linkchecks.py` for its
tables, adds the stack4 form and the wrappers' numbers to the direct facts,
and reruns the first two checks:

```python
"""Review 4 probe (Q2): the syscalls `linkchecks.py` does not see.

linkchecks.py reads a syscall number only where a body writes
`os_syscall_i64(I64{N}, ...)`. This probe adds the other two forms the
runtime uses: `os_syscall_stack4_i64(I64{N}, ...)`, and a wrapper whose
syscall number is its own parameter (`host_call(number, ...)`), called with a
literal number or with a number that is not a literal. It then reruns
linkchecks.py's first two checks and counts the exported entry points that
pass them and reach a syscall outside the allow-list, or a syscall whose
number no literal fixes.

Usage: python3 syscall_wrappers.py <linkchecks.py> <repo root>"""
import io
import os
import re
import runpy
import sys
from contextlib import redirect_stdout

sys.argv = [sys.argv[1], sys.argv[2]]
with redirect_stdout(io.StringIO()):
    lc = runpy.run_path(sys.argv[0])
bodies, params, files, exported, direct, ALLOWED = lc["bodies"], lc["params"], lc["files"], lc["exported"], lc["direct"], lc["ALLOWED"]

before = lc["closure"]()
base_two = lc["passing"](before, ("state", "loaded"), exported)
base_blocked = {name for name in base_two if before[name]["blocks"]}

stack4 = re.compile(r"os_syscall_stack4_i64\(I64\{(\d+)\}")
raw_site = re.compile(r"os_syscall(?:_stack4)?_i64\(\s*([^,()]+(?:\([^()]*\))?)\s*,")

# Wrappers: a body whose raw syscall number is one of its own parameters, and
# that parameter's position in its signature.
wrappers = {}
for name in sorted(bodies):
    text = "".join(bodies[name])
    for number in stack4.findall(text):
        if int(number) not in ALLOWED:
            direct[name][0]["blocks"].add("{} (syscall {}, stack4 form)".format(name, number))
    for argument in raw_site.findall(text):
        argument = argument.strip()
        if argument.startswith("I64{"):
            continue
        if argument in params[name]:
            wrappers[name] = argument
        else:
            direct[name][0]["blocks"].add("{} (syscall number `{}` fixed by no literal)".format(name, argument))

# The position of each wrapper's number parameter, from its declaration.
head = re.compile(r"^(?:#[A-Za-z]+(?:\([^)]*\))?\s+)*(?:priv\s+)?([a-z_][a-z0-9_]*): fn\(([^)]*)\)")
position = {}
runtime = lc["runtime"]
for file in sorted(os.listdir(runtime)):
    if not file.endswith(".jet"):
        continue
    for line in open(os.path.join(runtime, file), encoding="utf-8"):
        found = head.match(line)
        if found and found.group(1) in wrappers:
            names = [part.split(":")[0].strip() for part in found.group(2).split(",")]
            position[found.group(1)] = names.index(wrappers[found.group(1)])

literal_calls = 0
open_calls = 0
for name in sorted(bodies):
    text = "".join(bodies[name])
    for wrapper, index in position.items():
        if wrapper == name:
            continue
        for match in re.finditer(r"\b" + wrapper + r"\(", text):
            depth, start, args, current = 0, match.end(), [], ""
            for char in text[start:]:
                if char == "(" or char == "{":
                    depth += 1
                elif char == ")" or char == "}":
                    if depth == 0:
                        args.append(current.strip())
                        break
                    depth -= 1
                if char == "," and depth == 0:
                    args.append(current.strip())
                    current = ""
                    continue
                current += char
            if index >= len(args):
                continue
            number = re.fullmatch(r"I64\{(\d+)\}", args[index])
            if number:
                literal_calls += 1
                if int(number.group(1)) not in ALLOWED:
                    direct[name][0]["blocks"].add("{} (syscall {} through {})".format(name, number.group(1), wrapper))
            else:
                open_calls += 1
                direct[name][0]["blocks"].add("{} (syscall number `{}` through {}, fixed by no literal)".format(name, args[index], wrapper))

after = lc["closure"]()
two = lc["passing"](after, ("state", "loaded"), exported)
blocked = sorted(name for name in two if after[name]["blocks"])
print("wrappers whose syscall number is a parameter: {}".format(", ".join("{} ({})".format(w, files[w]) for w in sorted(wrappers))))
print("calls of them: {} with a literal number, {} with a number no literal fixes".format(literal_calls, open_calls))
print("exported entry points passing the first two checks: {} (linkchecks.py: {})".format(len(two), len(base_two)))
print("of them reaching a syscall outside the allow-list or one no literal fixes: {} (linkchecks.py: {})".format(len(blocked), len(base_blocked)))
for name in blocked:
    print("  {}".format(lc["chain"](after, name, "blocks")))
```

Output:

```text
wrappers whose syscall number is a parameter: host_call (HostProcess.jet), host_call4 (RouteAFiles.jet), host_call5 (RouteAFiles.jet), net_socket_call (HostSockets.jet), observe_syscall (ObserveLive.jet), thread_setxid (Thread.jet), trap_syscall (Trap.jet)
calls of them: 429 with a literal number, 17 with a number no literal fixes
exported entry points passing the first two checks: 836 (linkchecks.py: 836)
of them reaching a syscall outside the allow-list or one no literal fixes: 5 (linkchecks.py: 2)
  jet_fd_created_raw -> os_statx_created_i64 (syscall 332)
  jet_rt_host_program_start -> jet_rt_host_program_start (syscall 13, stack4 form)
  jet_socket_listen_raw -> jet_socket_listen_raw (syscall 50 through net_socket_call)
  jet_socket_shutdown_raw -> jet_socket_shutdown_raw (syscall 48 through net_socket_call)
  jet_term_write_stdout_line_direct -> os_write_line_i64 (syscall 20)
```

**Source reads.** These are plain reads, with no tool output to rerun:

- Q2: `Thread.jet:903-907` (`thread_synccall_run` loads its number with
  `sync_load(record)`), `HostSockets.jet:54` and `:134`, `Alloc.jet:3307`,
  and the second-level wrapper calls cited in Q2;
- Q3: `Trap.jet:7-23`, `Collections.jet:23-46`, `TextDebug.jet:28-32`, and
  a `git grep -l -w` of each `Trap.jet` function outside the three Trap
  files;
- Q4: `Fiber.jet`'s header comment (lines 1-34), `fiber_gs_write`
  (`Fiber.jet:224-232`) and `fiber_new` (`Fiber.jet:511-564`).
