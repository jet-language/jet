# Adversarial review of the native debug evaluator proposal, revision 2 — 2026-11-18

Author model family: Claude Opus. Adversarial model family: Claude Opus. Fresh agent: IfHole-6-1-3.

Reviewed: `Docs/Proposals/2026-11-18-debug-eval-native.md` revision 2 (#6841
R3 and R8), as submitted on base `ebce85f295`, against the tree at
`4171547dc0` and the gate-p293 `jet` image. It follows the first review,
`Docs/Audits/2026-11-18-debug-eval-native-review.md`, and AGENTS.md
"Adversarial design review". Each claim the first review failed or limited,
and each gap G1-G17, is checked again against code or a probe. New claims of
the revision are attacked as N1-N10. The peers are the first review's
(lldb with its IRInterpreter, gdb `compile`/`checkpoint`, rr diversions,
Delve `debugCallV2`, JDWP `InvokeMethod`, .NET func-eval with `nse`); where a
new mechanism has a closer peer, it is named in that section. Missing
context counts as a failure.

## Verdicts

| # | Claim | First review | Revision 2 |
|---|---|---|---|
| C1 | One evaluator for every surface, R7 included | holds only within stated limits | holds only within stated limits (N3, N6) |
| C2 | The expression gets the compiler's own scope and ownership state | fails | fails (N1, N2) |
| C3 | The row refuses every positive root and the escapes | fails | fails (N3) |
| C4 | Evaluation is refused when the source changed since the build | fails | fails (N5) |
| C5 | Generic frames | holds only within stated limits | holds only within stated limits |
| C6 | O1 and O2 locals | holds only within stated limits | holds only within stated limits |
| C7 | No image code runs | fails | fails (N4) |
| C8 | Module globals bind to the image | fails | holds only within stated limits (N10) |
| C9 | A toolchain digest match guarantees matching layouts | holds | holds |
| C10 | Runtime stubs and the test that enforces them | holds only within stated limits | holds only within stated limits (N4) |
| C11 | The injected `clone` leaves the target as stopped | holds only within stated limits | holds |
| C12 | No deadlock, no blocking wait, a budget for the rest | holds only within stated limits | holds only within stated limits (N3) |
| C13 | Descriptors, signals, timers and shared memory | holds only within stated limits | holds only within stated limits (N8, N9) |
| C14 | The call's stack | holds only within stated limits | holds |
| C15 | Results and `debug_view` run no target code | fails | fails (N4) |
| C16 | Dumps and replays | fails | holds only within stated limits (N7) |
| C17 | Latency and R7 | holds only within stated limits | holds only within stated limits (N6) |
| C18 | I9: one meaning at every level | fails | fails (N1, N2) |
| C19 | Alternatives considered | holds only within stated limits | holds |

Five claims still fail: C2, C3, C4, C7 (with C15, which inherits it) and
C18. N1, N2, N4 and N5 can show a wrong value or run image code; N3 makes a
stated guarantee false, and the fork contains its effects everywhere except
R7. The design stays unbuilt. #6841 c40 is not met.

## Gaps G1-G17

| Gap | Status | Why |
|---|---|---|
| G1 | open | A place path skips the check and shows a moved local (N1). An outer frame's probe sits before the arguments its call moved (N2). A refusal made at the node can name a move that a later use turned into a share (N2). |
| G2 | open in part | The column rule covers loop headers, multi-line expressions and one-line arms. Outer frames fail (N2). |
| G3 | closed | Probe `infer2` rerun here: no problems, then `4` and `3 items`. |
| G4 | open | `print`, files, env and Net are refused (Net: probe `net_row`, E3403). Channel waits, atomic stores and `#Unsafe` raw writes pass (N3). |
| G5 | closed | The lock stub reads the word: a free lock is taken in the fork and a held one is refused (§5). |
| G6 | open | The digests cover only files that emitted a function (N5). |
| G7 | closed as a plan | Template parameters are step 1, refused until they land. Probe: the image's strings hold no `__generic__` instance name, so the symbol table cannot stand in. |
| G8 | closed within limits | O1 and O2 are refused until the parity test passes. The test covers only the rows and locals its corpus reaches (C6). |
| G9 | open | The runtime object's own indirect calls read code addresses from target memory (N4). |
| G10 | closed within limits | Per-global symbols, with a naming hazard (N10). |
| G11 | closed | The stub test reads the runtime's IR for the module globals each reachable function references (§5). |
| G12 | closed | Signal-delivery-stops during the injection are suppressed and redelivered; the options are saved and restored (§6.0). |
| G13 | open | At O0 even `xs.len() > 2` allocates its result, so the in-program rule admits no real condition (N6). The rule also admits writes to target memory (N3). |
| G14 | closed within limits | The order and the trap are right. The allow-list has no argument filters (N8), and a `MADV_DONTFORK` hole can be reused (N9). |
| G15 | closed within limits | The TCB base is GS, not FS (N7). |
| G16 | closed | Place paths, the four DebugLatency rows with lldb figures, and item reuse named as the compiler's fix. |
| G17 | closed | lldb's hybrid, rr diversions, gdb `checkpoint`, Delve and JDWP are answered (§11). |

## Findings

### N1. A place path shows a local the program moved (C2, C18, G1)

**Claim** (§1 and §8.1). A capture the program had moved before the stop is
refused with the move's site. A place path (`x`, `p.field.sub`) is rendered
with no compile, by the c4 renderer's own read.

**Attack.** Without a compile, the place path has no ownership state, so
nothing can refuse a moved local. `print xs` is the most common expression.

**Probe** `place_moved.jet`, run with `jet debug place_moved.jet --break
place_moved.jet:12` and the commands `locals`, `print xs`, `print box`,
`c`:

```text
Locals:  box = module:14:source-root:Box{items: [1, 2, 3, 4]}   xs = [1, 2, 3]
(Jet) print xs
xs = [1, 2, 3]
```

`xs` was moved into `box` at line 10, and `box.items` now holds 4 items. The
read shows a 3-item `xs`, a value that exists nowhere in the program, with no
refusal. The proposal routes exactly this read past the probe check.

**Verdict: fails.** A place path has to consult the same per-position move
table the probe check records (§3.2). That table is one lookup per
position, not a compile. Without it, a moved place must be refused.
(Separately, the c4 renderer names the type `module:14:source-root:Box`, not
`Box`.)

### N2. The probe's position and move state are not the image's (C2, C18, G1, G2)

**Claim** (§3.1). In an outer frame the position is the call expression, and
the probe sits at the innermost node that starts there. "The statement
containing it has not finished, so the names it binds are not in scope."
(§3.2) A read of a place that sema's state at the node holds as moved is
refused with that origin.

**Attack 1: arguments move before the call.** In an outer frame the pc is a
return address. The callee is running, so every argument has already been
evaluated and moved into it. A probe at the start of the call node runs
before sema walks the arguments. `SemaOwnershipMoveOrigin.implicit_take`
(`Sema/Ownership/Checks.jet:119-139`) is exactly this case: "an unmarked
argument to a take parameter, moved as its name's last use". For
`n: consume(xs)` with `consume: fn(xs: &&[Int])`, a stop inside `consume` and
an evaluation of `xs.len()` in `run`'s frame sees `xs` unmoved. It reads the
caller's slot, whose block the callee owns and may have grown in place or
freed. That is a wrong value or a read of freed memory.

**Attack 2: a move at the node is tentative.** The same struct says of
`share_ty`: "A later use turns that site into a share." sema records an
unmarked move of a whole binding and converts it into a share when it meets
a later use. Take a stop between the move and that later use. The state at
the node holds `xs` as moved, so the probe refuses it, saying "`xs` was
moved into `Box{…}` at main.jet:4; it has no value here". But the built
program shared `xs`, so it does have a value there. The refusal is false,
and so is its message.

**Verdict: fails.**
- The outer-frame position must sit after the call's arguments (the node
  the pc - 1 row ends at).
- A refusal must read the move origins as they stand after the walk, not at
  the node.
- Plan step 2's byte-equal facts test proves that the probe changes no fact.
  It cannot catch either defect. Both need cases: a stop in a take callee,
  and a stop between a move and the later use that shares it.

### N3. `-[]>` admits waits, atomic stores and raw writes (C3, C12, C1, G4, G13)

**Claim** (§0, §3.4, §6.1). The probes show that `-[]>` refuses "`#Impure`
and `#Unsafe` blocks". "`-[]>` refuses `time.sleep`, channel operations and
every other wait." "The row refuses every positive root, including the
escapes."

**Attack.** The spec lists `Time.Wait`, `Unsafe`, `Panic` and `Mem` as
deny-only rights (spec.md "Effects", "neither are the deny-only rights";
"`Time.Wait` is the deny-only wait"). An empty positive ceiling does not
deny them. `time.sleep` is refused only because it reads the clock (E3403,
`NondeterminismInPureEval`), not because it waits. The proposal's own probe
`pure` shows a `Shared` read passing `-[]>`, while the first review showed
that it needs `Time.Wait`.

**Probe** `wait_row.jet`, with four functions declared `-[]>`:

- `rx.receive() ?? 0` on a `Receiver<Int>`;
- `tx.send(7)`;
- `flag.store(true)` on an `Atomic<Bool>`;
- `#Unsafe("…") { mem.volatile_write(p, 42) }` through a pointer to a local.

`jet check` reports no errors. Its one warning, `MutableNeverReassigned` on
the written local, shows that sema does not even see the raw write. `jet run`
prints `51`, which is 1 + 7 + 1 + 42: the raw write took effect.

**Consequences.**
- In the fork, the channel calls are caught one layer later, by the link
  refusal of §5's last row: channel entry points have no stub. The raw
  write and the atomic store land in the fork's private copy, or fault on a
  `MAP_SHARED` page made read-only (§6.2 step 4). So the fork contains them.
- R7's in-program placement (§8.1) excludes only allocation, `Shared`
  locks and indirect translation. An atomic store or an `#Unsafe` write in
  a condition would run in the target and change the program.

**Verdict: fails** as written.
- The eval ceiling must also deny the deny-only rights: `-[!Time.Wait,
  !Unsafe]>` under the empty ceiling, or the bare `#FX` form with those
  denials. Probes must then show the receive, the send and the raw write
  refused.
- `Atomic` stores need an explicit answer: refused, or fork-only.
- The §3.4 table must drop "#Unsafe blocks refused". The probe that backs it
  refused only the call inside the block.

### N4. The runtime object's indirect calls run image code (C7, C10, C15, G9)

**Claim** (§4.2). "Eval builds lower every `CallIndirect`" through
`jet_eval_call_target`, so no image code runs.

**Attack.** The lowering covers code the eval build compiles. The runtime is
linked as the prebuilt `jet_runtime_jet.o` (§5), and the runtime calls code
addresses that it loads from heap objects:

- `Iter.jet:76-94`: a lazy iterator node stores the closure's `invoke` at
  node+56 and its environment at node+48. `iter_unary` and `iter_binary`
  call `coll_call3`/`coll_call4` on them. A hooked kind calls node+56 at
  line 136.
- `Iter.jet:96-101, 742`: equality, order and text callbacks are read from
  the element metadata at meta+32, meta+40 and meta+24. `iter_meta` loads
  the metadata pointer from the node itself (line 17).
- `Shared.jet:156, 174, 201, 436, 767`: a `Shared` cell's `invoke` callbacks,
  and its payload's `drop_code`.

An iterator the program built, held in a local and pulled by the evaluated
expression (`it.count()`), or a `debug_view` that walks one, reaches these
runtime functions. They are not stateful by §5's test, since they read no
module global, so they are linked as they are. They then jump to the image
addresses stored in the target's node or metadata. That is image code
running against image state, which is what C7 and R8 forbid.

**Verdict: fails.**
- Either every runtime function that calls a loaded code address gets an
  eval twin that calls through `jet_eval_call_target`, or the eval link
  refuses it.
- §5's test must also check each reachable runtime function for calls to
  loaded code addresses, not only for global reads.

### N5. The source digests miss files that emit no function (C4, G6)

**Claim** (§3.5). The `.debug_line` file entries carry `DW_LNCT_MD5`, and
"the debugger compares the digest of each file it probes".

**Attack.** The line table's files are the files that emitted a function:
`ir_x64_tables_add` (`JetIR/Source/X64Module.jet:433-437`) adds
`f.source_file` only for an emitted function. A file that declares only
types or constants has no entry, nor does a file whose every function O2
inlined away. Yet the eval object is built from current source (§4.1). If a
struct in such a file gained a field after the build, the eval code reads
the target's old layout with the new offsets and shows a wrong value. No
digest catches it. The probed file is also not the only input: the check
reads every file of the package and its dependencies, and so does the
eval build, including the immutable globals it "compiles in" (§4.3).

**Verdict: fails.** The image must record a digest of every source file
the build read, for example a digest list in `.debug_jetline` or one
build-input digest beside the toolchain digest. Evaluation must compare all
of them. `DW_LNCT_MD5` remains useful for gdb and lldb, but it is not the
staleness proof. Whether gdb and lldb check `DW_LNCT_MD5` against the file
on disk was not verified here; the claim needs a source.

### N6. In practice, R7's in-program rule places no condition (C1, C17, G13)

**Claim** (§8.1). A condition is placed in the program only when its build
reaches no allocation, no `Shared` lock and no indirect translation. "Such a
lambda reads only the frame and immutable data."

**Probe** `r7_rc.jet`, then `jet emit --ir`. The lambda `fn() { xs.len() > 2
}`, passed to `fn<T>(body: fn() -[]> T)`, lowers at O0 to `load`, `gt`, then
`call_runtime jet_rt_alloc_value (16, 8)`, storing the tag and the `Bool`
into the new block. It returns its `Bool ! Err` result in an allocated
value. Creating the closure in `run` also calls `jet_rt_block_retain` on the
captured list.

**Attack.**
- Even the simplest comparison allocates, so the rule excludes every
  condition. Every hit then costs a fork. The case R7 exists for, a
  condition false a million times, is answered only with the message that
  it will be slow.
- The rule does not exclude a refcount write. The debugger fills the
  environment "with no retain" (§4.1). In the target, any retain or release
  that the lambda's own code performs would change real counts.
- The rule also does not exclude N3's atomic stores and raw writes.

**Verdict: holds only within stated limits.** One meaning holds, because
everything forks. The million-hit cost stays open. Answers could be an
eval lowering that returns small results in registers, or a fork reused
across hits with its writes discarded. Either needs its own §8.2 row.

### N7. The dump host restores the wrong TCB register (C16, G15)

**Claim** (§7.1 step 3). The host sets "its `fs` base (the TCB)" from the
dump.

**Attack.** The runtime's TCB is GS:
- `Thread.jet:103-107` `thread_base` reads `os_tls_base_i64`, and its
  comment names GS;
- `StackSegment.jet:6` reads the stack check `cmp rsp, gs:[STACK_TCB_LIMIT]`;
- `Fiber.jet:225-229` writes `ARCH_SET_GS`.

§5 of the proposal quotes the `gs:` form itself. With FS restored and GS
left as the host's, every TCB read in the host reads the wrong page: the
stack check, the stop poll and the allocator's thread state.

**Verdict: holds only within stated limits.** R9's format must carry the
`gs_base` (and `fs_base` where FFI code uses it) of each thread, and step 3
restores both. The format also needs file offsets aligned to pages, since
`MAP_PRIVATE | MAP_FIXED` from the dump file requires them.

### N8. The second wall can be undone through the syscalls it allows (C13, G14)

**Claim** (§6.2). A seccomp filter that allows `mmap`, `munmap`,
`mprotect`, `madvise`, `futex`, `rt_sigreturn` and `exit_group` is "a second
wall". Making the writable `MAP_SHARED` mappings read-only means that "a
write would carry back to the target; now it faults".

**Attack.** The filter checks only syscall numbers, not their arguments:
- `mprotect` can make the shared mapping writable again;
- `mmap` of an inherited descriptor with `MAP_SHARED | PROT_WRITE` writes
  through to the target's file;
- `madvise(MADV_REMOVE)` frees the backing store of shared memory;
- `futex(FUTEX_WAKE)` on a word in a shared mapping wakes the target's
  waiters.

None of these is reachable while the first wall holds. N3's raw writes are
plain stores, and the row refused every syscall-making call that was probed
(the proposal's `roots`, and `net_row` here). So this is defense in depth
that does not defend against the code it contains.

**Verdict: holds only within stated limits.** The filter should check
arguments:
- `mmap` only with `MAP_PRIVATE | MAP_ANONYMOUS` and fd -1;
- `madvise` only with `MADV_DONTNEED`, `MADV_FREE` and `MADV_HUGEPAGE`;
- `futex` only with `FUTEX_PRIVATE_FLAG`;
- `mprotect` only without `PROT_WRITE`, after the eval object and arena are
  mapped.

### N9. A `MADV_DONTFORK` hole can be reused (C13, G14)

**Claim** (§6.2 step 5). A `dc` mapping is absent from the fork, so a read
of it faults and is reported, "never shown as zeros".

**Attack.** The hole is free address space in the fork. The arena's `mmap`
(§5) or the eval object's "free range of the fork's address space" (§4.4)
can land in it. A target pointer into the region then reads eval bytes and
shows them with no fault.

**Probe.** The proposal's `dc`/`wf` count came from the probe shell, not a
Jet image. The smaps of a running Jet image (`sleeper.jet`, built with `jet
build`) show no `dc` or `wf` mapping. The case arises only with FFI
libraries that set them.

**Verdict: holds only within stated limits.** Before the call, the fork
reserves each `dc` range `PROT_NONE` with `MAP_FIXED_NOREPLACE`.

### N10. Per-global symbols named like function symbols collide (C8, G10)

**Claim** (§4.3). One `STT_OBJECT` symbol is added per bound global, "named
`<module>.<name>` as function symbols are".

**Attack.** The proposal's own census finds 79 function names shared by
several functions, and its §4.2 rejects names as identity for that reason.
Global names built the same way collide the same way. Two modules' `count`
would bind one address. gdb and lldb locate globals through a CU-level
`DW_TAG_variable` with a `DW_OP_addr` location. `Dwarf.jet` emits
`DW_TAG_variable` (abbrevs at lines 439-441) only for a function's locals.

**Verdict: holds only within stated limits.** Bind through a CU-level
`DW_TAG_variable` with `DW_AT_decl_file`/`DW_AT_decl_line` (the identity
§4.2 already uses), or make the symbol name carry the module identity.

## Claims rechecked

- **C1.** Every surface goes through one path. R7 forks per hit in practice
  (N6), and its in-program rule admits target writes (N3).
- **C2.** Fails through N1 and N2. The no-overlay direction is sound: the
  moved-place state the probe would read exists
  (`Checks.jet:189-197`), and the design keeps probe reads out of
  `SemaOwnershipShareSite`.
- **C3.** Fails through N3. `print`, file reads and writes, env reads, Net
  (`net_row`: E3403), module writes and impure callees are refused.
- **C4.** Fails through N5.
- **C5.** Refused until template parameters land, as stated. No symbol names
  an instance (G7 row).
- **C6.** The parity test is corpus-bound. A located O2 capture at a row
  the corpus never stops at is read on trust once the test passes.
- **C7, C15.** Fail through N4.
- **C8.** Compiling immutable globals in is sound under D-MODULE-BUILD1
  (spec.md "computed while building and stored in the program"), given
  N5's digests. Mutable globals have N10.
- **C9.** Holds.
- **C10.** The globals test is right, but it misses loaded code addresses
  (N4).
- **C11.** The redelivery and option restore answer G12.
- **C12.** Layer 2 is false (N3). Layers 1 and 3, the link refusal and the
  budget, still catch a channel wait.
- **C13.** N8 and N9.
- **C14.** Stack growth in the fork touches only TCB words and its own
  `mmap`/`madvise`/`munmap` (`StackSegment.jet:131-231`). It reads no module
  global on the grow path. Holds.
- **C16.** Replays hold as rr diversions. Dumps have N7.
- **C17.** The latency text claims no figure, and the bars are measurable.
  R7 has N6.
- **C18.** Fails: N1 and N2 give wrong values at O0, so "means what the same
  text means at that point" does not hold.
- **C19.** Holds. The interpreter rejection is argued on I9's one meaning
  and is reconsidered against the bars.

## Cards

Each finding needs a card or a revision of the proposal (this worker cannot
write Tower):

- N1: place paths through the move table;
- N2: outer-frame position after the arguments, and post-walk refusals;
- N3: the eval ceiling denies `Time.Wait` and `Unsafe`, and `Atomic` stores
  get an answer;
- N4: runtime indirect calls in the eval link;
- N5: a digest of every build input;
- N6: R7's per-hit cost;
- N7: GS in the dump host and in R9's format;
- N8: seccomp argument filters;
- N9: reserving `dc` holes;
- N10: global binding identity.

## Probes

All compiler probes ran with the gate-p293 seed compiler, `NO_COLOR=1`, one
at a time at `nice 19` under a 3 GB memory cap. The built `sleeper` image
ran on its own.

`place_moved.jet` (`jet debug place_moved.jet --break place_moved.jet:12`,
with the commands `locals`, `print xs`, `print box`, `c` on stdin):

```jet
// Probe: what does a place read (today's `print`, the §8.1 fast path) show
// for a local the program moved before the stop?
Box: struct {
    items: [Int],
}

run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    box := Box{items: xs}
    box.items.push(4)
    print(box.items.len())
}
```

Output: `xs = [1, 2, 3]`, `box = module:14:source-root:Box{items: [1, 2, 3, 4]}`,
then the program's `4`.

`wait_row.jet`:

```jet
// Probe: which waits and writes does the empty row `-[]>` admit?
use mem: core.mem

peek_receive: fn(rx: Receiver<Int>) -[]> Int { rx.receive() ?? 0 }

peek_send: fn(tx: Sender<Int>) -[]> Int {
    tx.send(7)
    1
}

peek_atomic: fn(flag: Atomic<Bool>) -[]> Int {
    flag.store(true)
    1
}

peek_raw: fn(total: Int) -[]> Int {
    cell := total
    #Unsafe("probe: a raw write under the empty row") {
        p: mem.Ptr<Int>.from_addr(mem.address_of(cell))
        mem.volatile_write(p, 42)
    }
    cell
}

run: fn() {
    { sender: tx, receiver: rx }: channel<Int>(capacity: 8)
    flag: Atomic<Bool>.try_new(false) ?? panic("no flag")
    print(peek_send(tx) + peek_receive(rx) + peek_atomic(flag) + peek_raw(1))
}
```

`jet check`: no errors, and one warning, `MutableNeverReassigned` on `cell`.
`jet run`: `51`.

`net_row.jet`:

```jet
// Probe G4: does the empty row `-[]>` refuse a Net call?
use net: core.net

peek_net: fn() -[]> Int {
    port: net.getservbyname("http") ?? 0
    port
}

run: fn() {
    print(peek_net())
}
```

`jet check`: E3403 `NondeterminismInPureEval`, "`net.getservbyname` is
non-deterministic and cannot appear in a pure evaluation".

`infer2.jet` is the proposal's probe, rerun as given: `jet check` reports no
problems, and `jet run` prints `4`, then `3 items`.

`r7_rc.jet` (`jet emit --ir r7_rc.jet`, then `jet run`):

```jet
// Probe G13: does a condition lambda that only reads a captured list (no
// allocation, no Shared lock, no indirect call) still touch refcounts?
jet_debug_entry: fn<T>(body: fn() -[]> T) -[]> T { body() }

run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    print(jet_debug_entry(fn() { xs.len() > 2 }))
    xs.push(4)
    print(xs.len())
}
```

The lambda `run.lambda@282` lowers to `offset`, `load`, `const`, `gt`, then
`call_runtime jet_rt_alloc_value (s5, s6)` with sizes 16 and 8, two
`store`s and `return`. `run` calls `jet_rt_block_retain` before the call to
`jet_debug_entry<Bool>`. `jet run`: `true`, then `4`.

`sleeper.jet` (`jet build`, then the image run for 3 s while its
`/proc/<pid>/smaps` was read):

```jet
// Probe: a running Jet image's mappings, for their smaps VmFlags.
use time: core.time

run: fn() {
    xs := [Int]{1, 2}
    xs.push(3)
    time.sleep(3s)
    print(xs.len())
}
```

No mapping has `dc` or `wf` in its `VmFlags`. `[vvar]` has `dd`, and no
mapping holds shared memory (`RssShmem: 0 kB`).

Code probes:
- `git grep` of the runtime for `coll_call` finds the indirect calls of N4;
- `X64Module.jet:433-437` (N5);
- `Thread.jet`, `StackSegment.jet` and `Fiber.jet` (N7);
- `Dwarf.jet:380-441, 1102-1114` (N10);
- `Sema/Ownership/Checks.jet:119-197` (N2).
