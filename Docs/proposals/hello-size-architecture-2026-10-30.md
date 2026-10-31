# Size and speed against hand-written assembly: architecture plan, 2026-10-30

Cards: #6328 (hello size; C6: every program within 1.5–2x of optimized
hand-written assembly in size) and #6414 (default codegen within 1.10x of
hand-written assembly in cycles, with identical output).

This is a plan. Nothing here changes the compiler yet. Every number is either
measured (marked **measured**, with the command) or modelled (marked
**model**, with the basis). One part changes what a Jet program prints when
Jet itself has a bug; that part is an owner ballot, D-RT-DEFECT-REPORT1, at
the end.

Related: Docs/research/2026-10-30-speed-of-light.md (#6343) measures Jet
against Rust and lists the codegen criteria (P-RANGE, P-BCE, P-LICM, P-VEC,
P-IA and others). This plan measures against asm instead and uses the same
criteria names; it adds no second set.

## 1. Where we are

### Size

| program | Jet today | hand-written asm | ratio |
|---|---:|---:|---:|
| hello | 134,064 B (after IrTypes-8) | 197 B | 680x |
| collatz (loop) | 478,136 B | 888 B | 538x |
| fib (calls) | 465,752 B | 864 B | 539x |
| isum (indexing) | 709,080 B | 1,040 B | 682x |

Jet numbers are `jet build --release` from the dev compiler 94e290bb4a
(hello is the IrTypes-8 build, 134,064 B at every mode). The asm builds are
complete static binaries (`cc -nostdlib -static -Wl,-n -s`).

Peer hello worlds, all stripped and static, same machine:

| peer | bytes |
|---|---:|
| hand-written asm (this plan's floor) | 197 |
| Zig `ReleaseSmall` | 10,720 |
| C, musl, `-Os` | 17,696 |
| Rust, `opt-level=z` | 394,032 |
| Go 1.26.7, `fmt.Println`, `-ldflags='-s -w'` | 1,601,698 |

### Speed

**Measured** with `kern/perf.sh 5`: user-mode cycles and instructions from
`perf stat`, best of 5 runs, pinned to CPU 8. Every run's output equals the
reference's.

| kernel | what it stresses | arg | reference | Jet cycles / ref | Jet instr / ref |
|---|---|---:|---|---:|---:|
| collatz | integer loop, branches | 3,000,000 | asm | 3.98x | 16.2x |
| fib | calls, recursion | 40 | asm | 1.97x | 5.6x |
| isum | fill + bounds-checked reads | 10,000,000 | asm (AVX2) | 18.4x | 124x |
| sieve | gauntlet `numerics.int-kernel` | 10,000,000 | C `-O2` (proxy) | 34.9x | 112x |

The sieve reference is the gauntlet's C entry at `gcc -O2`, not yet hand
asm. Default mode and `--release` are within 4% of each other on all four.

### Why: what the measurements show

- **Every Int use re-checks its representation.** In `fib`, each use of a
  value repeats `mov; add r,r; jo` (is it a small Int?), even right after the
  last check. Each temporary also gets a drop test (`add; jno; call
  jet_rt_int_drop`), and each call result gets a stop test (`test edx; jne`).
  The hot path of one `fib` call is 73 instructions; the asm is 11. Of the
  73: 27 are representation checks, 12 drop tests, 4 stop tests, about 10
  register copies, and 11 frame setup and teardown.
- **Library loops are not inlined.** In `sieve`, 9% of the time is
  `jet_rt_native_iter_from_list`, 4% `jet_rt_iter_charge`, 3% a lazy clone,
  and most of the rest is in local runtime functions with no symbol name
  (iterator code [INFERENCE]). `run` itself is 7% (`perf record`, then
  `perf report`). In `isum`, 45% of the time is outside `run`, in the lazy
  clone, `jet_rt_iter_charge`, a drop and unnamed runtime code, which is the
  list building [INFERENCE].
- **The program carries a general runtime.** A CLI parser for one Int
  argument is about 98 KB. Hello carries a scheduler, an allocator, a stop
  rail and a fault reporter that holds about 30 KB (29,896 B, stage S3
  below). The unwind table (`.eh_frame`) is
  12–35 KB, although the runtime never unwinds.

## 2. The floor: hello in 197 bytes

`asm-floor.mjs` writes the floor binary by hand. **Measured**: it runs and
prints `hello`. It matches today's Jet hello on every stdout outcome:

| case | today's Jet hello | floor |
|---|---|---|
| terminal | `hello`, exit 0 | `hello`, exit 0 |
| `>/dev/full` | exit 0, no stderr | exit 0, no stderr |
| stdout closed (`>&-`) | exit 0 | exit 0 |
| reader gone (EPIPE) | exit 0 | exit 0 (exit 141 without the SIGPIPE bytes) |

Every byte, and why hello needs it:

| bytes | what | reason |
|---:|---|---|
| 64 | ELF header | the kernel needs it |
| 56 | one `PT_LOAD`, read + execute | the code and the text; nothing is writable, so one segment |
| 25 | `rt_sigaction(SIGPIPE, SIG_IGN)` | program mains ignore SIGPIPE (Host.jet `jet_rt_host_program_start`), so a gone reader is exit 0, not 141 |
| 37 | write loop | retries EINTR and finishes short writes (Host.jet `host_write_all`); print ignores a failed write (Host.jet `jet_term_write_stdout_line`) |
| 9 | `exit_group(0)` | the exit status of an entry that returns |
| 6 | `hello\n` | the output; the line end folds into the constant |
| **197** | | |
| 56 | `PT_GNU_STACK` | **kept.** On kernel 7.2 the stack is `rw-p` without it (**measured**, `/proc/PID/maps` of `floor-pause`). Kernels before 5.8 make the stack executable when it is missing [INFERENCE from the kernel's `elf_read_implies_exec`]. Jet states no minimum kernel, so W^X needs these 56 bytes. |
| **253** | **floor with a non-executable stack** | |

Jet's hello must also keep a Fault line if a Jet bug ever crashes it
(D-RT-DEFECT-REPORT1 A, section 8). The hello target therefore adds a fixed
fault handler. `floor/target.S` is the hand-written target, wrapped by
`asm-floor.mjs`. **Measured**: 504 B, 1.99x the 253 B floor.

| bytes | what | reason |
|---:|---|---|
| 176 | ELF header, `PT_LOAD` R+X, `PT_GNU_STACK` | as above |
| 31 | `rt_sigaction` loop for SIGILL, SIGBUS, SIGFPE, SIGSEGV | install the fixed handler; constant arguments and an action in the image, so it cannot fail |
| 19 | SIGPIPE ignore | as above; it shares the size and set-size registers with the loop |
| 37 | write loop | as above |
| 9 | `exit_group(0)` | as above |
| 6 | `hello\n` | as above |
| 109 | fault handler | writes `Fault [SIG<name>]: Jet internal defect; rebuild with --backtrace=full`; then `getpid` + `kill` with the default action back (SA_RESETHAND, SA_NODEFER), as Fault.jet `fault_raise` does; `exit_group(128 + signal)` if the signal is not delivered |
| 32 | the `sigaction` record | handler, flags, restorer, mask |
| 85 | handler text | `Fault [SIG`, the four names with an offset table, the rest of the line |
| **504** | **hello target** | |

**Measured** (`floor/build-target.sh`), with deliberate faults assembled in
for testing:

| build | stdout | stderr | status |
|---|---|---|---|
| target | `hello` | empty | 0 |
| target `>/dev/full`, target into a closed pipe | | empty | 0, 0 |
| SIGSEGV | | `Fault [SIGSEGV]: Jet internal defect; rebuild with --backtrace=full` | 139, core dumped |
| SIGILL | | `Fault [SIGILL]: …` | 132 |
| SIGFPE | | `Fault [SIGFPE]: …` | 136 |

The statuses are the default actions, the same as today's report gives.
`kill` is enough to re-raise because a program with no tasks has one thread.

What hello does not need, and why that is safe:

- **No stack check.** Hello uses 32 bytes of stack (the sigaction record) and
  makes no call. Nothing can overflow. The fault handler runs on the
  faulting thread's stack: the kernel's signal frame plus 16 bytes. It needs
  no alternate stack, because this stack cannot have overflowed.
- **No arithmetic checks.** The write loop's `add`/`sub` are bounded: the
  kernel returns at most the requested count, so the pointer never passes the
  end of the text.
- **No defect stop for sigaction.** The arguments are constants and the
  record is valid memory. `rt_sigaction` fails only for a bad signal number,
  a bad set size, or a bad pointer, so it cannot fail here.
- **No section headers, symbols, unwind table, crt, constructors, scheduler,
  allocator, stop rail, full fault report or page padding.** Nothing in
  hello's reference graph reaches them.

## 3. Size: the architecture

The rule: **the program's reference graph alone decides what is in the
binary.** Nothing is linked "just in case". Startup does nothing a program
does not use.

### Stages for hello

Modelled with `why.mjs` (section liveness of the real runtime pack and the
real hello binary) through `model-stages.mjs`. Each stage removes roots or
call edges as the change would, and adds an allowance for the code the change
adds. S0 reproduces the real binary to the byte.

| stage | change | hello bytes | live runtime text |
|---|---|---:|---:|
| S0 | today (IrTypes-8) | 134,064 | 108,763 |
| S1 | linker output: no `.eh_frame`; symbols and debug sections go to a sidecar file; segments packed with no page padding | 113,528 | 108,762 |
| S2 | zero unconditional startup: no constructor runs; each subsystem starts on first use | 104,408 | 99,667 |
| S3 | the full fault report only when the program can fault; a fixed handler (257 B) otherwise | 74,768 | 70,934 |
| S4 | runtime defects use an allocation-free reporter; static main thread block; startup arithmetic proved bounded | 59,960 | 55,703 |
| S5 | the compiler emits the program's own entry: no crt, no `jet_rt_main`, no argument capture, no stop polls | 4,327 | 2,242 |
| S6 | direct print; one read+execute segment | **504** | 0 |

S5 and S6 do most of the work, because they remove the roots that hold
everything else. S1–S4 still matter: they are what every larger program
keeps paying for.

### The changes

1. **Program-owned entry (S5).** The compiler emits `_start` for the
   program: ignore SIGPIPE if the program writes to a stream, run the entry,
   `exit_group(status)`. It saves the argument pointer only when the program
   reads its arguments. It emits stop polls and the entry-return hook only
   when the program can spawn a task. The musl-style crt (2,856 B:
   `__libc_start_main`, `__init_libc`, `__init_tls`, `__init_ssp`) goes.
   Hello's `main_run` today calls `jet_ir_stop_poll` three times; that poll
   reads `%gs:0`, so it needs a thread block, which needs the allocator.
2. **Zero unconditional startup (S2).** Every `.init_array` entry and every
   init in `jet_rt_main` (thread, allocator, scheduler, process exit, host,
   observation, test capture) becomes lazy from its own entry points. A
   subsystem that nothing reaches is not linked. **Measured**: with no
   program references and no init roots, the runtime pack contributes 0
   bytes, so there are no hidden roots.
3. **Linker output (S1).** StaticLink.jet writes no `.eh_frame`: the
   runtime and Jet code never unwind, and fault backtraces walk frame
   pointers. Symbols, DWARF and the location table go to a sidecar file
   beside the binary (`main.sym`). The binary carries no section headers.
   Segments are packed: file offsets equal virtual-address offsets modulo the
   page size, so there is no page padding in the file. A program with
   writable data gets a second read+write segment (56 B): W^X forbids
   writable code. Two runtime readers need care:
   - The fault report reads `.debug_jetline` from the image to name Jet
     frames, and stack overflow is a user-program stop (E3012) that names the
     function and line. When the fault report is linked, the location table
     stays in the binary as an allocated section that the handler reaches by
     symbol, so no section headers are needed.
   - The checked allocator (`JET_CHECKED_ALLOC=1`) names frames from
     `.symtab`. It reads the sidecar file instead.
4. **Direct print (S6).** `print` lowers to one write loop when no buffered
   stdout writer and no test capture are linked. Today `print` already
   flushes after every call (BuildExpr.jet `ir_print` passes flush 1), so the
   number of `write` calls is unchanged. When a buffered writer is linked,
   print keeps today's path so output order holds. Test builds keep capture.
   **Measured**: today's print alone holds 65,998 B of runtime, because its
   checked arithmetic reaches the stop rail.
5. **Fault handler by need (S3; ballot).** The compiler knows when a program
   can fault: recursion or a stack bound it cannot compute (the worst-case
   stack per entry point, D-STACKBOUND1 A), unsafe code, or foreign calls.
   Only then is the 30 KB report linked, and it is unchanged. A program that
   cannot fault gets the fixed 257-byte handler from section 2. A fault there
   can only be a Jet bug; see the ballot.
6. **Allocation-free defect reporter and static main thread block (S4).**
   Runtime-internal defects (out of memory, GS failure, invalid layout,
   internal overflow) write their exact text with stack-only code and exit
   with the same code. Today out of memory and GS failure already do this;
   invalid layout goes through the stop rail. The main thread's block and the
   root's save area become static (`.bss`), so startup allocates nothing. The
   text and the exit codes do not change.
7. **Bounded runtime arithmetic, with proofs.** Each checked operation in
   runtime startup and output code either gets a written proof that it cannot
   overflow (a loop counter below a length, a sum of a pointer and a count
   the kernel bounded) and becomes wrapping, or it stays checked. No blanket
   wrapping. IrTypes-8 did one such site (Host.jet flush copy loop).

### Beyond hello: what larger programs pay

The kernels show the next costs:

| part | collatz | sieve | fix |
|---|---:|---:|---|
| `.eh_frame` | 25,704 | 34,728 | S1 |
| CLI parser and entry (one Int argument) | 98,168 | 104,205 | generate a parser for the program's own `#CLI` struct (one Int field: parse, plus the exact error text) instead of linking the general parser |
| Int big-number path | 15,122 | | link only the operations the program can overflow into |
| runtime (named) | 87,748 | | S2–S5 |

Two rules for C6's 1.5–2x:

- **The reference has the same behavior.** Jet's Int grows past 64 bits, and
  a bad index stops with a report. A fair reference keeps both: it carries
  the big-number operations the program can reach and the same stop text.
  Today's asm references keep only the fast path, so their sizes are a lower
  bound. The slow paths are cold, so cycle numbers are not affected
  [INFERENCE].
- **Generated code must be as dense as asm.** `run_fib` is 483 B; the asm
  `fib` is 30 B. The speed changes below remove the same bytes.

## 4. Speed: the architecture

Each change, what it removes, and the modelled result. Models are counts of
the instructions in the disassembled hot paths, measured as above, with the
removed instructions taken out. The #6343 criterion each change belongs to
is in brackets.

| change | removes | kernels | model after |
|---|---|---|---|
| **Small-Int facts** [P-RANGE] (value range on the IR): once a value is known to be a small Int, later uses do not re-check; `n - 1` for a small `n >= 2` is small; drops of small values vanish | 27 checks and 12 drop tests of 73 in `fib`'s hot path | fib, collatz, isum, sieve | fib 73 → 30 instructions per call |
| **Stop facts** across calls: a function that cannot stop returns no stop flag, and its callers do not test one | 4 of 73 in `fib` | all calls | fib → 26 |
| **Register allocation**: coalesce copies; use only the callee-saved registers needed; leaf functions get no frame; keep the frame pointer only when the fault report is linked | ~10 copies and ~6 frame instructions in `fib` | all | fib → 13 per call against asm's 9: **~1.3x** |
| **Specialized small-Int calling convention** [P-RANGE, P-IA]: a function whose parameters are proved small gets an untagged clone | entry checks | fib | **~1.15x** |
| **Inline and fuse library loops** [P-IA, closures]: `cycle(n).to_list()`, `count`, `last_index_of`, range-with-step loops become plain loops; per-element `jet_rt_iter_charge` is batched per loop | the 45% (isum) and ~93% (sieve) of time spent outside the program's own function, in runtime iterator code | sieve, isum | sieve 35x → ~3x |
| **Bounds-check elimination** [P-BCE, P-LICM]: a loop whose index is below the list's length (`i < n`, `n == xs.len`, no resize in the loop) reads without a check, and the data pointer and length load once | one compare and branch per access | isum, sieve | sieve ~3x → **~1.2x**; isum → ~1.3x |
| **Overflow check placement** [cold stop paths]: one `jo` after each arithmetic that can grow past 64 bits, and none where the range is known | redundant checks | collatz | collatz 4x → **~1.2x** |
| **Unrolling and multiple accumulators** for counted reductions; vector code for contiguous `I64`/`U8`/`F64` reductions, maps and fills [P-VEC] | one add per cycle | isum | isum ~1.3x → **~1.05x** (the AVX2 reference reads 1.6 GB, so it is near memory-bound [INFERENCE]) |
| **Block layout** [cold stop paths]: slow paths (big Int, stops, drops) out of line, hot loops aligned | taken branches | all | small, a few % |

Reaching 1.10x on every kernel needs all of these. The first three are the
largest and apply to every program.

## 5. Corpus and harness

### Corpus

Every corpus program has a Jet source, a hand-written x86-64 asm reference
with the same output and the same behavior on every input (big Ints,
stops), the arguments, and the expected output.

- **hello**: `fn run() { print("hello") }`. Reference: `asm-floor.mjs`.
- **Codegen kernels**, one construct each:
  - loops: `collatz` (written);
  - calls: `fib` (written);
  - bounds-checked indexing: `isum` (written);
  - string length and byte iteration; slices; closures; records (struct
    fields, copies); allocation (many small lists); write (many lines to a
    pipe). These need Jet sources and asm references.
- **One program per required performance cell**: the gauntlet entries
  (`Tools/gauntlet/entries`: sieve, nbody, fft-4096, tensor-map-4k, binparse,
  wordfreq, csvtransform, logreport, regex-find-all-4mb and the rest).
  Compute kernels get hand-written asm. For application-sized entries (an
  HTTP service, a CLI app), the reference is the expert C entry built with
  `gcc -O3 -march=native`, frozen as `.S`, with its hot loops tuned by hand.
  Writing a whole HTTP service in asm is not a good use of time; tuning the
  hot loops captures nearly all of the speed.

### Harness

`c6328/kern/build.sh` and `kern/perf.sh` are the prototype. As a tool
(`Tools/perf/asm-parity/`):

- build each Jet program at default and `--release`, and each reference;
- check that the output is byte-identical to the reference's on every
  listed argument;
- size: file bytes, plus a breakdown by section and by symbol group
  (`why.mjs` for the runtime);
- speed: `perf stat -e cycles:u,instructions:u`, best of N (default 5), on
  one pinned CPU, with the ratio to the reference;
- gates: size ≤ 2x the reference (C6) and cycles ≤ 1.10x (#6414), per
  program, per mode.

## 6. Order of work, by gain per day

| order | work | est. days | gain |
|---|---|---:|---|
| 1 | Program-owned entry, lazy startup, direct print (S2, S5, S6) | 4 | hello 134 KB → under 1 KB; every program loses ~100 KB of runtime it does not use |
| 2 | Linker output: no `.eh_frame`, sidecar symbols, packed segments, location table by symbol (S1) | 2 | 25–35 KB from every program |
| 3 | Small-Int facts and drop elision | 5 | fib 1.97x → ~1.5x; collatz 4x → ~2x; smaller code everywhere |
| 4 | Generated `#CLI` parsers | 2 | ~98 KB from every CLI program |
| 5 | Inline and fuse library loops, batch `iter_charge` | 8 | sieve 35x → ~3x; isum 18x → ~3x |
| 6 | Bounds-check elimination | 4 | sieve → ~1.2x; isum → ~1.3x |
| 7 | Stop facts and register allocation | 8 | fib → ~1.3x; every call |
| 8 | Fault handler by need, defect reporter (S3, S4; after the ballot) | 3 | 30–45 KB from every program that cannot fault |
| 9 | Overflow check placement, small-Int calling convention | 4 | collatz → ~1.2x; fib → ~1.15x |
| 10 | Unrolling, reductions, vector loops | 10 | isum → ~1.05x; numeric cells |
| 11 | Corpus completion: the remaining kernels and gauntlet references | 6 | the gates cover every cell |

## 7. Evidence and reproduction

All under `/mnt/jetscratch/lanes/IR/IrTypes/c6328/`:

- `asm-floor.mjs floor`: writes `floor-hello` (197 B), `floor-nosigpipe`,
  `floor-pause`.
- `model-stages.mjs`: the S0–S6 table (`model/stages.tsv`), over the frozen
  runtime pack `model-pack/` and `m/nc4/hello-default`.
- `why.mjs`: section liveness with `--noinit`, `--cutedge`, `--drop`,
  `--from`, `--into`.
- `kern/`: kernel sources, asm references, `build.sh`, `perf.sh`, and
  `perf-today.tsv`.
- `ballot-ev/`: allocator failure witness outputs and the Go hello build.

## 8. Owner ballot: D-RT-DEFECT-REPORT1

The JSON is in `/mnt/jetscratch/lanes/IrTypes/ballot/D-RT-DEFECT-REPORT1.json`.

**Question.** When a program cannot crash on its own, may Jet carry a
one-line crash note instead of the full crash report?

**Today.** Every program carries a fault reporter of about 30 KB. If a program
crashes, it prints a `Fault [SIGSEGV]: invalid memory access` report with the
Jet frames, then the system ends it with status 128 + signal. A stack
overflow is a stop (E3012, exit 70).

**A (recommended): the full fault report only where a fault can happen.**
The compiler proves that a program cannot fault: no recursion, no unbounded
stack frame, no unsafe code, no foreign calls. Such a program gets the fixed
257-byte handler from section 2 instead of the full report. A crash there can
only come from a Jet bug. It prints `Fault [SIGSEGV]: Jet internal defect;
rebuild with --backtrace=full`, and the process ends with the same status and
core dump as today. A build that writes the `backtrace` option links the full
report. Programs that can fault keep today's report byte for byte, including
E3012. Runtime defects (out of memory, invalid layout, GS failure) keep their
text and exit codes.

**B: keep the full fault report in every program.** Nothing changes, and
every program pays about 30 KB.

**Why A.** User programs see no change, and every crash still prints a Fault
line with the signal. Recommendation losses are empty. A gets hello to 504
bytes and gives C6 a chance on every small program.

### Measured runtime defect text, today and under A

| defect | stderr | exit | under A |
|---|---|---|---|
| out of memory | `memory allocation of 9223372036854775800 bytes failed` | SIGABRT | the same |
| invalid layout | ``Stop [E3001]: `panic: invalid box layout` `` then `--> <core.prelude>:0` and the E3001 why/fix lines | 70 in a Jet program | the same |
| GS failure | the defect-unreachable header, from Trap.jet `trap_gs_failure` (not triggerable in a test) | 101 | the same |

Out of memory and invalid layout are from `run-alloc-parity.mjs`, run on the
nc4 runtime. That witness also shows a bug that exists today: invalid layout
in a C-linked image that never ran `jet_rt_main` exits 0, not 70, at bases
59213de00f and 94e290bb4a too. It is card #6416.
