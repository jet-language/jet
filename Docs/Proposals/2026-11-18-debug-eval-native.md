# One debugger evaluator: the compiler's code, run in a fork of the stopped program

Status: revision 4 of a design proposal for #6841 R3 (one evaluator) and R8
(visualizers that never run in the target), from
`Docs/Audits/2026-11-18-best-debugger-profiler.md`. It ratifies nothing, and
nothing is built from it until an adversarial review passes.

- The first review, `Docs/Audits/2026-11-18-debug-eval-native-review.md`,
  found that 8 of 19 claims fail, 10 hold only within limits and 1 holds, and
  listed gaps G1-G17. Revision 2 answered them.
- The second review, `Docs/Audits/2026-11-18-debug-eval-native-review-2.md`,
  found that C2, C3, C4, C7 (with C15) and C18 still fail. It left G1, G2 (in
  part), G4, G6, G9 and G13 open, and raised N1-N10. Revision 3 answered
  them.
- The third review, `Docs/Audits/2026-11-18-debug-eval-native-review-3.md`,
  found that C2 and C18 still fail through P1, that the O1 and O2 move rows
  are unproven (P2), and that P3-P8 hold only within limits.

This revision answers P1-P8, each with a probe that was run: a defect
reproduced, or the compiler or runtime behavior the answer relies on, shown
working. §0 lists them. The appendix indexes the probes, which are
committed whole, with their runners and exact output, in
`DebugEvalNative20261118/Probes/` beside this file (`run_all.py` for
revision 3's, `run_r4.py` for this revision's).

The audit's D4 and D10 say a checked expression runs "under the JetEval
interpreter with memory reads served from the target". That premise is
stale. JetEval no longer has an interpreter: comptime code is compiled to
x86-64 at O0 and runs in a child of the compiler (JetEval
`IR/Native.jet`, JetBackend `Image/SessionHost.jet` and
`Image/MemoryLink.jet`). This proposal keeps D4's guarantees:

- the compiler checks the expression;
- the effect checker proves it cannot change the program;
- one evaluator serves every surface.

It replaces the interpreter with the compiler's own native code, run in a
copy-on-write fork of the stopped program.

## 0. What the reviews found and what changed

### 0.1 The third review (revision 4)

| Finding | What failed | Answer | Probe | Where |
|---|---|---|---|---|
| P1 (C2, C18, G2) | A (line, column) does not say which call of a chain an outer frame is in: `a.f(x)` and `a.f(x).g(y)` start where `a` does | Each call instruction gets a call-site row in `.debug_jetline`, keyed by its return address: the function and the call's index in sema's final walk order. A return-address pc puts the probe after that call's arguments. A local is named by (name, line, column) | `chain`: both stops in `add` show `run`'s frame at `chain.jet:18`, and today's Locals pane shows `xs = [1, 2]` at the first and `xs = [3, 4]` at the second, a list the program never built. `chain_later`: sema places the take of `xs` at 18:20 | §3.1, §3.7 |
| P2 (C6, C18, G8) | The O1 and O2 move rows are asserted, and the parity test skips moved locals | The §3.6 parity test also compares each local's moved or live state at every shared row. Until it passes, O1 and O2 show a moved state only where the row's pc range maps one-to-one onto the O0 image's, and "may have been moved at …" elsewhere | `take_arg` at O2: `consume` is inlined into `run`, `bt` has one frame, and `xs` and `n` are `<Unavailable>`. There is no return address for the take, only the moved range | §3.6, §3.7 |
| P3 (C3, C10, C12) | Waits are refused only when they also touch runtime state | §5's test gets a third check: no blocking syscall (a `futex` `FUTEX_WAIT*` operation, or any syscall outside §6.2's allow-list) in anything reachable. It names the wait | `linkchecks`: `alloc_lock` and `sync_futex_wait_shared` already pass the first two checks and wait. With `sync_futex_wait`'s timed scratch off the TCB, 9 functions would, `sync_thread_lock` among them. Two exported entry points that pass the first two checks reach a syscall outside the allow-list (`statx`, `writev`) | §5 |
| P4 (C10, §1) | The stub table has no journey row, so every `??` over a failure is refused | Three rows join §5's table: the journey (a journey of the fork's own in the arena), the whole stop report, and the task rows by name | `journey` and `linkchecks`: under revision 3's table the link refuses 11 of the 17 entry points the peeks reach, `jet_rt_int_clone` among them, because each operation's stop path reaches the stop report. Under this revision's table it refuses none | §1, §5 |
| P5 (C7, C10, C15, G9) | A `ret` on a loaded stack, and `wrgsbase`, escape a scan for register calls | The test scans for any transfer to an address not fixed at assembly: `call` or `jmp` through a register or memory, `ret` after a write to `rsp`, and `wrgsbase`. The fiber switch and GS-base rails are refused by name | `linkchecks`: 22 such asm bodies, the 20 register calls of `asmcalls` plus `os_fiber_switch_i64` and `os_gs_base_write_i64` | §4.2, §5 |
| P6 (C4, C8, G6) | The source table leaves out the build options, the host and the script dependencies | `.debug_jetline` also records the options key with its resolved rows, the host, and each script dependency. The eval object is built with the image's options, with only the optimization level changed. A session whose options differ is refused, naming the option | `bounds`: one source, two images. The stock image stops at `xs[5]` with E3010. The `--bounds=report` image reports it and reads `1` past the list | §3.5, §4.1 |
| P7 (C1, C17, G13) | R7's "writes no memory" has no checker, and one arena per breakpoint races between threads | "Writes no memory" is a fourth property of §5's test, marked per entry point, and R7 reads the mark. Each thread's hit uses its own arena slot, found through a TCB word the debugger reserves | `linkchecks`: of 834 entry points that pass the three checks, 63 reach a store. `jet_char_len`, one of `r7scan.py`'s assumed readers, writes its string's count cache word (`Text.jet:866`). With the mark, `r7scan` places 3 of 6 conditions, not 4 | §5, §8.1 |
| P8 (C8, G10) | Instances of a generic module share every declaration site | A global of a generic module instance carries the instance's identity in its DIE and its symbol, as functions do (§4.2) | `genmod`: two instances of `counter<Int>` compile `HITS` to 8 and 64 from one declaration site, and their members are `__jet_small_dbump` and `__jet_large_dbump`. `genmod_mutable`: a mutable global in a generic module's body is refused today (E0003). A top-level `tally := Shared{…}` builds and runs | §4.3 |

The probes also found two defects that are not this design's:

- A breakpoint on a generic module instance's member never hits, and `s`
  steps over a call of it (`genmod`). A one-line function outside a generic
  module stops (`bounds`, the control).
- spec.md "Generic modules" lets a module body hold module-global values,
  and the compiler refuses a mutable one (`genmod_mutable`).

### 0.2 The second review (revision 3)

| Finding | What failed | Answer | Probe | Where |
|---|---|---|---|---|
| N1 (C2, C18, G1) | A place path skips the check, so `print xs` shows a local the program moved into `box` | Every read of a local goes through the image's move rows: a place path, the Locals pane, and a capture. The rows are written at build from sema's final walk, so a moved local is shown as moved, with its site, and is never read | `place_moved` reproduces the stale read. `moved_read` shows the origin sema records | §3.2, §3.7 |
| N2 (C2, C18, G1, G2) | An outer frame's probe sits before the arguments its call moved. A refusal made mid-walk can name a move that a later use turns into a share | A return-address pc positions the probe after the call's arguments. Refusals come from the final walk, which is the walk the image was built from | `take_arg` reproduces the stale read in an outer frame. `take_arg_later` shows the move at the argument. `move_then_share` shows the share the final walk knows | §3.1, §3.2 |
| N3 (C3, C12, C1, G4) | `-[]>` admits channel waits, atomic stores and `#Unsafe` raw writes | The probe lambda's row is the empty ceiling composed with the deny-only `!Unsafe`, through sema's existing module-rung composition, which needs no source spelling. `!Time.Wait` cannot be in the row, because a `Shared` read carries it. Every wait is a runtime entry point instead, and the eval link's allow-list refuses it before anything runs. Atomic stores pass the row: in the fork they are contained, and R7 never places them in the program (§8.1) | `row_compose` and `unsafe_callee`: the raw write is refused. `unsafe_reach`: ordinary expressions pass. `shared_denied`: `!Time.Wait` refuses `cell.hits`. `wait_link`: the entry points each wait reaches. `row_forms3`, `fx_forms` and `fx_line`: the denial-only and body forms do not work | §3.4, §5 |
| N4 (C7, C10, C15, G9) | The prebuilt runtime object calls code addresses loaded from target memory | The eval link uses an eval build of the runtime, made with §4.2's lowering. Its 12 assembly primitives that call a register go through `jet_eval_call_target`. §5's test fails any other register call | `asmcalls` (20 such asm bodies; the runtime calls 12 of them, plus `os_clone_i64`). `rtindirect` (61 runtime functions call them) | §4.2, §5 |
| N5 (C4, G6) | The digests cover only files that emitted a function | `.debug_jetline` records the path and SHA-256 of every build input: the same rows `jet build`'s cache key hashes. `DW_LNCT_MD5` is dropped | `files.py` on a two-file build: the types-only `shapes.jet` has no line-table entry | §3.5 |
| N6 (C1, C17, G13) | R7's in-program rule places nothing: even `xs.len() > 2` allocates its result | The rule becomes an IR scan of the placed code: only the result allocation, from a debugger-mapped arena, plus runtime readers, with no retain or release, no atomic store, and stores only into its own blocks. The fork-per-hit cost is measured | `r7_scan`: 4 of 6 conditions are placeable, the review's `xs.len() > 2` among them. `fork_cost`: a median 1.59-3.00 ms at 64 MiB resident, and 13.74-400.11 ms at 2 GiB, moving with load and the huge-page share | §8.1, §8.2 |
| N7 (C16, G15) | The dump host restores FS, but the TCB is GS | R9's format carries `gs_base` and `fs_base` for each thread, and both are restored. File offsets are page-aligned | `tcb.py` on a running Jet image: `fs_base` 0, `gs_base` inside a 1 MiB anonymous mapping | §7.1 |
| N8 (C13, G14) | The seccomp filter checks only syscall numbers | Argument checks for `mmap`, `mprotect`, `madvise` and `futex` | `seccomp_args`: the 4 allowed forms pass, and the 4 escapes return the filter's refusal | §6.2 |
| N9 (C13, G14) | A `MADV_DONTFORK` hole can be reused by the fork's own `mmap` | The fork reserves each hole `PROT_NONE` with `MAP_FIXED_NOREPLACE` before any other mapping. Fork memory is read only through `process_vm_readv` | `dontfork`: without the reservation the arena lands in the hole, and every reader sees zeros. With it, the eval code's read and `process_vm_readv` fault, while `/proc/<pid>/mem` and `PTRACE_PEEKDATA` still read zeros | §6.2 |
| N10 (C8, G10) | Global symbols named `<module>.<name>` collide | Bound globals get CU-level `DW_TAG_variable` DIEs with `DW_OP_addr`, and binding is by declaration site | `globals.py`: 166 module-global keys, and no real collision today (its one hit is two script fixtures in string literals). Function names collide 79 times (`elfsyms.py`) | §4.3 |

Two compiler defects that the N3 probes found are named in §3.4. Neither is
on this design's path.

### 0.3 The first review (revision 2)

Rows that revision 3 replaces say so.

| Review | What failed or was missing | Answer | Where |
|---|---|---|---|
| C2, G1 | The overlay closure's capture is a later use, so sema turns the program's move into a copy: the checked program is not the image's | No text is inserted. The compiler checks the expression with a position probe inside the stopped function's own check. The probe's reads take no part in move inference. Revision 3 takes moved-place refusals from the image's move rows (N1, N2) | §3.2 |
| C2, G2 | No insertion point for loop headers, multi-line expressions, one-line arms, or outer frames' mid-line return addresses | The line table gains columns, and the probe sits at the node where the pc's row starts. Revision 3 places a return-address pc after the call's arguments (N2) | §3.1 |
| C2, G3 | `fn() -[…]> <inferred>` is not a Jet form | The expression is checked as a lambda against the expected type `fn() -[]> T`, where T is inferred. A probe shows that a lambda in that position infers its result and takes the bound | §3.2, probes `infer`, `infer2`, `infer3` |
| C3, G4 | A row of denials only has no ceiling, so `print` passes | The row is the empty ceiling `-[]>`, which refuses `print`, `time.sleep`, file reads and writes, environment reads, module writes and impure callees. Revision 3 composes it with the deny-only `!Unsafe`, refuses every wait at the eval link, and drops the "`#Unsafe` blocks refused" row (N3) | §3.4, probes `pure`, `roots`, `row_compose` |
| C3, G5 | A `Shared` read is refused (E0747 under `!Time.Wait`) | Under `-[]>` a `Shared` read passes the row (probe `pure`). The eval stub for the cell's lock takes a free lock in the fork and refuses a held one, naming it | §3.4, §5 |
| C4, G6 | No source hash is recorded | Revision 2 used `DW_LNCT_MD5`. Revision 3 replaces it with a digest of every build input in `.debug_jetline` (N5) | §3.5 |
| C5, G7 | Generic frames have no type arguments | Template parameters come first in the plan. The probe finds that no symbol names an instance's arguments and that 79 names are shared by several functions, so this is required for G9 as well | §3.3 |
| C6, C18, G8 | No `DW_OP_piece`; an O2 location may hold a value from another source point | The piece claim is removed. O1 and O2 captures are refused until a located-value parity test passes for the backend | §3.6, §9 |
| C7, C15, G9 | Function values and trait objects run image code | Eval builds lower every indirect call through a translation: an image code address maps through its subprogram DIE to the eval object's own copy, built on demand. An unnamed address is refused. Revision 3 extends this to the runtime object (N4) | §4.2 |
| C8, G10, G11 | The image has 2 data symbols, so globals can't be bound, and the runtime's globals are the copy's | Immutable globals are compiled in, since they are the same bytes by D-MODULE-BUILD1. Mutable and `Shared` globals bind to the image's storage; revision 3 binds them through CU-level global DIEs by declaration site (N10). The stub test counts runtime module globals as state. The lazy-global claim is removed | §4.3, §5 |
| C10, C14 | Stack segments grow, against "a stack overflow faults" | The call runs under the program's own stack growth. Past the thread's budget it faults and is reported | §6.3 |
| C11, G12 | Pending signals and `PTRACE_O_TRACECLONE` | A signal-delivery-stop during the injection is suppressed and delivered again on resume. The options are saved and restored | §6.0 |
| C13, G14 | `dc`/`wf` mappings, strict overcommit, the filter builder, the mask-then-filter order | All four are specified. Revision 3 adds the filter's argument checks (N8), the `dc` hole reservation and the `process_vm_readv`-only read rule (N9) | §6.2 |
| C16, G15 | Dumps and replays have no design | A replay stop is a live process and is forked like one (an rr diversion). A dump is hosted by the image, run under the tracer with the dump's memory mapped privately at its addresses. Revision 3 restores `gs_base`, the TCB's base (N7) | §7.1 |
| C17, G16 | About 10 s for a new expression in a large package; a watch pays again at every new line | A place path (`x`, `p.field.sub`) is rendered with no compile, after a lookup in the move rows (N1). One probe check covers every position of every function on the stack, so the cache key has no line. The DebugLatency rows carry lldb figures and bars | §8 |
| C1, G13 | R7's in-program conditions have another meaning | Revision 3 replaces the reach rule with an IR scan of the placed code (N6). Otherwise each hit uses the fork, and the user is told so | §8.1 |
| C19, G17 | lldb's interpret-first hybrid, rr diversions and gdb `checkpoint` are missing | Added and answered | §11 |

## 1. What changes for the user

There is no new command, flag or syntax. Everything that evaluates an
expression goes through this one path:

- console `print` and `watch`;
- breakpoint conditions and logpoint templates;
- DAP `evaluate` (`setVariable` is excluded because it writes: §9);
- a type's `debug_view` (R8).

Today `print` takes only a local's name (`Native.jet`
`native_debug_print`). DAP conditions and `evaluate` run a second
evaluator, `JetCLI/Source/CLI/DebugEval.jet`, over rendered value text. Both
are deleted (I8).

After the change these all work, with the compiler's own diagnostics for a
type error:

- `print xs.len() + total`;
- `print point.norm()`;
- `print items.map(fn(x) { x * 2 }).sum()`;
- `print Int.parse(text) ?? 0`, and a fallback over a call whose callee
  propagates its own callee's failure (§5's journey row);
- a generic call;
- a call through a function value.

An expression with an effect is refused by the compiler's own diagnostic,
which names the impure call and is shown with the expression as the span:
``print log.read_all()`` is refused as an impure call of `log.read_all`,
the direct callee, as `ImpureCallInPureContext` reports it (§3.4).

A local the program has moved is shown as moved wherever it appears: in the
Locals pane, in a place path such as `print xs`, and as a capture. Each says
where it was moved, for example "`xs` was moved into `Box{…}` at
place_moved.jet:9", and none of them shows the stale bytes (§3.7). Today
`jet debug` shows `xs = [1, 2, 3]` in that state (probe `place_moved`).

Proofs (from the audit):

- `Debug/print_evaluates_compiled_expressions`
- `Debug/print_refuses_effectful_expression`
- `Debug/debug_view_renders_without_running_the_target`

Revision 2 added two more:

- `Debug/print_refuses_moved_capture`;
- `Debug/print_calls_through_function_value`.

Revision 3 adds four, one for each defect the second review showed. Each
fixture is one of this revision's probes, and each case is written to fail
on today's `jet debug`:

- `Debug/locals_show_moved_local_as_moved`, from `place_moved`;
- `Debug/outer_frame_after_take_shows_argument_moved`, from `take_arg`;
- `Debug/move_then_share_reads_the_shared_value`, from `move_then_share`,
  which must show `xs = [1, 2]` and refuse nothing;
- `Debug/print_refuses_wait_and_raw_write`, from `row_compose`'s bodies.

Revision 4 adds five, from the third review's findings. The first fails on
today's `jet debug`; each of the others fails on revision 3's design:

- `Debug/outer_frame_in_chain_shows_each_take`, from `chain`: at the stop
  in the first `add`, `run`'s Locals show `xs` moved and `ys` live; at the
  stop in the second, both moved;
- `Debug/print_fallback_over_failure_evaluates`, from `journey`:
  `Int.parse(text) ?? 0` and `add_parsed(a, b) ?? 0`, where `parse_twice`'s
  failure propagates through `add_parsed`;
- `Debug/print_reads_each_generic_module_instance`, from `genmod`: a stop in
  `small`'s `bump` evaluates `HITS` as 8, and a stop in `large`'s as 64. It
  needs the instance members' line rows first (§0.1's first defect);
- `Debug/print_reads_top_level_shared_global`, from `genmod`:
  `tally.guard_read().value.len()` is 2 at a stop in `run`;
- `Debug/print_refuses_session_with_other_build_options`, from `bounds`:
  with the image built with `--bounds=report`, `print peek(xs, 1)` is 2, and
  `jet debug --bounds=fatal` on the same image refuses evaluation, naming
  `bounds`.

## 2. The mechanism in one paragraph

At a stop, the debugger maps each frame's pc to a source position: file,
line and column (§3.1). It then asks the compiler to check the package with
a position probe: when sema's walk of the stopped function reaches that
position, it checks the expression as a lambda `fn() -[]> T` in the
environment the walk holds there. That gives the compiler's own scope,
shadowing, generic instance and ownership state. It inserts no text and
changes no fact of the function (§3.2). Every local the expression captures,
and every local the debugger shows, is first looked up in the image's move
rows. A moved one is shown as moved and never read (§3.7).

The lambda and every Jet function it reaches are built at O0 through the
`jet run` code path into one eval object. Indirect calls go through a
translation to the eval object's own copies (§4.2). The object links with
the toolchain's runtime object, whose stateful entry points are replaced by
eval stubs (§5).

The debugger then injects a `clone` into the stopped thread, making a
single-threaded copy-on-write fork that the target never sees (§6). In the
fork it maps the eval object, prepares the fork (§6.2), writes the captured
locals into the lambda's environment from their DWARF locations, and calls
the lambda. It reads the result with the type-fact renderer that already
shows locals (#6841 c4), then kills the fork. The original process is never
written, apart from the injected syscall's saved and restored registers.

## 3. Checking

### 3.1 The stop position

**Probe.** `Image/Dwarf.jet` writes no `DW_LNS_set_column`. The only
"column" in the file is the CFI return-address column. So a stop is known
only to its line, and a line can hold several scopes (`loop i in xs -> f(i)`,
`-> expr` arms, a lambda).

**Change.** Each `.debug_line` row also carries the column of the IR span
it was emitted for (`DW_LNS_set_column`), and `.debug_jetline` rows carry
it too. A frame whose pc is not a return address is at the row of its pc.

A return address is not looked up in the line rows (third review, P1). A
method call's span starts at its receiver, so in `start.add(xs).add(ys)`
the receiver, the first call and the second call all start at one
(line, column), and the two calls have different move states. Probe `chain`
shows it today: at a stop in the first `add` and at a stop in the second,
`run`'s frame is at `chain.jet:18` both times. Instead, each call
instruction gets a **call-site row** in `.debug_jetline`, keyed by its
return address: the function, and the call's index in the order sema's
final walk of that function finishes walking each call's callee and
arguments. Lowering attaches the index to the IR call as it attaches the
span, and the backend writes it beside the return address. An inlined call
has no return address; its inlined range (c35) carries the same index. The
index needs no source token, so it also covers the calls a statement makes
with none: a loop's per-pass call, an interpolation's builder calls. That
covers every outer frame, and the top frame after `finish` or a step out.

A move row names its local by (name, line, column) of the binding, so two
same-named locals declared on one line are two locals.

**Rule.** For a pc that is not a return address, the probe sits at the
innermost syntax node that starts at the row's (line, column). The names it
sees are the ones sema's walk has in scope when it begins that node:
bindings whose statements ended before it, the enclosing arm's pattern
names, an enclosing lambda's parameters, and the loop variable when the
node is inside the loop's body. A return-address pc uses its call-site row
instead. These two rules cover every case the reviews name:

- **A loop header.** The per-pass code (taking the next item) is the
  header's `in` expression, which is outside the body. So the loop variable
  is not in scope there, and DWARF does not locate it there either. A
  single-line body `-> f(i)` has its own column, so a stop in it sees `i`.
- **A multi-line expression.** The row names the sub-expression the pc
  belongs to. Names the enclosing statement binds are not yet in scope.
- **A one-line arm.** The arm body's column puts the probe inside the arm,
  with its pattern names.
- **An outer frame, or any return-address pc.** The callee is running or
  has just returned, so every argument was already evaluated and passed
  (review 2, N2). The probe therefore sits **after the call's arguments**,
  not at a node's start. The call-site row names the call: the probe sits
  at the point of sema's walk where call number k's callee and arguments
  have been walked and their moves and loans recorded
  (`sema_ownership_note_implicit_take` in `Sema/Calls/Checking.jet`, among
  others), and its result is not yet used. The statement containing the
  call has not finished, so the names it binds are not in scope. Probe
  `take_arg` shows why: a stop in `consume`, then `finish`, shows `run`'s
  `xs = [1, 2, 3]` while the callee owns the block, has pushed 9 into it,
  and returns 4. Probe `take_arg_later` shows sema recording that move at
  the argument, `take_arg_later.jet:13:16`. After the arguments, `xs` is
  moved, and §3.7 shows it as moved. In `chain`, the first call's probe
  sees `xs` moved and `ys` live, the second's sees both moved. Today's
  Locals pane shows `xs = [1, 2]` at the first stop and `xs = [3, 4]` at
  the second, a list the program never built, read through a slot whose
  list was taken (`chain_later` shows sema's take of `xs` at 18:20).

If no node starts at the row's position, which only happens for generated
code, the probe sits at the start of the innermost statement that contains
the position.

### 3.2 The position probe (no overlay)

**Why not an overlay.** The review's probe `overlay_move.jet` shows the
failure. In the program, `box := Box{items: xs}` is `xs`'s last use, so the
image moves `xs` into the box. A closure inserted after it adds a later use,
sema turns the move into a copy, and the overlay program prints 3 where the
image holds 4 items. Any inserted text is a later use, so no overlay can be
the image's program.

**The probe.** The compiler check gets one more input:

- a position, as in §3.1;
- the expressions to check there.

When the body walk of the function containing the position reaches the
probe's point (§3.1), sema checks each expression as the body of a lambda
against the expected type `fn() -[]> T`, with `T` a fresh inference
variable, under the denials of §3.4. This is the same check sema runs for a
lambda argument to a generic parameter.
Probe `infer2` shows the result is inferred (`4`, `3 items`). Probe `infer3`
shows the parameter's bound applies to an unannotated lambda (E-code
`CallbackEffectBoundExceeded`, "This callback uses the effect `IO`, which
the parameter doesn't allow"). A local `fn() -[]> { … }` binding does not
infer (probe `infer`: `ValueThrownAway`, "`a` has no return type"), which is
why the probe uses the lambda form.

The walk then continues as if the probe were not there:

- **No effect on moves.** A probe read never enters move inference. It
  records no `SemaOwnershipShareSite`, so it never turns a move into a
  share, and it opens no loan that outlives the probe. The function's facts,
  and so the image built from them, are the build's.
- **Only the final walk answers.** sema may walk a function more than once:
  "An unmarked move site that a later use reached. The function check
  re-runs with the site known, and the site then checks as the authored copy
  `name.copy()`" (`SemaOwnershipShareSite`, `Sema/Ownership/Checks.jet`).
  A move seen in an earlier walk can be a share in the last. So the probe's
  results, its refusals included, are taken from the last walk, the one
  whose facts the build used. Earlier walks skip the probe. Probe
  `move_then_share` is the review's case: `box := Box{items: xs}`, then a
  stop at line 10, then a later `xs.len()`. The built program copied: at
  the stop, `jet debug` shows `xs = [1, 2]` and `box = Box{items: [1, 2]}`,
  and the run prints 2, then 3. The final walk holds `xs` as live, so the
  probe reads it and is right. An earlier walk would have refused it
  falsely.
- **Moved captures are refused,** with the move rows of §3.7. Those rows
  are this same final walk's moved-place state, written into the image at
  build. The probe's own state at its point must equal the rows for that
  position, and plan step 2 tests that equality. A place moved on only some
  paths to the point is refused as "may have been moved at …", as sema
  refuses a later use there.
- **Captures are sema's.** The lambda's captures are sema's own capture
  list, with their sema types. The debugger never translates DWARF type
  names back into Jet types.
- **Inlined frames.** In an inlined frame (c35) the position is in the
  inlined function, so the probe is in that function's walk.

**One check per function, not per line.** The probe input may name every
position in a function's line table. sema then records, for each
expression, the check result at each position (its captures, its type and
any refusal) in one walk. The debugger asks once for every function on the
stopped stack and every expression it holds: the console's, the watches,
the conditions and logpoints that apply in those functions. A step to
another line of a checked function is a table lookup (§8).

### 3.3 Generic frames

A stop in an instance of `f[T]` probes the walk of `f`'s instance with the
instance's type arguments. These must come from the instance's subprogram
DIE, as `DW_TAG_template_type_parameter`, named as sema names the type.

**Probe** (`elfsyms.py` on the probe `jet` image):

- 43454 function symbols;
- no symbol name carries type arguments (none holds `[` or `<`);
- 79 names are shared by more than one function (for example
  `BinderDescriptor.debug_name` 14 times, `Parse.finish` 11 times).

So neither the symbol table nor a symbol name identifies an instance today.
The review confirmed that `.debug_info` has no template parameters. This is
the plan's first step, because §4.2 needs it too.

Until it lands, a stop in a generic instance refuses evaluation with "the
frame's type arguments are not in the debug info". The first step's case
also reports what share of `jet perf` samples of the compiler's own check
land in generic instances. That is the figure the review asks for: how often
the refusal would hit.

### 3.4 The effect row: the empty ceiling, and `!Unsafe`

**The ceiling.** The lambda's row is `-[]>`, the empty upper bound. The
spec says "any effect in the body is rejected as a purity violation" and
"ambient effects such as `IO` still violate that empty ceiling". `-[]>`
admits allocation, heap copies and panic facts. It forbids writing module
state, directly or in any callee (E3401).

**The deny-only rights** (review 2, N3). The spec lists `Time.Wait`,
`Unsafe`, `Panic` and `Mem` as deny-only. An empty positive ceiling does
not deny them, so `-[]>` alone admits a channel wait, an atomic store and
an `#Unsafe` raw write; the review's `wait_row` showed all three. Each is
answered below, with the probe that was run:

- **`!Unsafe` is in the probe's row.** The probe lambda is checked as if it
  stood in an enclosing rung that writes `#FX(!Unsafe)`. That is the
  policy record sema already composes from a module's `#FX` rung with the
  rows of the functions inside it, so the probe needs no source spelling.
  - Probe `row_compose` puts the review's bodies in `-[]>` functions inside
    a `#FX(!Time.Wait, !Unsafe)` module. The raw write is refused: "reaches
    `#Unsafe` code, but module `probe` denies Unsafe". `print` stays
    refused (`ImpureCallInPureContext`).
  - Probe `unsafe_callee`: the same refusal holds when the raw write sits in
    a package function the rung calls, outside the rung.
  - Probe `unsafe_reach` checks that `!Unsafe` does not refuse ordinary
    expressions through Core's own `#Unsafe` internals. Under `#FX(!Unsafe)`
    over `-[]>`, these all pass and run, printing `20`, `3 items, first 3`
    and `true`: a copied list pushed to, `map` and `sum`, string
    interpolation with indexing, `contains`, a copied list sorted, a
    dictionary `get` and `split`. So Core's `#Unsafe` internals do not count
    against the denial, while a package's own `#Unsafe` code does. An
    expression that reaches a package's `#Unsafe` code is refused (§10).
- **`Time.Wait` is not in the row,** because a `Shared` read carries it.
  Probe `shared_denied`: under `#FX(!Time.Wait, !Unsafe)`, `cell.hits` on a
  `Shared<Stats>` is refused ("reaches the `Time.Wait` effect, which module
  `both` denies"). Under `#FX(!Unsafe)` alone it passes. Denying
  `Time.Wait` would lose the `Shared` reads that G5 closed.
- **Every wait is refused at the eval link instead.** A wait is a runtime
  entry point. Probe `wait_link` lists the runtime calls of each body:
  - `rx.receive()` reaches `jet_rt_native_receiver_receive`;
  - `tx.send(7)` reaches `jet_rt_native_sender_send`;
  - `cell.hits` reaches `jet_rt_native_shared_read`.

  The eval link (§5) is an allow-list: a runtime function links only if it
  has an eval stub or is proven to touch no runtime state. The channel entry
  points are neither. So the link refuses the expression before anything
  runs, naming the entry point and the source line that reached it: "the
  expression waits on a channel (`jet_rt_native_receiver_receive`, reached
  at `main.jet:7`); the debugger never waits on the stopped program". The
  `Shared` read links to its lock stub, which never waits (§5). The review
  agrees that this link layer catches a channel wait (its C12).
- **Atomic stores pass the row,** since no right covers them. Probe
  `row_compose`: `flag.store(true)` on an `Atomic<Bool>` passes. In the fork
  the store lands in the fork's copy-on-write page, or faults on a
  `MAP_SHARED` page made read-only (§6.2). That is contained: the target
  never sees it. R7's in-program placement refuses it (§8.1).
- **`Panic` and `Mem.Alloc` are admitted.** A panic ends the evaluation
  through the panic stub, and allocation goes to the eval arena (§5).

Probes `pure` and `roots` put each positive root in a `-[]>` function, and
`row_compose` and `wait_link` add the bodies above:

| Body | Result |
|---|---|
| `print("side effect")` | `ImpureCallInPureContext`: "calls the impure function `print`" |
| `time.sleep(1ms)` | `NondeterminismInPureEval`: "`time.sleep` is non-deterministic and cannot appear in a pure evaluation" |
| `files.read(…)`, `files.write(…)` | `ImpureCallInPureContext`, naming `files.read` and `files.write` |
| `env.get("HOME")` (`core.sys`) | `ImpureCallInPureContext`, naming `sys.get` |
| `#Impure("…") { print(…) }` inside the function | refused, naming `print` |
| a callee that hides `print` in `#Impure` | refused, naming the callee `helper_impure` |
| `#Unsafe("…") { mem.volatile_write(p, 42) }`, a raw write | refused by `!Unsafe`: "reaches `#Unsafe` code, but module `probe` denies Unsafe" |
| `count = total` on a module `:=` global | `ImmutableAssignment`: "`count` is module state, so a function can't change it directly" |
| `rx.receive()`, `tx.send(7)` | pass the row; refused at the eval link, naming the channel entry point |
| `flag.store(true)` | passes; contained in the fork; never placed by R7 |
| `count + total`, a read of that global | passes |
| a list built, pushed to and measured | passes: allocation is admitted |
| `cell.hits` on a `Shared<Stats>` parameter | passes the row and links to the lock stub |

Revision 2's row "`#Unsafe` blocks refused" is dropped. Its probe refused
only the impure call inside the block, not the block itself.

So the row refuses every positive root and the package's `#Unsafe` code,
and the link refuses every wait: by name for the channel, timer, spawn and
thread entry points, and by §5's third check, no blocking syscall, for any
other runtime path that waits. Each refusal is sema's or the link's
ordinary diagnostic. The debugger shows it with the expression as the span,
so it names the call and the effect.

Two rows need a runtime answer, not a static one:

- **Allocation.** It passes the row. The eval stubs give it an arena of its
  own (§5), so the target heap's metadata is never touched.
- **`Shared` reads.** They pass the row (G5). The cell's lock is a runtime
  entry point, and its eval stub decides (§5). A free lock is taken in the
  fork: no other thread exists there and the target's word is not written.
  A held lock refuses the read with "`cell` was locked by another thread at
  the stop; its value may be mid-update". lldb and gdb would read the
  possibly torn value without saying so.

The seccomp filter (§6.2) stays as a second wall, not the first.

**Compiler defects the probes found.** Both are off this design's path,
since the probe composes its row internally, and each needs its own card:

- The spec allows denials in a body's rungs: "as an `#FX: (...)` line in a
  body, or around a block as `#FX(...) { ... }`. Inside the list, `!E`
  denies" (spec.md, "Effects"). The compiler accepts neither form with a
  denial:
  - probe `fx_forms`: `#FX(IO) { … }` passes, but `#FX(!Time.Wait) { … }`
    gives `MarkerArgumentMismatch`, "`#FX` arguments do not match
    `FX(Effect...)`";
  - probe `fx_line`: `#FX: (!Time.Wait, !Unsafe)` in a `-[]>` function's
    body gives `InvalidScopedPolicy`, "`#FX` cannot attach to a function".
    In a function with no written row (`fx_forms`' `denial_line`), the same
    line draws no diagnostic, so whether it denies anything is unknown.
- The `ProhibitedEffectReached` message misprints the row. For
  `-[!Time.Wait, !Unsafe]>` it says "which it prohibits with
  `-[!Time.Wait, Unsafe]>`" (probe `row_forms3`). The same probe shows that
  a denial-only arrow row sets no ceiling: the wait is refused, but `print`
  passes.

### 3.5 Stale source: a digest of every build input

**The gap** (review 2, N5). Revision 2 put `DW_LNCT_MD5` on the line table's
file entries. But the line table names only files that emitted a function
(`ir_x64_tables_add`, `JetIR/Source/X64Module.jet`). Probe `files.py`
builds a two-file program: `main.jet` uses `Point` from `shapes.jet`, a file
that declares only that struct. `.debug_line` and `.debug_jetline` name
`main.jet` and nothing else. `shapes.jet` appears only inside the type's
identity string in `.debug_info`. If `Point` gained a field after the build,
no digest would catch it, and the eval code would read the old layout with
the new offsets. The probe check also reads every file of the package and
its dependencies, not only the stopped one.

**The digest list already exists.** `jet build`'s store key,
`jet_cli_build_cache_key` (`JetCLI/Source/CLI/BuildCache.jet:190-227`),
hashes the inputs that decide the image. Revision 3 said "exactly", and
copied only the file rows; the third review (P6) showed the key hashes
more. All of it:

- `build_options_key(options)` (`JetFoundation/Source/Registry/BuildOptions.jet:706-719`):
  the profile, each resolved option row that shapes the image, each
  `--set` setting value, `observe`, `log` and the deep checks;
- the host's arch and OS;
- the compiler identity, the runtime key and the Core rows;
- the path and SHA-256 of every file of every root in the authorized source
  snapshot, foreign cache files included;
- each script dependency's name and content hash;
- the path and SHA-256 of every comptime input the compile read
  (`embed_file` assets);
- the hot-text list.

The options matter as much as the files. Probe `bounds` builds one source
twice. The stock image stops at `xs[5]` with E3010. The `--bounds=report`
image, a row with `removes_check`, reports the failure and reads `1` past
the list. The two images differ, and an eval object built with the other
options would stop where the image goes on, or the reverse. A global
computed at build from a `--set` value has other bytes under another value,
and §4.3 compiles immutable globals in.

**Change.** That row list is computed once, by one function that the store
key hashes and the linker writes into the image. It goes into a source
table in `.debug_jetline`, Jet's own section, which `jet debug` and
`jet perf` already read:

- the options key, and each resolved option row as `(name, value)`;
- the host's arch and OS;
- one `(path, sha256)` row per snapshot file and comptime input;
- one `(name, content hash)` row per script dependency;
- the hot-text list.

The compiler, runtime and Core rows are covered by the toolchain digest
(§4.4). The table lists rows, not one combined hash, so a refusal can name
the file or the option.

**The eval object is built with the image's options** (§4.1), read from
the table, with one change: the optimization level is O0. That one cannot
change a value: I9 requires every program to behave the same at O0, O1 and
O2, and the parity suites test it. Every other row is the image's, so a
removed check stays removed and a setting keeps its value.

Before the first probe check of a session, the debugger rebuilds the same
rows from the current snapshot and compares them with the image's. It
re-hashes only files whose stat identity changed. Any difference refuses
evaluation and names the first row that differs: "shapes.jet changed since
the program was built; rebuild to evaluate here". The same applies to a
file added to or removed from the snapshot, and to a script dependency. An
option the session names that differs from the image's is refused too,
naming it: "the program was built with `bounds: report`; this session
asks for `bounds: fatal`". An image with no source table is refused in the
same way, naming the missing table. The digest is a staleness check, not
an integrity claim.

`DW_LNCT_MD5` is dropped, since it is not the staleness proof. Whether gdb
and lldb check it against the file on disk is not claimed here.

### 3.6 O1 and O2 locals

The piece claim is removed: the backend writes no `DW_OP_piece` (review,
C6). An unlocated capture is refused before the build: "`x` has no value at
this point of the -O2 build; at -O0 it does". A callee-saved register in an
outer frame is recovered with CFI (c38). A caller-saved register in an outer
frame is not recoverable, so that local counts as unlocated.

A located O1 or O2 capture may still hold a value from another source
point, because the backend writes imprecise ranges and no
`DW_OP_entry_value`. Nothing today proves otherwise. So until the parity
test below passes, evaluation in an O1 or O2 image refuses every capture:
"evaluating in an -O2 build waits for the location parity proof; `jet debug`
builds at -O0".

**The parity test** (G8, and the third review's P2):

1. For each program of the `Tests/Cases/Debug` fixture corpus, the test
   builds O0 and O2 images.
2. It steps through every line-table row the two images share (file, line,
   column), and stops at every call-site row they share (§3.1).
3. At each stop it compares the rendered value of every local that both
   images locate.
4. At each stop it also compares, for every local of the frame, the moved
   or live state each image's move rows give (§3.7). A local that neither
   image locates after its move is compared here, which step 3 skips.
5. Any difference is a backend location or move-row defect, reported with
   the local, the row and both values or states.

Once it passes, a located capture is read at every level, and O1 and O2
move rows are trusted as O0's are. The case runs in the debug suite, so a
later backend change that breaks a location or a range fails it.

### 3.7 Moved places: the image's move rows

**The defect** (review 2, N1). Probe `place_moved`:

1. `xs := [Int]{1, 2}` is built, `xs.push(3)`, then `box :=
   Box{items: xs}` moves `xs` into `box` at line 9, and
   `box.items.push(4)`.
2. At a break on line 11, today's `jet debug` shows `xs = [1, 2, 3]` in the
   Locals pane and for `print xs`. Meanwhile `box = Box{items: [1, 2, 3,
   4]}`, and the program prints 4.
3. The block that `xs`'s slot points to belongs to `box`. A 3-item `xs`
   exists nowhere in the program. (The reviews' runs name the type
   `module:14:source-root:Box`, the third review's with the gate-p293 seed
   compiler. The probe compiler used here shows `Box`. The type's name
   depends on the compiler build, not on this design.)

Probe `take_arg` gives the same stale read in a caller: `run`'s
`xs = [1, 2, 3]` after `consume` took it and pushed 9 (§3.1). Probe `chain`
gives a worse one: `xs = [3, 4]`, a list that never existed (§3.1).

**The fact exists.** sema's final walk holds the moved places at each
point, `SemaOwnershipMovedPlace`, with a `SemaOwnershipMoveOrigin` that has
the span and the consumer (`Sema/Ownership/Checks.jet`). Its text is what
the user sees today for a use after a move:

- probe `moved_read`: `UseAfterMove`, "`xs` was consumed by `xs.move()`, so
  it can't be used here", with the origin at `moved_read.jet:9:23`;
- probe `take_arg_later`: `MoveWithoutMarker`, "`consume` takes `xs`, but
  `xs` is used again later", with the origin at the argument,
  `take_arg_later.jet:13:16`.

**Change: move rows.** At build, the backend writes the final walk's moved
places into `.debug_jetline` as move rows, one per (function, local or
place path, pc range):

- the local, identified by its binding's name, line and column (its
  `DW_TAG_variable` carries `DW_AT_decl_line` and `DW_AT_decl_column`), or
  a place path under it (`p.items`);
- the pc range where the place is moved. It starts at the first
  instruction of the moving expression's code, which for an argument to a
  take parameter is the call instruction. So an outer frame's return
  address, which lies past the call, is inside it;
- whether the move is certain or happened on only some paths;
- the origin's file, line and column, and the consumer's text.

The same range also ends the local's DWARF location list, so gdb and lldb
show the local as unavailable there instead of showing stale bytes. The
row is what `jet debug` reads, because it carries the reason.

**Every read goes through the rows.** Before a local is read, the debugger
looks up (function, local, pc) in the rows. That covers the Locals pane, a
place path, a capture for the probe lambda and a `debug_view`'s receiver.

- A moved place shows as "`xs`: moved into `Box{…}` at
  place_moved.jet:9:23" and is never read.
- A place moved on only some paths shows as "may have been moved at …".
- A parent whose field was moved renders that field as moved and the rest
  as usual.
- A place path into a moved place is refused with the row's text.

The lookup is a table read, not a compile, so a place path keeps the
renderer's latency (§8.1). The rows come from the walk the build used, so
a share that a later use made (`move_then_share`) produces no row. The
probe check's own state agrees with the rows by construction, and plan
step 2 tests that agreement.

**Levels.** The rows come from sema, which is the same at every level. Only
the pc ranges are per image, mapped through the same IR spans as the line
rows. At O1 and O2 a span need not be one pc range: inlining puts it at
several, scheduling moves the moving expression's first instruction, and
loop rotation splits a range (third review, P2). Probe `take_arg` built at
O2 shows the first: `consume` is inlined into `run`, `bt` has one frame,
`xs` is `<Unavailable>`, and no return address exists for the take. So
until the parity test's step 4 passes, an O1 or O2 image shows a moved
state only where the row's pc range maps one-to-one onto the O0 image's
range for the same span. Elsewhere the local shows as "may have been moved
at …", with the row's origin, and is never read. After it passes, a moved
local reads as moved at O0, O1 and O2 (I9).

## 4. Building: the `jet run` path, at O0

### 4.1 The item set

The probe lambda compiles through the same driver path as `jet run -O0`:

- sema;
- `ir_build_program`;
- the same x86-64 selection;
- the object writer.

It builds with the image's build options, read from the source table, at
O0 (§3.5). Only one other thing differs: the item set is the lambda plus
every Jet function it transitively calls directly, all compiled from
source. That includes `debug_view` methods, generic instances the image
never made, and functions that O2 inlined away.

The lambda's environment is the compiler's own closure-capture layout. The
debugger fills it field by field from the DWARF locations. A heap value is
filled as the pointer the frame holds, with no retain (§5's release stub
never frees). So the calling convention is the compiler's, not one the
debugger invents.

### 4.2 Indirect calls: no image code runs

**The gap** (review, C7). JetIR has `CallIndirect` and `FunctionAddress`. A
function value, a closure or a trait value read from target memory holds an
image code address (a trait value's comes from its vtable). Calling it
jumps into image code that is linked against the image's allocator, locks
and globals.

**Change.** An eval build lowers every `CallIndirect` to a call through
`jet_eval_call_target(address) -> address`, an eval-only runtime entry. The
image's own code is not changed: the lowering is an option of the eval
build only. The translation works like this:

- An address inside the eval object is returned as it is: a lambda the
  expression itself wrote.
- An image address is looked up in a table the debugger fills. On a miss,
  the stub executes `int3`. The fork stops with `SIGTRAP`, and the debugger
  takes the address from the stub's argument register. It finds the
  subprogram DIE whose `DW_AT_low_pc` equals the address (a function's
  entry; any other address is not a call target and is refused). From its
  name, `DW_AT_decl_file`, `DW_AT_decl_line` and, for an instance, its
  template parameters (§3.3), it identifies the function. It builds that
  function from source into a second eval object, mapped in the next free
  range of the fork, adds the pair to the table and resumes the fork. The
  fork is still the same fork, so the evaluation continues with its state.
- An address with no subprogram DIE (C or assembly code, a runtime entry),
  or a generic instance before §3.3 lands, is refused: "the expression calls
  code at 0x… that the debug info doesn't name (`<symbol>` if the symbol
  table has one)".

A closure's environment pointer is passed unchanged: the eval copy of the
lambda body has the image's closure layout, because the toolchain digests
match (§4.4). Names are not enough to identify a function: the probe shows
79 names shared by several functions. The DIE's declaration site is enough.

**The runtime's own indirect calls** (review 2, N4). The lowering covers
only code the eval build compiles, and revision 2 linked the prebuilt
runtime object as it was. But the runtime also calls code addresses that it
loads from heap objects. A lazy iterator node holds its closure's code
address (`Iter.jet`), and so do the collection callbacks and the `Shared`
and task helpers. An iterator the program built and the expression pulls
(`it.count()`) would then jump to image code. Probe `rtindirect` finds 61
runtime functions that make such calls, through `RawWords.jet`'s
`coll_call*` wrappers: in `Iter.jet`, `Shared.jet`, `Sync.jet`, `Task.jet`,
`Scheduler.jet` and more. Through runtime calls, 2175 of the 3253 exported
entry points reach one, `jet_rt_native_shared_read` among them.

All of them reach one choke point. Probe `asmcalls` lists every instruction
in the backend's OS assembly (`JetBackend/Source/OS/Linux.jet`) that calls
a register. There are 20 such asm functions, and the runtime calls 13 of
them:

- `os_call0_unit`, `os_call1_i64` through `os_call5_i64`, and
  `os_call1_unit_i64` through `os_call3_unit_i64`, from `RawWords.jet`'s
  `coll_call*` wrappers;
- `os_call_address_i64`, from `Alloc.jet`, `Fault.jet`, `RawWords.jet` and
  `Thread.jet`;
- `os_call_address_two_i64`, from `RawWords.jet`, `Sync.jet` and
  `Thread.jet`;
- `os_call_address_four_i64`, from `Thread.jet`;
- `os_clone_i64`, from `HostSpawn.jet` and `Thread.jet`.

The other 7 (`os_call_address`, `os_call_address_two`, `os_call_address_three`
and the four `os_call_address_*_unit` forms) have no runtime caller.

So the eval link does not use the prebuilt runtime object. It uses an eval
build of the runtime, made from the runtime's source with this section's
lowering on:

- The runtime's own `CallIndirect` operations go through
  `jet_eval_call_target`, like the expression's.
- The 12 calling primitives above, all but `os_clone_i64`, have eval
  bodies. Each first passes the loaded address through
  `jet_eval_call_target`, then calls the result. An image address is
  translated or refused exactly as above, so a pulled iterator runs the eval
  copy of its closure.
- `os_clone_i64` makes a thread or a process. It gets no eval body, so the
  link refuses it (§5).

The eval runtime object is built when the runtime pack is and is cached by
the toolchain digest, so an evaluation never compiles it. That is one
mechanism with the expression's lowering, not a second list of runtime
twins.

**Other transfers** (third review, P5). A register call is not the only
jump to an address that assembly does not fix. `os_fiber_switch_i64`
(`Linux.jet:2090`) loads `rsp` from memory and then executes `ret`, which
returns to a fiber's resume address in image code. `os_gs_base_write_i64`
(`Linux.jet:2058`) runs `wrgsbase`, which switches the TCB. Neither has a
`call`, so a register-call scan passes both; today only the fiber code's
carrier state keeps them out. §5's test therefore scans every asm body for
any transfer to an address not fixed at assembly:

- `call` or `jmp` through a register or memory;
- `ret` after a write to `rsp`;
- `wrgsbase`.

Probe `linkchecks` finds 22 such bodies: the 20 register calls above, the
fiber switch and the GS-base write. Each is one of the 12 routed
primitives, or fails the test and is named. The fiber switch and GS-base
rails (`X64/InlineAsm.jet` `X64_FIBER_SWITCH`, `X64_GS_BASE_WRITE`) are
refused by name, with `os_clone_i64`.

### 4.3 Module globals

The spec (D-MODULE-VALUE1, D-MODULE-BUILD1) computes every module global's
initializer while building and stores the result in the program. Only a
global whose starting value holds a runtime handle (a `Shared` cell) is
built on first use. So globals split three ways:

| Global | Eval object | Why |
|---|---|---|
| immutable `name: value`, no runtime handle | compiled in from source | The source table (§3.5), its option rows included, and the toolchain digest (§4.4) match, and the eval object is built with the image's options, so the build computes the same bytes. No read can tell the copies apart, since the value never changes. A generic module instance's constant is compiled in for that instance (below). |
| mutable `name := value` | bound to the image's storage | Its current value is the target's. |
| one holding a `Shared` cell | bound to the image's storage | The cell's identity and contents are the target's. One not yet built in the target is refused: "`cache` is not built yet in the program". |

**Probe** (`elfsyms.py`): the probe `jet` image has 43457 symbols, and 2 of
them are data symbols (`jet_rt_checks_armed`, `jet_rt_process_ending`), as
the review found. So bound globals need a binding the image does not have
today (G10).

**Binding by declaration site** (review 2, N10). Revision 2 named one
`STT_OBJECT` symbol per global `<module>.<name>`, the way function symbols
are named. Those names are not identities: §4.2 already rejects function
names for that reason, since 79 are shared. Probe `globals.py` counts the
top-level `name := value` bindings in `Compiler` and `Core`, keyed by file
stem and name: 166 keys, of which one is bound twice. That one is a false
positive: two `words` script fixtures inside string literals in
`JetSema/Tests/ScriptHelperFixtures.jet`. So no global collides today. But
names that collide for functions can collide for globals, and a collision
would bind the wrong address silently.

So the backend writes one CU-level `DW_TAG_variable` per bound global, with:

- `DW_AT_name`, `DW_AT_decl_file` and `DW_AT_decl_line`, the identity §4.2
  uses for functions;
- for a global of a generic module instance, the instance's identity;
- `DW_AT_location` as `DW_OP_addr` of its storage;
- its type.

This is how gdb and lldb find globals. `Dwarf.jet` today emits
`DW_TAG_variable` only for a function's locals (the abbreviations at
`DWARF_ABBREV_VARIABLE`). The eval object refers to each bound global as
an undefined symbol keyed by its declaration site and, in an instance, the
instance's identity. The link binds it to that DIE's address. A global the
DIEs do not name is refused.

**Generic module instances** (third review, P8). spec.md "Generic modules"
makes each application a module of its own, whose members are its
template's text, so every instance's globals share one declaration site.
Probe `genmod` applies `counter<T>(start: Int)` twice: `small:
counter<Int>(8)` and `large: counter<Int>(64)`. Each instance's
`HITS: start` is its own constant (the IR of `small`'s `bump` adds 1 to
`const 8`), and the run prints 9 and 65, from one declaration site. So a
site alone would bind one instance's read to the other's storage.
D-GENMOD-IDENTITY1=A fixes the identity: an instance is its template's
definition plus its normalized arguments, so two aliases of one
application are one instance with one storage, and `counter<Int>(8)` and
`counter<Int>(64)` are two. The global's DIE and its undefined symbol carry
that instance identity, as §4.2's function identity carries an instance's
template parameters. A compiled-in constant is built for the instance the
stopped frame belongs to. Today's symbols name the alias
(`__jet_small_dbump`, `__jet_large_dbump`), which is not that identity, so
the DIE does not take its identity from the symbol.

Two facts limit what the probe could show. A mutable global in a generic
module's body is refused today (probe `genmod_mutable`: E0003, "Expected a
function, struct, enum, or other item inside a module"), though the spec
allows module-global values there, so the mutable case has no fixture until
the compiler accepts it. And a breakpoint on an instance member never hits
(§0.1). A top-level `tally := Shared{[Int]{1, 2}}` builds and runs
(`genmod`), so §4.3's third row now has a fixture.

Until that lands, an expression that reads a bound global is refused:
"`count` is module state the image has no debug entry for". The revision 1
claim that a lazy global's initializer is `Time.Wait` is removed. No such
fact exists, and the spec has no lazy globals besides the handle case.

### 4.4 Addresses and identity

Jet executables are `ET_EXEC` (`StaticLink.jet`, "a static non-PIE
executable", confirmed by the probe's `e_type 2`), so image addresses are
fixed. If images become PIE, the bias is `AT_ENTRY` from
`/proc/<pid>/auxv` minus `e_entry`.

The eval object is linked at a free range of the fork's address space,
taken from `/proc/<pid>/maps`. The linker is MemoryLink's
`link_bind_image`, the binder comptime uses for its batches. Here the target
image is the image it binds to, and the gap is at that range.

The image's toolchain digest must equal the running `jet`'s, because type
layouts, the closure layout and the runtime's object layout must match.
Otherwise evaluation is refused: "the image was built by jet `<digest>`;
rebuild it, or debug it with that jet". `jet debug` builds the image itself,
so the usual case matches.

## 5. The eval link set: no target allocator, scheduler or runtime state

The eval object links with the eval runtime object, the runtime built from
source with §4.2's lowering, which sits beside the runtime pack's
`jet_runtime_jet.o` (§4.2). The link replaces the runtime's stateful entry
points with eval stubs:

| Entry points | Eval stub |
|---|---|
| allocation, free and the large-block cache (`Alloc.jet`) | A bump arena: one fresh `mmap` in the fork, never freed (the fork dies). |
| retain and release (`Refcount.jet`) | Adjust the count word and never free. A release that would free a block the target allocated does nothing, so the target allocator is never entered. The count word lives in a copy-on-write page of the fork. |
| the `Shared` cell's lock acquire and release (`Shared.jet`; revision 3 named a `SharedAccess.jet`, which does not exist) | Read the lock word. If it is free, take it in the fork's copy; the target's word is not written, and no other thread exists in the fork. If it is held, end the evaluation with the refusal of §3.4, naming the cell's binding or the eval line. |
| the stop poll (`jet_ir_stop_turn`), the task context, the cleanup shield (`jet_rt_cleanup_enter`, `jet_rt_cleanup_leave`) and the stop propagation a failed operation polls (`jet_scheduler_native_propagate`) | Return at once: there is no scheduler in the fork. `jet_rt_cleanup_enter` returns a token that `jet_rt_cleanup_leave` ignores, and the propagation poll returns 0. |
| the journey, the error trace a failure carries (`Journey.jet`, `JourneyTLS.jet`: `jet_journey_reset`, `jet_journey_origin`, `jet_journey_frame_text` and the rest) | A journey of the fork's own, in the arena: reset clears it, and origin and frame calls append to it. The target's TLS slot (`JourneyTLS.jet:3` `journey_tls_key`) is never read, and the eval runtime's own copy of that key, which nothing initialized, is never used. |
| panic and the whole stop report, listed by function, not by file: each function that starts one (`jet_rt_panic_*`, `jet_rt_unreachable`, the stop's publication `rt_task_panic_publish`, the functions that end the process, and each `*_stop` or `*_exit` function of `Trap.jet`, `TrapMessages.jet` and `TrapDefectMessages.jet`), and each function a stop report reaches that nothing else calls. A call after a stop report's start, in its block, never runs in the fork. `trap_word`, `trap_byte`, `trap_store`, `trap_put`, `trap_debug_escaped` and the other helpers success paths call are no stubs and pass the checks themselves (review 4, Q3). | Format the message with the eval runtime's own text code into the result block, and `exit_group`. Revision 3 named only `jet_rt_panic_*`. But every Int clone, text append and allocation has a stop path, which flushes stdout and reaches `trap_defect_exit` (third review, P4), so without this row the link refuses nearly every expression. |
| `jet_eval_call_target` (§4.2) | Look up the table; on a miss, `int3`. |
| the 12 register-calling primitives of §4.2 | Their eval bodies: translate the loaded address through `jet_eval_call_target`, then call it. |
| spawn, channel, timer, I/O and thread entry points, `os_clone_i64` among them | None. If the eval object references one, the link refuses before anything runs, naming the entry point and the source line that reached it. This is where the waits a user writes are refused (§3.4): probe `wait_link` shows `rx.receive()` reaching `jet_rt_native_receiver_receive` and `tx.send` reaching `jet_rt_native_sender_send`. A wait deeper in the runtime fails the third check below. |

Stack growth is not stubbed. A body that can reach itself runs the stack
check (`StackSegment.jet`: `cmp rsp, gs:[STACK_TCB_LIMIT]; jb grow`) and
maps a segment. That is thread-local, its `mmap` is allowed (§6.2), and the
TCB words it writes are the fork's copy.

The table is a list in JetIR `EvalLink.jet`, which reads the runtime's IR
as the library build makes it. The link is an allow-list. The eval runtime build defines only the
stubs, plus the runtime functions that pass the three checks below; it runs
the checks itself. A reference to any other runtime function is an
undefined symbol, so the link refuses it, naming the function. A test runs
the same checks over every exported entry point the eval link can reach,
and fails when one is neither in the list nor passes. An entry point passes
when it, and every runtime function it calls that is not stubbed, meets
all three checks:

- **No runtime state.** They read and write no runtime module global, no
  state-page word, and no TCB word other than the stack segment's. The test
  reads the runtime's IR for the globals each function references, so it
  covers the runtime's module globals (the review counted 145 mutable ones,
  for example `Alloc.jet` `alloc_root`). An eval object therefore never
  reads its own runtime copy's state where the target's would differ (G11).
- **No loaded code address** (review 2, N4; third review, P5). Each
  indirect call in their IR is a `CallIndirect` that §4.2 lowers. Each asm
  body they reach transfers to an address not fixed at assembly only if it
  is one of §4.2's 12 primitives: the test scans every asm body in the
  backend's OS sources for `call` or `jmp` through a register or memory,
  `ret` after a write to `rsp`, and `wrgsbase`, as probe `linkchecks`
  does. Any other such body, in a body the runtime calls, fails the test
  and names the body.
- **No blocking syscall** (third review, P3). Nothing they reach, in IR or
  asm, issues `futex` with a `FUTEX_WAIT*` operation or without
  `FUTEX_PRIVATE_FLAG`, or any syscall outside §6.2's allow-list. The check
  is over syscall sites (review 4, Q2): a syscall's number, and a futex
  call's operation, must be a constant at the site, or a parameter whose
  every caller passes a constant, followed through wrappers
  (`os_syscall_i64`, `os_syscall_stack4_i64`, `host_call`,
  `net_socket_call`, `trap_syscall` and the rest). Any other number, one
  loaded or computed or one an export's caller passes, fails. The
  assembler records each `syscall` a template makes with its number and
  second argument (JetBackend InlineAsm.jet), and JetIR EvalLink.jet
  writes the verdicts as the runtime's `runtime.eval` receipt. The test
  names the wait and the path to it.
  The first two checks say nothing about waiting, and probe `linkchecks`
  shows they are not enough: `alloc_lock` (`Alloc.jet:833`) and
  `sync_futex_wait_shared` (`Sync.jet:102`) already pass both and issue a
  `FUTEX_WAIT`. Only the allocator stub keeps `alloc_lock` out, and only
  thread join's refusal keeps the other out. `sync_futex_wait` fails the
  first check only through its timed branch's TCB scratch word; with that
  word moved, 9 runtime functions would pass both checks and wait,
  `sync_thread_lock` among them. No exported entry point that passes the
  first two checks reaches a futex wait today, but two reach a syscall
  outside the allow-list: `jet_fd_created_raw` (`statx`, through
  `os_statx_created_i64`) and `jet_term_write_stdout_line_direct`
  (`writev`, through `os_write_line_i64`). Without this check they would
  link, and the filter of §6.2 would end the fork.

The test also marks a fourth property per entry point, which the link does
not need and R7 does (§8.1):

- **Writes no memory** (third review, P7). No `store` except into blocks
  the function allocated itself, and no asm body that stores. Probe
  `linkchecks` finds that of the 834 exported entry points that pass the
  three checks, 63 reach a store. `jet_char_len`, which revision 3's
  `r7scan.py` assumed was a reader, is one: a counted text's scalar count is
  cached in the text's own word at `+24` (`Text.jet:866`,
  `rt_text_cache_store`). `jet_rt_atomic_bool_load` writes nothing.

Probe `linkchecks` runs the three checks with both stub tables over the
entry points that probe `journey`'s peeks reach: `Int.parse(text) ?? 0`,
`xs.get(5) ?? 0`, `ages.get("ann") ?? 0`, `xs.first() ?? 0` and a fallback
over a failure propagated through two callees. With revision 3's table the
link refuses 11 of the 17, `jet_rt_int_clone`, `jet_int_parse` and
`jet_journey_reset` among them. With this revision's table it refuses none.
The probe is a source scan standing in for the IR test, so its counts are
evidence, not the test.

## 6. Running: the fork

### 6.0 Making the fork

All threads are stopped (all-stop, as `jet debug` runs today). The debugger
takes the selected frame's thread and does the following:

1. Saves the thread's ptrace options, then sets `PTRACE_O_TRACECLONE`, so
   the tracer attaches to the new process before it runs a single
   instruction. Step 6 restores the saved options, so the target's own
   clones are reported afterward exactly as before.
2. Saves its registers, including `orig_rax`. A thread stopped inside a
   restartable syscall gets `orig_rax = -1` for the injected call, so the
   kernel does not restart the interrupted syscall in its place. The
   restore puts back the original value and the `-ERESTART*` result. This
   is the technique gdb uses for inferior calls; the review confirmed it.
3. Points `rip` at a `syscall` instruction already in the image (the
   runtime's `os_syscall`), so no byte of the target is written.
4. Makes `clone(CLONE_PARENT, exit signal 0)` and resumes the thread for
   that one instruction. If the thread reports a signal-delivery-stop
   instead of the syscall's completion, the signal was pending on it. The
   debugger records it, resumes with signal 0 (suppressed), and repeats
   until the syscall completes. When the target is next resumed, the
   debugger resumes that thread with the recorded signal, so it is
   delivered as if the evaluation never happened (G12). As to the clone:
   - `CLONE_PARENT` makes the fork the child of the target's parent (the
     tracer helper), not of the target.
   - Exit signal 0 means no `SIGCHLD` reaches anyone.
   - So a `waitpid(-1)` in the program, or its process watcher, never sees
     the fork. The helper reaps it with `waitpid(__WALL)`, because a child
     whose exit signal is not `SIGCHLD` is a clone child.
   - For a process that `jet debug` attaches to (D-DBG-VERBS1), the parent
     is someone else, so `CLONE_PARENT` would hand the fork to that parent.
     There the fork is made without `CLONE_PARENT`, still with exit
     signal 0. The target gets no `SIGCHLD`, and the runtime's
     `jet_sys_wait_raw` waits with options 0 (`HostProcess.jet`, confirmed
     by the review), so the program never sees the fork. The killed fork
     stays a zombie of the target until the target exits: one process-table
     entry per evaluation in an attached session. This is a stated limit
     (§10).
5. Restores the thread's registers.
6. Restores the saved ptrace options. The target, resumed later, is in the
   state it was stopped in: the same memory, the same registers, and the
   same pending signals, since step 4 delivers again any it absorbed.

### 6.1 Threads and locks

A fork holds only the calling thread. A lock another thread held at the
stop stays held in the fork forever. This design answers that in three
layers:

1. **No target allocator, scheduler or runtime lock is reachable.** The
   eval link (§5) replaces every runtime entry point that takes a runtime
   lock. That includes:
   - the allocator's arena futex (`Alloc.jet` `alloc_lock`, taken when a
     thread's cache refills or overflows);
   - the large-block cache lock;
   - the page-map lock;
   - the execution-end flush lock;
   - the `Shared` cell lock, which the stub takes only when it is free.

   No image code runs (§4.2), so an image lock is not reachable through a
   function value either. An evaluated expression that allocates never
   touches the target heap's metadata.
2. **No blocking wait links.** The row refuses `time.sleep` (it reads the
   clock). Every other wait is a runtime entry point that the eval link's
   allow-list does not define, so the link refuses it before anything runs
   (§3.4, §5). Revision 2 said `-[]>` refused waits, which was wrong (review
   2, N3). The `Shared` lock stub never waits.
3. **Detection for what is left.** What is left is a runaway loop, or a
   spin on a user atomic. The evaluation has a wall budget of 2 s, a
   constant named in the message. When it runs out, the debugger reads
   `/proc/<fork>/syscall` and the fork's pc, unwinds with the eval object's
   line table, reports "the evaluation ran past its 2 s budget at
   `<line>`", and kills the fork.

### 6.2 Preparing the fork: descriptors, signals, memory

The debugger injects the following into the fork, in this order. Step 0
comes before anything is mapped in the fork, including the eval object;
steps 1-5 come before the call.

0. **Reserve the holes.** For each mapping whose `/proc/<pid>/smaps`
   `VmFlags` holds `dc` (`MADV_DONTFORK`), the fork has a hole: free
   address space where the target has memory. The fork's own `mmap`, or the
   eval object's "free range", could land there (review 2, N9). The
   debugger therefore maps each hole `PROT_NONE` with
   `MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE` at its address.

   Probe `dontfork` (Python, a 1 MiB `dc` range holding `0x5a` bytes):
   - with no reservation, the fork's `mmap` hinted at the range lands in
     it, and both the eval code's own read and the debugger's
     `process_vm_readv` read `0x00` with no fault, a wrong value shown as
     real;
   - with the reservation, the hinted `mmap` lands elsewhere, the eval
     code's read takes `SIGSEGV`, and `process_vm_readv` fails with
     `EFAULT`.

   The review's `sleeper` run found no `dc` or `wf` mapping in a Jet image.
   The case arises with FFI libraries that set them.
1. `rt_sigprocmask`, blocking every asynchronous signal. This comes first,
   because the filter in step 3 does not allow it.
2. `prctl(PR_SET_NO_NEW_PRIVS)`.
3. **A seccomp filter** that allows only `mmap`, `munmap`, `mprotect`,
   `madvise`, `futex`, `rt_sigreturn` and `exit_group`. It first checks
   `AUDIT_ARCH_X86_64`, and it checks the arguments of four of those
   syscalls (review 2, N8):
   - `mmap` only with `MAP_PRIVATE | MAP_ANONYMOUS` among the sharing and
     anonymity flags, and fd -1, so no inherited descriptor can be mapped
     shared;
   - `mprotect` only without `PROT_WRITE`, so step 4's read-only shared
     mappings stay read-only;
   - `madvise` only with `MADV_DONTNEED`, `MADV_FREE` or `MADV_HUGEPAGE`,
     so `MADV_REMOVE` cannot free a shared mapping's backing store;
   - `futex` only with `FUTEX_PRIVATE_FLAG`, so no shared futex can wake
     a target waiter.

   Probe `seccomp_args` (Python, a forked child under the same checks,
   with `SECCOMP_RET_ERRNO` in place of the trap so it can print) shows
   each allowed form passing and each escape refused:
   - allowed: `mprotect PROT_READ`, an anonymous private `mmap`,
     `MADV_DONTNEED`, and `FUTEX_WAKE | FUTEX_PRIVATE_FLAG`;
   - refused with `EPERM`: `mprotect PROT_READ|PROT_WRITE`, a
     `MAP_SHARED` map of an inherited memfd, `MADV_REMOVE`, and a shared
     `FUTEX_WAKE`.

   The fork's own needs fit these forms. The arena and stack segments are
   anonymous private maps (`StackSegment.jet`: `mmap`, then
   `madvise(MADV_HUGEPAGE)`, with no `mprotect`). A second eval object
   (§4.2) is mapped anonymous and private, written by the debugger through
   ptrace, then made read and execute with `mprotect`.

   The filter's default action is `SECCOMP_RET_TRAP`. The `SIGSYS` stops
   the fork in a signal-delivery-stop, where the debugger reads
   `si_syscall` from the siginfo, names the syscall and kills the fork. The
   inherited `SIGSYS` handler, which is image code, never runs.

   The filter builder is `ComptimeWire.jet`'s, which today builds the
   comptime child's filter around its cookie and its refusal handler. It is
   split into the plain allow-list form, with argument checks, that both
   callers use, so there is one builder.
4. `mprotect(PROT_READ)` on each writable `MAP_SHARED` mapping (shared
   memory, a memfd ring), as listed in `/proc/<fork>/maps`. A write would
   carry back to the target; now it faults and is reported. The heap and
   stacks are private, so their copy-on-write pages are the fork's.
5. `mprotect(PROT_NONE)` on each mapping whose `VmFlags` holds `wf`
   (`MADV_WIPEONFORK`), which the fork sees as zeros.

A read of a reserved `dc` hole or a `wf` mapping faults. It is reported as
"the value lives in memory a fork does not copy (`MADV_WIPEONFORK`)", or in
the `MADV_DONTFORK` form, and never shown as zeros. That holds for both
readers:

- **The eval code** takes `SIGSEGV`, reported as below.
- **The renderer** reads fork memory only with `process_vm_readv`
  (`x64_trace_read`, `Image/Loader.jet`), which fails on `PROT_NONE`. It
  never reads fork memory with `PTRACE_PEEKDATA` (`x64_trace_peek`) or
  `/proc/<pid>/mem`, because both read through page protections. In probe
  `dontfork`, with the hole reserved, `process_vm_readv` fails with
  `EFAULT`, while `/proc/<pid>/mem` and `PTRACE_PEEKDATA` both read `0x00`
  with no fault.

The rest of the fork's environment:

- **Descriptors.** The fork shares the target's open file descriptions
  (offsets, `flock` locks, sockets) but never uses them: the filter allows
  no I/O. On exit the fork's descriptor table is dropped. Closing a copy
  releases neither the target's `flock` locks, which the shared description
  holds and the target still holds, nor its `fcntl` record locks, which a
  fork does not inherit (fork(2)). `epoll` registrations belong to the
  description and stay.
- **Pending signals.** A fork starts with an empty pending set (fork(2)).
- **Synchronous faults.** `SIGSEGV`, `SIGBUS`, `SIGFPE` and `SIGILL` stop
  the fork in a signal-delivery-stop before any handler runs. The debugger
  reports "the evaluation read invalid memory at 0x… (`<line>`)" and kills
  the fork, so the runtime's stop report never runs and never writes to
  the shared stderr.
- **Timers.** Neither POSIX timers (the profiler's `SIGPROF`) nor itimers
  are inherited (fork(2)).
- **Strict overcommit.** Under `vm.overcommit_memory=2`, a fork of a large
  target can fail with `ENOMEM`. The clone's result is then refused: "the
  kernel refused a copy of the program (vm.overcommit_memory=2)". There is
  no fallback, since the only fallback would be running in the target. This
  machine runs 0, the default.

### 6.3 The call

The fork's only thread runs on its copy of the stopped thread's stack,
below the red zone (`rsp - 128`), aligned to 16. The stack check and
segment growth work as in the program (§5). Past the thread's stack budget,
growth faults, and the fault is reported as above. A stop inside a
function's prologue, or inside the `grow` trampoline, still has its frame
below the red zone. The environment block and the result block are in the
eval arena.

The debugger:

1. sets `rip` to the lambda's entry and `rdi` to the environment;
2. sets a return address pointing at a `ud2` in the eval object, so the
   return stops the fork;
3. resumes the fork and waits within the budget, serving `int3` stops from
   `jet_eval_call_target` (§4.2) on the way;
4. reads the result block;
5. renders it from the fork's memory with the type facts;
6. sends `SIGKILL`.

## 7. Results and `debug_view` (R8)

The c4 renderer, the one that shows locals, renders the result from the
fork's memory before the kill. A panicking expression shows
`panicked: <message>` with the eval line.

Rendering a value whose type has a `debug_view` calls that view through this
same path: the view is one more Jet function in the eval object, checked
under `-[]>`. A view that calls a stored function value goes through §4.2's
translation, so it too runs only eval code (C15). A view with an effect is
refused when its type is first rendered; the value falls back to its
fields, with the refusal shown once.

### 7.1 Replays and dumps

**A replay** (R1, D-RUN-RECORD1 and D-TIMETRAVEL1) stops in a live process
that the replayer drives under ptrace. At a replay stop, that process is
forked exactly as in §6. This is rr's diversion session: the call runs in a
disposable copy, so the recording is untouched. The filter refuses every
syscall that could diverge from the recording.

**A dump** (R9) has no process. Its host is the image itself:

1. The loader starts the image the dump names (by its build identity),
   traced and stopped at its entry, through the loader's existing ptrace
   start (`Loader.jet`).
2. Through the same injected-syscall step as §6.0, the debugger maps each of
   the dump's writable regions `MAP_PRIVATE | MAP_FIXED` from the dump file
   at its recorded address. A file-backed private mapping is copy-on-write,
   so nothing is copied. `mmap` needs a file offset that is a multiple of
   the page size, so the format stores each region at a page-aligned offset
   (review 2, N7).
3. It sets each thread's registers from the dump with `PTRACE_SETREGS`,
   including the `fs_base` and `gs_base` fields of `user_regs_struct`.
   Revision 2 restored only the `fs` base, but the Jet runtime's TCB is
   behind GS (review 2, N7):
   - the runtime reads its TCB through `os_tls_base_i64` (`Thread.jet`),
     the stack check is `cmp rsp, gs:[STACK_TCB_LIMIT]`
     (`StackSegment.jet`), and a new thread sets it with `ARCH_SET_GS`
     (`Fiber.jet`);
   - probe `tcb.py` attaches to a running Jet image (`sleeper.jet`) and
     reads `fs_base = 0x0`, unset, and `gs_base` inside a 1 MiB anonymous
     `rw-p` mapping.

   `fs_base` is restored too, because an FFI library's thread-local
   storage lives behind it.
4. From there on, the host is a stopped process like a fork: §6.2's
   preparation and §6.3's call run unchanged.

This fixes what R9's format must carry for evaluation: the image identity;
every writable region with its address, its protection and a page-aligned
file offset; and each thread's registers, `fs_base` and `gs_base` among
them. The TCB's own mapping is one of the writable regions. R9's ballot
(D-OBSERVE-FLIGHT1) decides the rest of the format. Until R9 lands,
`jet debug` opens no dumps, so there is nothing to refuse.

## 8. Latency and watches

### 8.1 Costs and caches

The proposal claims no latency figure; the figures come from measurement.
The review measured the inputs:

- lldb answers an interpreted `expr total + 1` within its 10 ms noise
  floor, and a JIT call `expr square(total)` in 6-13 ms (lldb 21.1.8, a
  16-line C program at `-O0`);
- on the Jet side, `jet check` of 9- to 16-line probe files takes 60-233 ms;
- a check of one file of the JetCLI package takes 9.95 s.

Each evaluation has four parts:

| Part | Cost driver | Cache |
|---|---|---|
| a place path (`x`, `p.field.sub`) | Nothing compiles. First the move rows are looked up (§3.7), then the c4 renderer reads the value, the same read the Variables pane uses to expand a value. It is a lookup by path, not a second evaluator: no operators, no calls, no indexing. | none needed |
| probe check and O0 build | One check of the stopped package with the probes (§3.2), covering every position of every function on the stack and every pending expression; plus O0 code for the lambdas' direct call closure. | The result table is keyed by (function instance, source digest, image digest) and the eval object by (expression, capture types, function instance, image digest). The line is not in either key. A watch stepped through a loop body, or through any line of a checked function, is a lookup. |
| fork | The page-table copy, proportional to the target's resident size; copy-on-write faults only for pages the evaluation writes. | none: one fork per stop |
| map, prepare, call, read, kill | About 10 ptrace round trips, plus the expression's own work and any §4.2 build on a miss. | The §4.2 translations are kept with the eval object. |

**Watches.** All watches, display expressions and logpoints that apply at
a stop are batched: into one probe check per new function, and into one
eval object with one entry per expression. They run in one fork per stop,
so a stop with 20 watches costs one fork, not 20.

**The package check.** For a large package the package check dominates the
first evaluation in a function. The spec's incremental service
(architecture, "Incremental Compiler Service") already specifies the fix:
"inside a rechecked package, items reuse their previous results by
red/green over recorded, ordered reads". It is not built in the self-hosted
compiler (a probe search of JetDriver, JetQueries and JetSema finds no item
reuse). With it, a probe check re-walks only the probed functions. That
work belongs to the compiler, not to a second evaluator. §8.2's rows show
whether evaluation waits on it.

**R7 conditions** (G13; review 2, N6). A conditional breakpoint whose
condition is false a million times cannot afford a fork per hit. R7 (ballot
D-DBG-VERBS1) runs such a condition inside the program, placing this
design's compiled lambda there. Revision 2 placed a lambda only if its build
reached no allocation. But a lambda returns its result through an
allocation, so that rule placed nothing, `xs.len() > 2` included.

The rule is now a scan of the placed code's IR: the lambda, and every
lambda and function it calls. A lambda is placed only if:

- its only runtime calls are the result allocation (`jet_rt_alloc_value*`)
  and runtime entry points that §5's test marks "writes no memory";
- it makes no retain or release and no atomic store;
- it stores only into blocks it allocated itself.

Revision 3's scan read a hand-written list of readers in place of the mark.
The third review (P7) pointed out that no check proved the property, and
probe `linkchecks` shows the list was wrong: `jet_char_len` caches the
text's scalar count in the text's own word (`Text.jet:866`). Placed in the
program, that is a plain store into a block the program owns, which
another thread may be reading. With the mark, it is refused.

**One arena per thread.** In the target, the result allocation goes to an
arena the debugger maps when the breakpoint is set. A breakpoint in code
several threads run is hit by them at once, and the others keep running
while a placed lambda runs in one, so one arena reset at each hit would let
one thread's reset reclaim a result another thread is still reading (third
review, P7). So each thread has its own arena. The runtime reserves one TCB
word for the debugger (`TCB_DEBUG_ARENA`). With every thread stopped, the
debugger maps one arena per thread and writes its address into that
thread's word, and the placed code resets and uses the arena that
`gs:[TCB_DEBUG_ARENA]` names. A thread created after the breakpoint was set
has 0 there, so its hit traps to the debugger, which maps its arena and
writes the word before the condition runs. That is one extra mapping per
thread in the program. The target's heap is never entered, and a placed
lambda writes nothing the program reads, so it computes in the target the
same value it computes in the fork. Its panic stub writes the message to
the breakpoint's slot and reports a stop: "the condition panicked:
`<message>`". That is the same text the fork shows.

Probe `r7_scan` applies the rule to six conditions, with `r7scan.py` over
`jet emit --ir`. Its readers are now taken from the mark: of revision 3's
two, only `jet_rt_atomic_bool_load` is marked (`--readers
jet_rt_atomic_bool_load`).

- placeable: `xs.len() > 2`, `xs.contains(2)` and `flag.load()`;
- fork per hit: `stats.hits == 3 && stats.name.len() == 5`
  (`jet_char_len` writes), `xs.map(fn(x) { x * 2 }).sum() > 10` (it
  allocates, releases and frees a list, moves memory and polls the
  scheduler), and a body that calls `flag.store(true)`
  (`jet_rt_atomic_bool_store`).

Revision 3's list placed 4 of the 6. An atomic store passes the row (§3.4)
but never runs in the program: in the fork it writes only the fork's copy.

Any other condition runs in a fork per hit. The debugger says so when the
breakpoint is set ("this condition allocates, so each hit costs a fork of
the program"), and §8.2 measures the cost. So a condition has one meaning,
whichever way it runs.

### 8.2 Against lldb

`Tools/Perf/DebugLatency` gains five rows. Each runs beside lldb on the
same image and stop, with the figures in its receipt:

- `print_place`: `print p.field` against `frame variable p.field`;
- `print_first`: the first evaluation of a new call expression, against
  `expr square(total)`;
- `print_again`: the same expression at a later stop in the same function;
- `watch_per_stop`: 20 `n` steps with 5 watches, against `target stop-hook
  add -o 'expr …'`;
- `condition_per_hit`: 1000 hits of a fork-per-hit condition that is
  false, against lldb's `breakpoint modify -c` with the same expression.

Proposed bars:

- `print_place` within lldb plus 10 ms, the method's noise floor;
- `print_again` and `watch_per_stop` within 2× lldb, since they cost a fork
  and the calls only;
- `print_first` at most 250 ms on the 16-line fixture;
- `condition_per_hit` is reported, not barred, with the target's resident
  size and its huge-page share in the receipt.

Probe `fork_cost` (Python) gives the floor of `condition_per_hit`: one fork,
exit and wait of a process with private anonymous memory touched. These are
the three runs in the committed log, each with 20 forks per size, on a
machine shared with compiler builds:

| Resident | In huge pages | Median | Min |
|---|---|---|---|
| 64 MiB | 64 MiB | 1.59-3.00 ms | 0.65-1.55 ms |
| 512 MiB | 512 MiB | 3.42-4.87 ms | 2.08-2.75 ms |
| 2048 MiB | 1268-2048 MiB | 13.74-400.11 ms | 9.02-112.92 ms |

The figures move with the machine's load and with how much of the memory
the kernel backed with transparent huge pages, which a fork copies as one
entry each instead of 512. At 2 GiB, the run with all of it in huge pages
took 13.74 ms, and the runs with 1268 and 1290 MiB in huge pages took
324.66 and 400.11 ms. So `condition_per_hit`'s receipt carries the
huge-page share and the load.

So a million false hits of a fork-per-hit condition cost about 26 minutes
at 64 MiB on this machine, and hours at 2 GiB. That is why the scan of §8.1
matters, and why the debugger names the cost when the breakpoint is set.

For a package of the JetCLI's size, `print_first` is reported, not barred,
until item reuse lands; then the bar becomes 2× the warm check of one
function. The bars are kept by the same tool's `--bar` (c22, R11).

## 9. I9: one meaning

The expression is compiled by `jet run`'s own pipeline (§4), at the probe's
position in the program's own check (§3.2). So it means what the same text
means at that point of the program. The probe changes no move, and its
refusals come from the final walk, the one the image was built from (G1).
A moved local is read through the move rows, which come from that walk
(§3.7). So the Locals pane, a place path and an expression agree on what
has been moved, at every level.

The image's level (O0, O1 or O2) changes only which captures are read and
which move states are shown: at O1 and O2, no capture and only one-to-one
move ranges until the parity test passes (§3.6, §3.7), and after that the
located captures and every move row. So an expression computes the same
value at every level, or gives a named refusal.

The eval object itself is always built at O0, with every other build
option of the image (§3.5), so a removed check or a `--set` value means in
the expression what it means in the image.

The proof cases run on O0, O1 and O2 images of one fixture. They expect the
same printed values at O0 now, and at all three levels once the parity test
passes; before that they expect the named refusal at O1 and O2.

Assignment (`set x = …`, DAP `setVariable`) is not evaluation: it writes
the target. It stays out of this design and needs its own ballot.

## 10. Limits stated up front

- An effectful call is refused where lldb would run it. That is the price
  of the guarantee (the audit's D4 limit). Visual Studio's `, nse` refuses
  side effects dynamically; this design refuses them statically, before
  anything runs.
- A stop in a frame without Jet source (the runtime, a C library) evaluates
  nothing. The nearest Jet frame is selectable.
- In O1 and O2 images, captures are refused until the parity test passes
  (§3.6), and after that wherever DWARF has no location. Until then a
  moved state is shown only where its pc range maps one-to-one onto O0's;
  elsewhere a local shows as "may have been moved" (§3.7).
- A stop in a generic instance evaluates nothing until template parameters
  land (§3.3).
- A bound global is refused until its global DIE lands (§4.3). A global of
  a generic module instance needs the instance's identity in that DIE.
- A moved local is shown as moved, with its site, and never read (§3.7).
  The Locals pane no longer shows the stale bytes that it shows today.
- Every wait is refused at the eval link, not by the row: by name for the
  entry points a user reaches, and by §5's no-blocking-syscall check
  anywhere deeper. The row cannot deny `Time.Wait`, because a `Shared`
  read carries it (§3.4). An expression that reaches a package's `#Unsafe`
  code is refused, even when the code only reads.
- A session whose build options differ from the image's is refused, naming
  the option (§3.5).
- An R7 condition placed in the program uses one extra mapping per thread
  in the target, its result arena, and one reserved TCB word (§8.1). A
  condition that calls a runtime entry point not marked "writes no memory",
  such as `text.len()` (`jet_char_len`), runs in a fork per hit.
- A `MADV_DONTFORK` range is a reserved hole in the fork. Jet images set no
  such range; FFI libraries can (§6.2).
- Each evaluation costs a fork of the target's page tables. For very large
  targets that is the dominant cost, and §8 measures it.
- A `Shared` cell another thread held at the stop is refused, not read
  (§3.4).
- In an attached session each evaluation leaves one zombie entry in the
  target's process table until the target exits (§6.0).
- A new expression in a function costs one check of its package until the
  compiler's item reuse lands (§8.1).

## 11. Alternatives considered

- **Run in the stopped target, as lldb, gdb, Delve, JDWP and .NET
  func-eval do.** It perturbs the program: it allocates from the target's
  heap, takes its locks and runs its code.
  - JDWP's answer to locks is to resume every thread during the call,
    which perturbs the program further.
  - Delve's `runtime.debugCallV2` lets the runtime refuse an unsafe point,
    but the call still runs with the real allocator and scheduler.

  All of these contradict R8 and c31.
- **lldb's interpret-first hybrid.** `IRInterpreter` runs a call-free
  expression in the debugger over target memory, and JITs into the target
  otherwise. The review argues that an interpreter of the compiler's own IR
  is not a second evaluator. It is still a second back end: every IR
  operation needs a second implementation whose arithmetic, overflow, layout
  and panic behavior match the x86-64 back end's. Without that it breaks
  I9's one meaning as a fourth "level". Jet already removed JetEval's
  interpreter and runs comptime code natively. This design takes the
  hybrid's cheap case another way: a place path is the renderer's own read
  (§8.1), which needs no execution at all. An interpreter is reconsidered
  only if the §8.2 bars are missed once item reuse lands.
- **rr diversion sessions.** These are the closest prior art: rr clones the
  replay state into a scratch session, runs the call there for its side
  effects and discards it. This design adopts the idea for live processes
  (the fork), and §7.1 uses it as it stands for replays. rr restricts a
  diversion's syscalls dynamically; the empty row refuses them before the
  build.
- **gdb `checkpoint`.** It snapshots a stopped inferior by forking it and
  documents that a checkpoint shares file descriptions. It is prior art for
  §6.0. Unlike it, the fork here never resumes the program's own code:
  only eval code runs in it.
- **Copy the captured values into the comptime child and run there.** It
  needs a deep copy of every reachable block across processes, and loses
  identity and sharing. A `debug_view` that compares references or walks a
  graph would then see a different value. It also doubles the marshaling
  code.
- **`vfork` or `CLONE_VM`.** The fork would share memory with the target,
  so its writes (refcounts, arena) would reach the program.
- **An overlay closure inserted into the source** (revision 1). Its capture
  is a later use, so it changes sema's move decisions (§3.2). This revision
  replaces it with the position probe.

## 12. Plan once the review passes

Each step is its own SUB, with its cases. The order follows the
dependencies.

1. **DWARF facts.**
   - Template type parameters on generic instances (§3.3), with a
     `jet debug` case that reads them and the generic-sample share.
   - Line rows for generic module instance members, so a breakpoint on one
     stops (§0.1's first defect; `genmod`).
   - Columns in `.debug_line` and `.debug_jetline`, and call-site rows keyed
     by return address (§3.1). A case stops in each `add` of `chain` and
     expects each frame's call index.
   - A source table in `.debug_jetline` with every row `jet build`'s store
     key hashes: the options key and resolved rows, the host, each build
     input's (path, SHA-256) and each script dependency, from the function
     that the store key hashes (§3.5). One case edits a types-only file and
     expects the refusal naming it; one builds `bounds` with
     `--bounds=report` and expects a session with `--bounds=fatal` to be
     refused naming `bounds`.
   - Move rows in `.debug_jetline`, locals named by (name, line, column),
     with each moved local's location list ended at the move (§3.7).
   - One CU-level `DW_TAG_variable` per bound global, carrying a generic
     module instance's identity (§4.3). Cases: two instances of one module
     global (once the compiler accepts a mutable one, `genmod_mutable`),
     and a top-level `Shared` global (`genmod`).
2. **The position probe in sema** (§3.2), with compiler unit tests:
   - a moved capture is refused with its site (from `overlay_move.jet`);
   - the function's facts are byte-equal with and without probes;
   - inference, and the `-[]>` and `!Unsafe` refusals of §3.4's table;
   - an outer frame's probe placed after its call's arguments (from
     `take_arg`), with the call named by its call-site index (from
     `chain`: `xs` moved and `ys` live in the first call, both moved in the
     second), and refusals taken from the final walk (from
     `move_then_share`);
   - for every function of the compiler unit corpus, the probe's moved
     state at each position equals the image's move rows.
3. **The eval link set.** The eval runtime build with §4.2's lowering and
   the 12 primitives' eval bodies, the runtime stub list with this
   revision's journey, stop-report and task rows, and its test (§5): the
   three checks (no runtime state, no loaded code address by the transfer
   scan, no blocking syscall) and the "writes no memory" mark. The test's
   cases include `journey`'s peeks linking with no refusal, the fiber
   switch and GS-base rails refused by name, and `jet_char_len` marked as
   writing. Then the `CallIndirect` translation (§4.2) and the eval build
   through the `jet run` path with the image's options (§4.1). Unit tests
   run on a fixture image; one pulls an iterator the program built.
4. **The fork host and R7's arenas.** Clone injection with signal and option restore,
   preparation in the §6.2 order, the call, the read and the kill (§6).
   That includes the `dc` reservation and the filter's argument checks.
   Cases cover the budget message, a fault, a refused syscall, each refused
   argument form, a shared mapping, a `wf` mapping, a held `Shared` cell
   and a call through a function value. R7's per-thread arenas through the
   reserved TCB word (§8.1), with a case where two threads hit one placed
   condition at once and each reads its own result.
5. **Cutover.** Console `print` and `watch`, conditions, logpoints and DAP
   `evaluate` move onto it, with the place path (§8.1). `DebugEval.jet` and
   `native_debug_print` are deleted. Add §1's proof cases except
   `debug_view`'s (step 6), the
   DebugLatency rows and the spec text in `Docs/Spec`.
6. **`debug_view`** through the same path (R8), with
   `Debug/debug_view_renders_without_running_the_target`.
7. **The O2 location and move-row parity test** (§3.6), which lifts the O1
   and O2 refusal and the one-to-one move-range limit when it passes.
8. **Replays** (§7.1) with R1, and **dumps** with R9.

## Appendix: probes

### Revision 2 probes

All probes ran with the lane's probe `jet` compiler on 2026-11-18, one at a
time at `nice 19`, with `NO_COLOR=1`. Sources are given whole so they can be
rerun. The review's probes (`row_io.jet`, `overlay_move.jet`,
`shared_read.jet`) are in its own appendix.

`pure.jet`. Expect: `print`, `time.sleep` and the module write refused; the
module read, the allocation and the `Shared` read pass.

```jet
// Probe G4: does the empty row `-[]>` refuse every positive root?
use time: core.time

Stats: struct {
    hits: Int,
}

count := 1

peek_print: fn(total: Int) -[]> Int {
    print("side effect")
    total + 1
}

peek_sleep: fn(total: Int) -[]> Int {
    time.sleep(1ms)
    total + 1
}

peek_shared: fn(cell: Shared<Stats>) -[]> Int { cell.hits }

peek_write: fn(total: Int) -[]> Int {
    count = total
    total
}

peek_read: fn(total: Int) -[]> Int { count + total }

peek_alloc: fn(total: Int) -[]> Int {
    items := [Int]{total}
    items.push(1)
    items.len()
}

run: fn() {
    cell: Shared{Stats{hits: 1}}
    print(peek_print(1) + peek_sleep(1) + peek_shared(cell) + peek_write(1) + peek_read(1) + peek_alloc(1))
}
```

`jet check` reports exactly three errors:

- `ImpureCallInPureContext` at 11:5, "`peek_print` calls the impure
  function `print`";
- `NondeterminismInPureEval` at 16:10, "`time.sleep` is non-deterministic
  and cannot appear in a pure evaluation";
- `ImmutableAssignment` at 23:5, "`count` is module state, so a function
  can't change it directly".

`roots.jet`:

```jet
// Probe G4: the empty row against the other positive roots and the escapes.
use files: core.files
use env: core.sys
use process: core.process

peek_read: fn() -[]> Int { (files.read("main.jet") ?? "").len() }

peek_write: fn() -[]> Int {
    files.write("eval-wrote.txt", "x") ?? 0
    1
}

peek_env: fn() -[]> Int { (env.get("HOME") ?? "").len() }

peek_impure: fn() -[]> Int {
    #Impure("probe: does an escape pass the empty row") { print("impure ran") }
    1
}

helper_impure: fn() -> Int {
    #Impure("probe: an escape in a callee") { print("callee impure ran") }
    2
}

peek_callee: fn() -[]> Int { helper_impure() }

peek_unsafe: fn() -[]> Int {
    #Unsafe("probe: does an unsafe block pass the empty row") { process.exit_unmapped(101) }
    1
}

run: fn() {
    print(peek_read() + peek_write() + peek_env() + peek_impure() + peek_callee() + peek_unsafe())
}
```

`jet check` reports `ImpureCallInPureContext` for each of the six functions:

- `peek_read` names `files.read`;
- `peek_write` names `files.write`;
- `peek_env` names `sys.get`;
- `peek_impure` names `print`;
- `peek_callee` names `helper_impure`;
- `peek_unsafe` names `process.exit_unmapped`.

The seventh error is the probe's own typo: `FallbackTypeMismatch` on
`files.write(…) ?? 0`, whose success value is `Unit`. It does not affect
the verdicts.

`infer.jet`:

```jet
// Probe G3: which closure forms infer the result type under a written row?
run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    a: fn() -[]> { xs.len() + 1 }
    print(a())
}
```

`jet check`: `ValueThrownAway` at 5:20, "`a` has no return type, so it
returns nothing and the value of its last line is lost".

`infer2.jet`:

```jet
// Probe G3: a lambda passed to a generic parameter infers its result.
jet_debug_entry: fn<T>(body: fn() -[]> T) -[]> T { body() }

run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    print(jet_debug_entry(fn() { xs.len() + 1 }))
    print(jet_debug_entry(fn() { "{xs.len()} items" }))
}
```

`jet check`: no problems. `jet run`: `4`, then `3 items`.

`infer3.jet`:

```jet
// Probe G3: the bound `-[]>` of the parameter applies to an unannotated lambda.
jet_debug_entry: fn<T>(body: fn() -[]> T) -[]> T { body() }

run: fn() {
    xs := [Int]{1, 2}
    print(jet_debug_entry(fn() {
        print("side effect")
        xs.len()
    }))
}
```

`jet check`: `CallbackEffectBoundExceeded` at 6:27, "This callback uses the
effect `IO`, which the parameter doesn't allow".

`elfsyms.py` (Python 3, standard library only):

```python
# Probe: census of an ELF64 image's symbol tables. Usage: python3 elfsyms.py <image>
import collections, struct, sys

data = open(sys.argv[1], 'rb').read()
assert data[:4] == b'\x7fELF'
e_type, = struct.unpack_from('<H', data, 16)
shoff, = struct.unpack_from('<Q', data, 0x28)
shentsize, shnum, shstrndx = struct.unpack_from('<HHH', data, 0x3a)
secs = [struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * shentsize) for i in range(shnum)]

def text(table, offset):
    start = table[4] + offset
    return data[start:data.index(b'\0', start)].decode(errors='replace')

print('e_type', e_type)
print('sections', [n for n in (text(secs[shstrndx], s[0]) for s in secs) if n.startswith(('.debug', '.jet'))])
for s in secs:
    if s[1] != 2:  # SHT_SYMTAB
        continue
    kinds = collections.Counter()
    names = collections.Counter()
    for i in range(s[5] // 24):
        st_name, st_info, _, _, _, _ = struct.unpack_from('<IBBHQQ', data, s[4] + i * 24)
        kinds[st_info & 15] += 1
        if st_info & 15 == 2:  # STT_FUNC
            names[text(secs[s[6]], st_name)] += 1
    generic = [n for n in names if '[' in n or '<' in n]
    shared = sorted(((c, n) for n, c in names.items() if c > 1), reverse=True)
    print('symbols', sum(kinds.values()), 'by STT type', dict(kinds))
    print('function names with type arguments', len(generic))
    print('names shared by several functions', len(shared), shared[:3])
```

On the probe `jet` image it prints:

- `e_type 2` (`ET_EXEC`);
- sections `.debug_abbrev`, `.debug_info`, `.debug_line`,
  `.debug_loclists`, `.debug_jetline` and `.jet_command`;
- 43457 symbols, of which 43454 are `STT_FUNC`, 2 are `STT_OBJECT` and 1
  is `STT_NOTYPE`;
- 0 function names with type arguments;
- 79 names shared by several functions, led by
  `BinderDescriptor.debug_name` (14), `Parse.finish` (11) and
  `Parse.assign` (10).

Other probes:

- `/proc/self/smaps` of the probe shell counts `VmFlags` `dc` = 1 and
  `wf` = 4.
- `/proc/sys/vm/overcommit_memory` is 0.
- `Image/Dwarf.jet` has no `DW_LNS_set_column`; its one "column" is the CFI
  return-address column.
- A search of `Compiler/JetDriver`, `Compiler/JetQueries` and
  `Compiler/JetSema` finds no item-level reuse inside a rechecked package.

### Revision 3 probes

Every revision 3 probe is committed whole in
[`DebugEvalNative20261118/Probes/`](DebugEvalNative20261118/Probes/), with
the runner that ran them all in order, `run_all.py`, and its exact output,
[`run_all.log`](DebugEvalNative20261118/Probes/run_all.log). The runner takes
the `jet` to use and the repository root: `python3 run_all.py <jet> <repo>`.
It runs each command at `nice 19` with `NO_COLOR=1`, removes machine paths
from the output, and writes IR listings to a temporary directory. The log
was made with the lane's probe `jet` on 2026-11-18. Builds go to the
ignored `.jet/build`. The Python probes use only the standard library.

| Probe | Question | Result in `run_all.log` |
|---|---|---|
| `place_moved.jet` | At a stop after `box := Box{items: xs}`, what do the Locals pane and `print xs` show? (N1) | `xs: [Int] = [1, 2, 3]` and `box: Box = Box{items: [1, 2, 3, 4]}`; the program prints `4` |
| `take_arg.jet` | After `f` from inside `consume`, what does `run`'s frame show for the argument `consume` took? (N2) | In `consume`, `items = [1, 2, 3]   xs = [1, 2, 3]`; after `f`, `12 \| n: consume(xs)` and `xs: [Int] = [1, 2, 3]`; the program prints `4` |
| `take_arg_later.jet` | Where does sema's final walk place the move of a take argument? (N2) | `MoveWithoutMarker` at 13:16, "`consume` takes `xs`, but `xs` is used again later", with the use at 14:11 |
| `moved_read.jet` | What origin does sema record for an exact move? (N1) | `UseAfterMove` at 11:11, "`xs` was consumed by `xs.move()`", with the origin at 9:23 |
| `move_then_share.jet` | Between an unmarked move and a later use that makes it a share, what is real? (N2) | `box = Box{items: [1, 2]}`, `xs = [1, 2]`; the program prints `2`, then `3` |
| `row_compose.jet` | Under `-[]>` in a `#FX(!Time.Wait, !Unsafe)` module, which of the review's bodies are refused? (N3) | `ProhibitedEffectReached` for `peek_receive` (7:5) and `peek_send` (9:5) (`Time.Wait`) and for `peek_raw` (19:5) (`#Unsafe`); `ImpureCallInPureContext` for `print` (29:9); the atomic store and the pure read pass |
| `unsafe_callee.jet` | Is a raw write in a callee outside the rung refused too? (N3) | `ProhibitedEffectReached` at 15:5, with the call chain `peek_callee → poke` |
| `row_forms3.jet` | Does a denial-only arrow row work as the probe's row? (N3) | The wait is refused, with the row misprinted as `-[!Time.Wait, Unsafe]>`; `print` under the same row passes |
| `fx_forms.jet` | Which of the spec's body `#FX` forms does the compiler accept? (N3) | `#FX(IO) { … }` passes; `#FX(!Time.Wait) { … }` gives `MarkerArgumentMismatch` at 11:6; the `#FX: (!Time.Wait)` line draws no diagnostic |
| `fx_line.jet` | Does an `#FX: (…)` line in a `-[]>` body deny? (N3) | `InvalidScopedPolicy` at 6:5 and 11:5, "`#FX` cannot attach to a function" |
| `shared_denied.jet` | Does `!Time.Wait` keep the `Shared` read of G5? (N3) | Under `#FX(!Time.Wait, !Unsafe)`, `cell.hits` is refused at 9:5; under `#FX(!Unsafe)`, it and `rx.receive()` pass |
| `unsafe_reach.jet` | Does `!Unsafe` refuse ordinary expressions through Core's internals? (N3) | No errors (one style warning); the run prints `20`, `3 items, first 3`, `true` |
| `wait_link.jet` | Which runtime entry points does each wait reach? (N3) | `peek_receive` → `jet_rt_native_receiver_receive`, `peek_send` → `jet_rt_native_sender_send`, `peek_shared` → `jet_rt_native_shared_read`; the run prints `9` |
| `r7_scan.jet`, `r7scan.py` | Which of six conditions can R7 place under the scan rule? (N6) | Placeable: `@539`, `@588`, `@665` and `@892`; fork per hit: `@716` (map and sum) and `@796` (`jet_rt_atomic_bool_store`); the run prints six `true`, then `4` |
| `asmcalls.py` | Which asm functions in `OS/Linux.jet` call a register, and who calls them? (N4) | 20 functions; the runtime calls 13 of them, and 7 have no runtime caller |
| `rtindirect.py` | Which runtime functions call a loaded code address? (N4) | 61 of 7274 call one through `coll_callN`; 2175 of 3253 exported entry points reach one |
| `Inputs/` (`main.jet`, `shapes.jet`, `files.py`) | Which files do the debug sections name? (N5) | `.debug_line` and `.debug_jetline` name `main.jet` only; `shapes.jet` appears only inside a type identity in `.debug_info`; the run prints `7` |
| `Sleep/sleeper.jet`, `tcb.py` | Which base register holds a running Jet image's TCB? (N7) | `fs_base = 0x0: unset`; `gs_base` inside a 1 MiB anonymous `rw-p` mapping |
| `seccomp_args.py` | Do the argument checks pass the fork's forms and refuse the escapes? (N8) | The 4 allowed forms pass; `PROT_WRITE`, a shared map of an inherited fd, `MADV_REMOVE` and a shared futex wake get `EPERM` |
| `dontfork.py` | Can the fork's `mmap` land in a `dc` hole, and which reads fault? (N9) | Without the reservation the arena lands in the hole and every read gives `0x00`; with it, `process_vm_readv` gives `EFAULT` and the eval code `SIGSEGV`, while `/proc/<pid>/mem` and `PTRACE_PEEKDATA` read `0x00` |
| `globals.py` | Do module globals collide by `<module>.<name>`? (N10) | 166 mutable keys, 1 shared: `ScriptHelperFixtures.words`, two script fixtures inside string literals of one file |
| `fork_cost.py` | What does one fork cost by resident size? (N6) | §8.2's table, three runs |

### Revision 4 probes

Every revision 4 probe is committed whole beside revision 3's, with its
runner, `run_r4.py`, and its exact output,
[`run_r4.log`](DebugEvalNative20261118/Probes/run_r4.log):
`python3 run_r4.py <jet> <repo>`. Both runners now take their shared
helpers from `probes.py`; `run_all.log` was not rerun for this revision.
The log was made with the lane's probe `jet` on 2026-11-18, one command at
a time at `nice 19` under a 4 GB memory cap. Builds go to a temporary
directory.

| Probe | Question | Result in `run_r4.log` |
|---|---|---|
| `chain.jet` | At a stop in each of two takes in `start.add(xs).add(ys)`, what does `run`'s frame show? (P1) | Both `f`s show `18 \| out: start.add(xs).add(ys)`. First stop: `xs = [1, 2]`, `ys = [3, 4, 5]`. Second stop: `xs = [3, 4]`, `ys = [3, 4, 5]`. The program prints `4` |
| `chain_later.jet` | Where does sema's final walk place the take of `xs` in the chain? (P1) | `MoveWithoutMarker` at 18:20, "`add` takes `xs`, but `xs` is used again later", with the use at 19:11 |
| `take_arg.jet` at `-O2` | What is left of the take at O2? (P2) | "Breakpoint hit take_arg.jet:5 in run()"; `bt` is one frame, `#0 run() at take_arg.jet:5`; `items = [1, 2, 3]   n = <Unavailable>   xs = <Unavailable>`; `f` has no caller frame to stop in, and the program runs on and prints `4` |
| `journey.jet` | Which runtime entry points do ordinary fallbacks over failures reach? (P4) | 17 entry points, `jet_int_parse`, `jet_journey_reset`, `jet_journey_origin`, `jet_journey_frame_text` and `jet_rt_cleanup_enter` among them; the run prints `16`, then `6` |
| `linkchecks.py` | §5's checks as this revision states them, over the runtime's source and `OS/Linux.jet` (P3, P4, P5, P7) | 7369 runtime functions, 3270 exported, 147 module globals, 95 asm bodies. P5: 22 transfer bodies (20 register calls, the fiber switch's `ret` on a loaded stack, `wrgsbase`). P3: `alloc_lock` and `sync_futex_wait_shared` pass the first two checks and wait; 9 functions would with `sync_futex_wait`'s scratch moved; 836 exported entry points pass the first two checks, none reaches a futex wait, 2 reach `statx` or `writev`. P7: 834 pass three checks, 63 reach a store, 2 store through a parameter (`jet_rt_mem_fill`, `jet_rt_mem_fill_bytes`); `jet_char_len` writes through `rt_text_cache_store`; `jet_rt_atomic_bool_load` writes nothing. P4: revision 3's table refuses 11 of `journey`'s 17 entry points, this revision's none |
| `r7scan.py --readers jet_rt_atomic_bool_load` | Which of the six conditions does R7 place with the mark? (P7) | Placeable: `@539`, `@665` and `@892`; fork per hit: `@588` (`jet_char_len`), `@716` and `@796` |
| `bounds.jet` | Do the build options change what one source does? (P6) | Source digest `e056b6ab32d21633`. Stock image `789a73872f4cd3ec`: prints `2`, then stops at `xs[5]` with E3010. `--bounds=report` image `2c6c1d4adc175d9d`: prints `2`, `1`, `went on`, and reports the bounds failure |
| `genmod.jet` | Do two instances of one generic module keep separate globals from one site, and does a top-level `Shared` global work? (P8) | The run prints `9`, `65`, `2`. The members are `__jet_large_dbump` and `__jet_small_dbump`; `small`'s adds 1 to `const 8`. A breakpoint at `genmod.jet:6` never hits, and `s` from line 16 lands on line 17 |
| `genmod_mutable.jet` | Does a generic module's body take a mutable global? (P8) | `InvalidStatement` (E0003) at 4:5, "Expected a function, struct, enum, or other item inside a module" |
| `bounds.jet:4` breakpoint | The control for `genmod`'s breakpoint: does a one-line function stop? | It stops twice, with `bt` showing `peek()` at `bounds.jet:4` under `run()` at `bounds.jet:8` |
