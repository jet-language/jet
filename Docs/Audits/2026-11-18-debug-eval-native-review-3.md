# Adversarial review of the native debug evaluator proposal, revision 3 — 2026-11-18

Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: IfHole-6-1-3.

Reviewed: `Docs/Proposals/2026-11-18-debug-eval-native.md` revision 3 (#6841
R3 and R8), as submitted on base `ebce85f295`, with its committed probes in
`Docs/Proposals/DebugEvalNative20261118/Probes/`. Probes ran against the
tree at `4171547dc0` and the gate-p293 `jet` image. Every source file this
review cites is the same at `ebce85f295`. It follows
`Docs/Audits/2026-11-18-debug-eval-native-review.md` and
`Docs/Audits/2026-11-18-debug-eval-native-review-2.md`, and AGENTS.md
"Adversarial design review".

Method, as in review 2:

- each answer N1-N10 is attacked and gets a verdict;
- each claim C1-C19 and each gap G1-G17 the second review left failed or
  open is checked again against code or a probe;
- the proposal's probes are rerun with its runner, `run_all.py`;
- new findings are numbered P1-P8, so they do not collide with N1-N10.

The peers are the earlier reviews' peers: lldb with its IRInterpreter, gdb
`compile`/`checkpoint`, rr diversions, Delve `debugCallV2`, JDWP
`InvokeMethod`, and .NET func-eval with `nse`. Missing context counts as a
failure.

## Verdicts

| # | Claim | Review 2 | Revision 3 |
|---|---|---|---|
| C1 | One evaluator for every surface, R7 included | holds only within stated limits | holds only within stated limits (P7) |
| C2 | The expression gets the compiler's own scope and ownership state | fails | fails (P1) |
| C3 | The row refuses every positive root and the escapes | fails | holds only within stated limits (P3) |
| C4 | Evaluation is refused when the source changed since the build | fails | holds only within stated limits (P6) |
| C5 | Generic frames | holds only within stated limits | holds only within stated limits |
| C6 | O1 and O2 locals | holds only within stated limits | holds only within stated limits (P2) |
| C7 | No image code runs | fails | holds only within stated limits (P5) |
| C8 | Module globals bind to the image | holds only within stated limits | holds only within stated limits (P8) |
| C9 | A toolchain digest match guarantees matching layouts | holds | holds |
| C10 | Runtime stubs and the test that enforces them | holds only within stated limits | holds only within stated limits (P3, P4, P5) |
| C11 | The injected `clone` leaves the target as stopped | holds | holds |
| C12 | No deadlock, no blocking wait, a budget for the rest | holds only within stated limits | holds only within stated limits (P3) |
| C13 | Descriptors, signals, timers and shared memory | holds only within stated limits | holds |
| C14 | The call's stack | holds | holds |
| C15 | Results and `debug_view` run no target code | fails | holds only within stated limits (P5) |
| C16 | Dumps and replays | holds only within stated limits | holds |
| C17 | Latency and R7 | holds only within stated limits | holds only within stated limits (P7) |
| C18 | I9: one meaning at every level | fails | fails (P1; P2 unproven) |
| C19 | Alternatives considered | holds | holds |

Two claims still fail: C2 and C18, both through P1. In a method chain the
stop's (line, column) does not say which call an outer frame is in, so the
probe can read the wrong move state and show a moved local. The fix is
small (P1), but until it is written the design can show a wrong value in a
case §3.1 says it covers. C3, C4, C7 and C15 move from fail to holds only
within stated limits. #6841 c40 is not met yet. Revision 4 needs P1 and
should answer P2-P8.

Every revision 3 probe reproduced, except one side remark: §3.7 says that
with the current tree `place_moved` names the type `Box`. With gate-p293 it
still prints `module:14:source-root:Box`, so the type name depends on the
compiler build. This does not affect any verdict.

## Answers N1-N10

| Answer | Verdict | Why |
|---|---|---|
| N1 move rows | holds at O0 | `place_moved` and `moved_read` reproduce. Rows from the final walk, read before every local, answer review 2's N1. The O1 and O2 sentence is unproven (P2). |
| N2 return-address position | fails | `take_arg`, `take_arg_later` and `move_then_share` reproduce. The rule does not identify the call in a chain (P1). Taking refusals from the final walk is right. |
| N3 `-[]>` with `!Unsafe`, waits at the link | holds only within stated limits | `row_compose`, `unsafe_callee`, `shared_denied`, `unsafe_reach` and `wait_link` reproduce. Waits are refused only because each wait path happens to touch runtime state (P3). The link also refuses every `??` over an `E!` result (P4). |
| N4 eval build of the runtime | holds only within stated limits | `asmcalls` and `rtindirect` reproduce. The scan finds only `call reg`. A `ret` into a loaded stack escapes it (P5). |
| N5 source table | holds only within stated limits | `files.py` reproduces. "Exactly the inputs that decide the image" is false. The store key also hashes the build options and the script dependencies, and the table leaves both out (P6). |
| N6 R7 IR scan | holds only within stated limits | `r7_scan` reproduces 4 of 6. The rule's "writes no memory" proof does not exist. The per-breakpoint arena races across threads (P7). |
| N7 `gs_base` | holds | `tcb.py` reproduces: `fs_base` 0, `gs_base` inside a 1 MiB anonymous `rw-p` mapping. |
| N8 seccomp argument checks | holds | `seccomp_args` reproduces: the 4 forms pass and the 4 escapes get `EPERM`. |
| N9 `dc` hole reservation | holds | `dontfork` reproduces both halves, including `/proc/<pid>/mem` and `PTRACE_PEEKDATA` reading zeros, which is why reads use only `process_vm_readv`. |
| N10 globals by declaration site | holds only within stated limits | The instances of one generic module share every declaration site (P8). |

The compiler defects in §3.4 reproduce exactly:

- `fx_forms`: `MarkerArgumentMismatch` at 11:6;
- `fx_line`: `InvalidScopedPolicy` at 6:5 and 11:5;
- `row_forms3`: the row prints as `-[!Time.Wait, Unsafe]>`.

The spec text they contradict is real: spec.md "Effects" allows "an
`#FX: (...)` line in a body" with `!E` denials. Each needs its own card, as
§3.4 says.

## Gaps G1-G17

| Gap | Status | Why |
|---|---|---|
| G1 | closed within limits | Move rows from the final walk cover the Locals pane, place paths and captures (N1). The position that selects the rows is P1. |
| G2 | open in part | Loop headers, multi-line expressions, one-line arms and outer frames of a plain call are covered. Outer frames in a method chain are not (P1). |
| G3 | closed | Unchanged. |
| G4 | closed | Positive roots, a package's `#Unsafe` raw write and an impure callee are refused, and waits are refused at the link (P3 is about how). |
| G5 | closed | Unchanged. `shared_denied` confirms why `!Time.Wait` cannot be in the row. |
| G6 | open in part | Files that emit no function are now covered. Build options and script dependencies are not (P6). |
| G7 | closed as a plan | Unchanged. |
| G8 | closed within limits | The parity test does not cover move rows (P2). |
| G9 | closed within limits | The runtime's register calls are routed. Its `ret`-into-a-stack rail is not (P5). |
| G10 | closed within limits | DIE binding by declaration site; generic module instances (P8). |
| G11 | closed | Unchanged. P4 is the cost of this rule, not a hole in it. |
| G12 | closed | Unchanged. |
| G13 | open in part | The rule now places real conditions, but it rests on an unproven runtime property and a shared arena (P7). |
| G14 | closed | N8 and N9 reproduce. |
| G15 | closed | N7 reproduces. |
| G16 | closed | Unchanged. |
| G17 | closed | Unchanged. |

## Findings

### P1. A (line, column) does not say which call of a chain a frame is in (C2, C18, G2)

**Claim** (§3.1). "A pc that is a return address gets the row of `pc - 1`,
which is the call expression's." "The probe sits at the innermost syntax
node that starts at that (line, column)", and for a return-address pc it
sits "after the call's arguments". "That one rule covers every case the
review names."

**Attack.** A method call's span starts at its receiver. Probe
`moved_read`, rerun here, shows it: the origin of `xs.move()` is reported at
`moved_read.jet:9:23`, with the caret over the whole `xs.move()`, and 9:23
is the column of `xs`. So in `a.f(x).g(y)` three nodes start at one
(line, column):

- the receiver `a`;
- the call `a.f(x)`;
- the call `a.f(x).g(y)`.

The innermost node there is `a`, which is not a call. The calls `f` and `g`
have different return addresses but the same row. The row therefore cannot
say whether the frame is inside `f` or inside `g`, and the two positions
have different move states.

Example: `out: builder.add(xs).add(ys)`, where `add` takes its argument and
the stop is in the first `add`. The caller's probe must see `xs` moved and
`ys` live. At a stop in the second `add` it must see both moved. Both
frames have the same (line, column). Either choice gives one of the two
stops the wrong state: a stale `ys` shown, or a live `ys` refused. The
same holds for a binary expression whose left operand starts the node
(`total + consume(xs)`), and for nested calls on one line
(`f(g(xs), h(ys))`), where the three nodes have different start columns but
the frame's position is still "after the arguments of which call".

No `jet debug` probe can show this yet, because the backend writes no
columns. The evidence is the span rule above and the text of §3.1.

**Verdict: fails.** This is review 2's N2 in a form the column rule does
not reach.

**Fix.** A call site cannot be identified by a source start. Give each call
instruction's row the call node's own identity: a call-site row in
`.debug_jetline` (function, call-site index in sema's walk order), keyed by
the return address. Place the probe after that call node's arguments. A
column of the call's own token (the method name or the opening
parenthesis) would also work for chains, but not for two calls of one
callee on one line. The walk-order index works for both.

Smaller point: a move row names the local as `(name, DW_AT_decl_line)`.
Revision 3 adds columns anyway, so the identity should be (name, line,
column). That rules out two same-named locals of one function declared on
one line.

### P2. Move rows at O1 and O2 are asserted, not tested (C6, C18, G8)

**Claim** (§3.7). "The rows come from sema, which is the same at every
level. Only the pc ranges are per image, mapped through the same IR spans
as the line rows. So a moved local reads as moved at O0, O1 and O2 (I9)."

**Attack.** At O1 and O2 a pc range is not a program interval:

- inlining puts one IR span at several pc ranges;
- scheduling and hoisting move a moving expression's first instruction;
- loop rotation splits a range.

So "the pc range where the place is moved, starting at the first
instruction of the moving expression's code" has no single definition
there. The §3.6 parity test compares the rendered values of locals that
both images *locate*. §3.7 ends a moved local's location list at the move,
so after the move neither image locates it, and the test never compares a
moved local. The move rows at O2 are therefore exactly the part the parity
test skips.

**Verdict: unproven.** Captures at O1 and O2 are refused anyway (§3.6), so
this reaches only the Locals pane and place paths of an O2 image.

**Fix.** The parity test also compares, at each shared row, the moved or
live state each image's rows give for every local. Until it passes, O1 and
O2 Locals show a moved state only where the row's pc range maps one-to-one.
Otherwise they show "may have been moved".

### P3. Waits are refused by accident, not by a rule (C3, C10, C12)

**Claim** (§3.4, §6.1). "Every other wait is a runtime entry point that
the eval link's allow-list does not define, so the link refuses it before
anything runs." The allow-list admits a runtime function that passes two
checks (§5): no runtime state, and no loaded code address.

**Attack.** Neither check says anything about waiting. A wait is refused
only when its code also touches runtime state:

- `Sync.jet:84` `sync_futex_wait` fails "no runtime state" only because
  its *timed* branch writes the TCB word at `+216`. Its untimed branch is
  a bare `FUTEX_WAIT_PRIVATE`.
- `Sync.jet:284` `sync_thread_lock`, the runtime's lock acquire, waits
  through `sync_futex_wait`, so it is refused only by that one store.
- `Sync.jet:102` `sync_futex_wait_shared` reads no module global, no
  state-page word and no TCB word, and calls no loaded address. Its one
  callee, `jet_rt_host_can_wait` (`Sync.jet:118`), returns `0`. So it
  passes both checks. Today it is reached only from thread join
  (`Thread.jet:735`), which the stub table refuses by name.

If the timed wait's scratch word moved off the TCB, a refactor with no
debugger in mind, every runtime lock acquire would pass the allow-list.
§6.2's filter allows a private futex wait. A lock that another thread held
at the stop would then block the fork until the 2 s budget, and the
evaluation would report "ran past its budget" instead of naming the wait.
The fork contains it, but the guarantee that "the link refuses every wait,
naming the entry point" would be false without any test failing.

**Verdict: holds only within stated limits.**

**Fix.** Give §5's test a third check: no blocking syscall. That means no
`futex` with a `FUTEX_WAIT*` operation, and no syscall outside the fork's
§6.2 allow-list, in the IR or the asm of anything reachable. The test then
names the wait.

### P4. The link refuses every `??` over an `E!` result (C10, §1)

**Claim** (§1, §5). Ordinary expressions evaluate; the stub table lists
every stateful entry point an expression needs.

**Attack.** Error handling calls the journey (error trace) entry points.
`wait_link`'s IR, rerun here, already shows `rx.receive() ?? 0` calling
`jet_journey_reset`, and `tx.send(7)` calling `jet_journey_frame_text` and
`jet_err_apply_conversion`. Probe `journey.jet` (below) checks four
ordinary fallbacks under `-[]>`:

- `Int.parse(text) ?? 0` calls `jet_int_parse`, `jet_rt_alloc_value`,
  `jet_rt_block_unique`, `jet_rt_free`, `jet_rt_int_clone` and
  `jet_journey_reset`;
- `xs.get(5) ?? 0`, `ages.get("ann") ?? 0` and `xs.first() ?? 0` call only
  `jet_rt_int_clone` and `jet_int_owned_to_i64`.

`jet_journey_reset` (`JourneyTLS.jet:50`) reads the module global
`journey_tls_key` (`JourneyTLS.jet:3`) and a TLS slot. That is runtime
state, so it fails §5's first check. §5's stub table has no journey row. So
the link refuses `print Int.parse(text) ?? 0`, and every `?` or error built
in an expression. In the eval runtime build, `journey_tls_key` is the eval
copy's own global and was never initialized, so linking it unstubbed would
be wrong too. The rule is right. The table is incomplete.

**Verdict: holds only within stated limits.** This refuses and never shows
a wrong value, but §1's "these all work" list implies that ordinary
fallible expressions evaluate.

**Fix.** Add a journey row to §5. The stubs give the fork a journey of its
own in the arena: reset clears it, and frame and origin calls append to it.
Add a case with `Int.parse(text) ?? 0` and one with an `E!` call's `?`
inside a callee.

### P5. A `ret` into a loaded stack escapes the register-call scan (C7, C10, C15, G9)

**Claim** (§4.2, §5). "§5's test keeps the choke point closed: a register
call in any asm body outside these 12 primitives fails it." Probe
`asmcalls` matches `^\s+call (r…|e?x)`.

**Attack.** A register call is not the only transfer to a loaded address.
`Linux.jet:2090` `os_fiber_switch_i64` does
`mov rsp, {target}` … `ret`. That returns to the word on top of a stack
loaded from memory: a fiber's resume address in image code. It is one of
the runtime's closed rails (`X64/InlineAsm.jet:498`), next to the GS-base
write rail (`:504`), which switches the TCB. Neither contains a `call`, so
`asmcalls` and §5's test, as worded, pass them. Today they are reached
through `Fiber.jet:125` `fiber_switch_out`, whose chain reads carrier
state, so the first check refuses it. That is the same accident as P3.

The other forms are clean. Every `call` in `Linux.jet` is `call r11` (19)
or `call rax` (1), and the fiber switch holds its only `ret`. The signal
restorer ends in `rt_sigreturn`. No other OS source holds asm, and no
runtime or Core file declares an `#FFI(asm)` body.

**Verdict: holds only within stated limits.**

**Fix.** State the test as "any transfer to an address not fixed at
assembly": `call` or `jmp` through a register or memory, `ret` after a
write to `rsp`, and a `wrgsbase`. Each such body is either one of the
routed primitives or fails the test. Name the fiber switch and GS rails as
refused.

### P6. The source table leaves out the build options and script dependencies (C4, C8, G6)

**Claim** (§3.5). `jet_cli_build_cache_key` "hashes exactly the inputs
that decide the image", and the table copies "one `(path, sha256)` row per
snapshot file and comptime input". The compiler, runtime and Core rows are
covered by the toolchain digest.

**Attack.** The key hashes more (`BuildCache.jet:190-227`):

- `build_options_key(options)`, which is the profile, every resolved row
  that shapes the image, every `--set` setting value, `observe`, `log` and
  the deep checks (`BuildOptions.jet:706-719`);
- the host's arch and OS;
- each script dependency's content hash (`BuildCache.jet:216-218`);
- the hot-text list.

The toolchain digest covers none of these, and §3.5 copies none of them.
They matter for two of the design's premises:

- §4.3 compiles immutable globals in because "the build computes the same
  bytes". A global computed at build from a `--set` setting or a profile
  row has other bytes under other options.
- A row with `removes_check` changes what the code does. A check the image
  removed would panic in the eval copy, or the reverse.

§4.1 builds the eval object "through the `jet run` path" and never says
with which options.

**Verdict on N5: the "exactly the inputs" claim fails.** C4 holds only
within stated limits, because a source edit is caught.

**Fix.**

- Write the options key and its resolved rows into `.debug_jetline` beside
  the source table, plus the script dependency rows.
- Build the eval object with the image's options, changing only the
  optimization level, and say why that one cannot change a value.
- Refuse, naming the option, when the session's options differ from the
  image's.

### P7. R7 rests on a property no test proves, and on one arena per breakpoint (C1, C17, G13)

**Claim** (§8.1). A lambda is placed only if its runtime calls are the
result allocation "and runtime functions that §5's test proves write no
memory". The result goes to an arena "that the debugger maps once, when the
breakpoint is set, and resets at each hit".

**Attack.**

- §5's test proves two things: no runtime state and no loaded code
  address. Neither is "writes no memory". A runtime function with no state
  can still store through an argument. `r7scan.py` admits that its
  `READERS` set (`jet_char_len`, `jet_rt_atomic_bool_load`) "stands in for
  §5's runtime test". The property R7 relies on has no checker.
- A breakpoint in code that several threads run is hit by them at once.
  Placed code runs in each hitting thread while the others keep running.
  One arena reset at each hit lets one thread's reset reclaim the result
  another thread is still reading.

**Verdict: holds only within stated limits.**

**Fix.**

- Define "writes no memory" as a third property of §5's test: no `store`
  except into blocks the function allocated, and no store-performing asm.
  Mark it per entry point and make R7 read that mark.
- Give each thread its own arena slot, for example indexed by a TCB word
  the debugger reserves, or allocate atomically and reset only when no hit
  is in flight.

`fork_cost` was not rerun for this review: a 2 GiB resident run was outside
the memory rule in force while it was written. The figures are the
proposal's.

### P8. Instances of a generic module share every declaration site (C8, G10)

**Claim** (§4.3). A bound global's DIE carries "`DW_AT_name`,
`DW_AT_decl_file` and `DW_AT_decl_line`, the identity §4.2 uses for
functions". The eval object refers to the global by its declaration site.

**Attack.** spec.md "Generic modules": a generic module's body "may contain
… module-global values", and applying it creates a separate module per
instance. An instance's members are "its template's text". So
`small: counter<Int>(8)` and `large: counter<Int>(64)` each have their own
`hits := 0` storage with one declaration site. Binding by site would bind
one instance's read to the other's storage, which is a wrong value. §4.2
already adds "for an instance, its template parameters" for functions. §4.3
does not do the same for globals.

`globals.py`, rerun here over `Compiler` and `Core`, gives 159 mutable keys,
with the one string-literal false positive. The proposal's count is 166, on
a later tree. A wider run over `Compiler`, `Core`, `Jetpack`, `Tools` and
`Examples` finds 684 names and 39 shared keys. Each is a `package.jet` or
`run.jet` header name, an example that shares a Core file name, or
same-file string-literal fixtures. None is a real collision. The tree
also has no top-level global holding a `Shared` cell, so §4.3's third row
has no fixture.

**Verdict: holds only within stated limits.**

**Fix.** A global in a generic module instance carries the instance's
identity (D-GENMOD-IDENTITY1, or the instance line) in its DIE and its
undefined symbol. Plan step 1 adds a case with two instances of one module
global and a case with a top-level `Shared` global.

## Claims rechecked

- **C1.** Every surface goes through one path. R7 has P7.
- **C2.** Fails through P1. The position probe, with no overlay and with
  refusals taken from the final walk, is sound. `take_arg` and
  `move_then_share` show the states it must reproduce.
- **C3.** Positive roots are refused (`pure`, `roots`, from review 2's
  reruns). A package's `#Unsafe`, directly and in a callee, is refused
  (`row_compose`, `unsafe_callee`). Core's internals still pass
  (`unsafe_reach`). The `!Unsafe` composition needs no source spelling,
  which keeps it clear of the `#FX` body-form defects. Waits: P3. A foreign
  call is a positive root (`FFI`, spec.md "Effects"; one effect leaf per
  foreign declaration, D-FFI-UNIFY1), so `-[]>` refuses a direct foreign
  call before the item set needs a body for it. This is from the spec, not
  a probe.
- **C4.** Source edits, types-only files included, are caught by the table.
  Options are P6.
- **C5.** Unchanged: refused until template parameters land.
- **C6.** Unchanged, plus P2.
- **C7, C15.** The eval runtime build routes every register call (P5 for
  the rest). Foreign code is refused by the row (C3).
- **C8.** P8. The compiled-in path also depends on P6.
- **C9.** Holds.
- **C10.** P3, P4 and P5 are each a gap in the test's wording or the stub
  table, not in the allow-list idea.
- **C11.** Holds.
- **C12.** Layer 2 holds today, by P3's accident. Layer 3, the budget,
  catches what slips.
- **C13.** N8 and N9 reproduce. A second-wall caveat, not a finding: the
  filter allows `munmap` and `MAP_FIXED`, so code in the fork could unmap a
  reserved hole and map over it. Only eval stubs map memory, and package
  `#Unsafe` is refused, so no eval code can issue those calls.
- **C14.** Holds.
- **C16.** Holds. The dump host needs only the selected thread's registers
  and `gs_base`, and R9 carries them.
- **C17.** P7. The fork cost figures are the proposal's (not rerun).
- **C18.** Fails through P1 at O0. P2 at O1 and O2 is unproven.
- **C19.** Holds.

## Cards

Each finding needs a card or a revision of the proposal (this worker cannot
write Tower):

- P1: call-site rows keyed by return address; local identity with column;
- P2: move-row parity at O1 and O2;
- P3: a "no blocking syscall" check in the eval link test;
- P4: journey stubs in the eval link;
- P5: the transfer scan covers `ret` on a switched stack and the GS rail;
- P6: build options and script dependencies in `.debug_jetline`;
- P7: R7's writes-no-memory property and per-thread arenas;
- P8: generic module instance identity for bound globals;
- the three `#FX` compiler defects of §3.4 (one card each, as the proposal
  says).

## Probes

All probes ran one at a time with the gate-p293 seed compiler,
`NO_COLOR=1`, at `nice 19`, under a 4 GB memory cap. The repository root
given to the runner was the tree at `4171547dc0`.

**The proposal's runner.** `python3 run_all.py <jet> <repo>` ran in the
proposal's `Probes/` directory, except for `fork_cost.py`. Every result
matches `run_all.log`. The differences come from the trees and do not
change a verdict:

- `place_moved` names the type `module:14:source-root:Box`, not `Box`;
- `rtindirect`: 7209 runtime functions, 61 of them call a loaded address,
  and 2173 of 3252 exported entry points reach one (the log has 7274, 61
  and 2175 of 3253);
- `globals.py`: 159 mutable keys (the log has 166);
- `tcb.py`: a different `gs_base` address, in the same 1 MiB `rw-p`
  mapping shape.

The `moved_read` output used in P1:

```text
Error [jet.UseAfterMove]: `xs` was consumed by `xs.move()`, so it can't be used here
  --> moved_read.jet:11:11
  --> moved_read.jet:9:23
    |
  9 |     box := Box{items: xs.move()}
    |                       ^^^^^^^ `xs` moved to `xs.move()` here
```

**`journey.jet`** (P4). `jet check journey.jet` reports no problems.
`jet run journey.jet` prints `16`. Then `jet emit --ir journey.jet`, and
`calls.py` on the listing:

```jet
// Review 3 probe: which runtime entry points do ordinary failable handling
// forms reach? Each peek is an expression a user would type at a stop.
peek_parse: fn(text: String) -[]> Int { Int.parse(text) ?? 0 }

peek_get: fn(xs: [Int]) -[]> Int { xs.get(5) ?? 0 }

peek_key: fn(ages: [String:Int]) -[]> Int { ages.get("ann") ?? 0 }

peek_first: fn(xs: [Int]) -[]> Int { xs.first() ?? 0 }

run: fn() {
    ages: [String:Int]{"bob": 3}
    print(peek_parse("12") + peek_get([Int]{1, 2}) + peek_key(ages) + peek_first([Int]{4}))
}
```

```python
# Review 3 probe helper: each function of an IR listing whose name holds
# `part`, and the runtime entries it calls (as run_all.py's runtime_calls).
# Usage: python3 calls.py <listing> <part>
import re
import sys

current = None
for line in open(sys.argv[1], encoding="utf-8"):
    head = re.match(r"^fn (\S+) ", line)
    if head:
        current = head.group(1) if sys.argv[2] in head.group(1) else None
        continue
    target = re.search(r"call_runtime .*?(jet_\w+) \(", line)
    if current and target:
        print("  {} -> {}".format(current, target.group(1)))
```

Output:

```text
  peek_first -> jet_rt_int_clone
  peek_first -> jet_rt_int_clone
  peek_get -> jet_int_owned_to_i64
  peek_get -> jet_rt_int_clone
  peek_get -> jet_rt_int_clone
  peek_key -> jet_rt_int_clone
  peek_parse -> jet_int_parse
  peek_parse -> jet_rt_alloc_value
  peek_parse -> jet_rt_block_unique
  peek_parse -> jet_rt_free
  peek_parse -> jet_rt_int_clone
  peek_parse -> jet_journey_reset
  peek_parse -> jet_rt_int_clone
```

**Source reads.** These are plain reads, with no tool output to rerun:

- P3: `Compiler/JetRuntime/Source/Sync.jet` lines 84-99, 102-107, 118 and
  284-305, and `Thread.jet:735`;
- P4: `JourneyTLS.jet` lines 3 and 50-53;
- P5: the instruction census of `Compiler/JetBackend/Source/OS/Linux.jet`
  (every `call` is `call r11` or `call rax`; the only `ret` is the fiber
  switch's, line 2103), and
  `X64/InlineAsm.jet:498-523`;
- P6: `Compiler/JetCLI/Source/CLI/BuildCache.jet:190-227` and
  `Compiler/JetFoundation/Source/Registry/BuildOptions.jet:706-719`;
- P8: spec.md "Generic modules".

The wider globals census (P8) is `globals.py`'s pattern run over
`Compiler`, `Core`, `Jetpack`, `Tools` and `Examples`, outside `.jet`
build directories. It counts every lowercase top-level binding, `Shared`
initializers among them.
