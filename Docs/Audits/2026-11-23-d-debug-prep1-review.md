# Adversarial review: D-DEBUG-PREP1 draft

Date: 2026-11-23. Reviewer: DerivReadReview (a fresh agent; the draft is G1Facts').

Subject: the draft ballot for card #5007 c10 and c11 (merged from #5008).
It asks how a developer stops at a breakpoint in code that runs while Jet
builds, and what a stop shows.

- c10: a breakpoint in a prep block, a prep fn recipe, a marker body or fn
  build pauses during `jet build`, with locals, call stack and the step
  count shown.
- c11: behavior, step budget and results are the same with or without the
  debugger, proved by a focused test.

This is the second pass. The first pass repaired the draft with the step
limit measured as processor time. Main ruled that ratified D-PREP-FUEL1
binds: a step is a deterministic count, never time. The recommendation is
now option A: the in-session debugging of the first repair, with counted
steps. Processor time is kept as option B, which needs D-PREP-FUEL1
amended. That the code measures time today is a defect against the
ratified decision. Main is adding it to the owning card as a criterion, so
this ballot does not decide it.

All paths are relative to the repository root, and line numbers are from
the dev worktree on the review date. Where a claim was probed, the probe is
named. Where it was not, it says unverified and what a failure would cost.

## Strength 1: build-time code stops under the console and adapter people already use

Peers' best case. Gradle debugs build logic inside the running build.
Setting `org.gradle.debug` to true makes the build wait for a remote
debugger on port 5005. Breakpoints in buildSrc, plugins and Kotlin build
scripts then stop there (docs.gradle.org, user guide, "Troubleshooting
builds", section "Debugging build logic"). That is the shape of A.

Attack. The draft said "the child is spawned traced" and that the session
maps each batch's addresses to its debug facts as the batch loads. Four
facts make this harder than stated.

- `jet debug` runs the build as a separate process,
  `jet build -O0 --dead-code=keep`, before the session starts
  (`Compiler/JetCLI/Source/CLI/Debug.jet:333-350`,
  `Compiler/JetDebug/Source/Native.jet:6570-6581`).
- Batches carry no line table. The compiler only lists each batch
  function's address range, declaration line and file, so that `perf` can
  name it (`Compiler/JetEval/Source/IR/Native.jet:144-151, 184-192,
  585-613`).
- Marker bodies never reach a child (probe (a) below). A tracer of the
  children can't stop in them.
- The debugger has no waiting breakpoints (probe (c) below).

Verdict: holds within limits, as repaired. The ballot now does four things:

- it runs the build in the session's own process;
- it extends the existing batch listing with line tables and local places;
- it places a waiting breakpoint when its batch loads;
- it stops marker bodies with a hook in the compiler's template engine.

All four are unbuilt and named as losses.

## Strength 2: debugged and plain builds have the same budget and results (c11)

Peers' best case. Several peers count work instead of timing it:

- Zig's `@setEvalBranchQuota` raises a limit counted in backward branches,
  default 1000 (ziglang.org language reference, master).
- Nim's compile-time VM limit `--maxLoopIterationsVM:N` counts loop
  iterations (nim-lang.org, "Nim Compiler User Guide").
- Wasmtime's `consume_fuel` bounds code "deterministically" by
  instrumenting it to count. Its docs contrast that with the cheaper,
  time-based `epoch_interruption` (docs.wasmtime.dev, `wasmtime::Config`).

A counted budget is the same with or without a debugger by construction.

Attack, first pass. The draft said fuel is "the child's run time minus
paused time". In fact the limit is the wall-clock time the compiler spends
waiting on the child:

- `x64_comptime_receive_bytes` polls with the remaining budget and
  subtracts the elapsed monotonic time
  (`Compiler/JetBackend/Source/Image/SessionHost.jet:285-309, 391-393`);
- the child is killed and the call fails with E0952 when the poll times
  out (`Native.jet:941-953, 1644-1649`).

A pause made by an outside tracer would keep that clock running.

Attack, second pass. The first repair counted the child's processor time.
That is not deterministic either: it varies with the machine, the cache and
the kernel. It therefore breaks D-PREP-FUEL1, ratified 2026-10-02: "What a
step is: one evaluated expression or statement in the prep evaluator. The
count is deterministic, never wall time, so every machine stops at the same
point."

Verdict: holds, as now repaired. A counts steps. A paused call runs no
code and spends none, the count is the same on every machine, and c11's
test can compare exact counts. The cost is the counter (probe (b)).
D-PREP-FUEL1 also says "a new compiler version may count a little
differently". So the unit may follow the compiled code: one statement
counted per straight-line block, added as the block starts.

## Strength 3: a stop shows the step count (c10)

Peers' best case. Wasmtime and Zig can show an exact remaining count
because they count. Gradle and the native debuggers show none.

Attack. The draft's stop line was `fuel: 2,400 of 10,000,000 steps used`.
D-PREP-FUEL1 records that the owner rejected "fuel" as theatrical; the
user-facing name is "step limit", as E0952's text uses (`Native.jet:1646`).
Under time measurement the number was time multiplied by 2000.

Verdict: holds, as repaired. The stop line reads `steps: 1,204 of
10,000,000`, an exact count, and the prose says "step limit" throughout.

## Strength 4: `jet build` stays non-interactive (against option C)

Peers' best case. Gradle puts its debug switch on the build command itself
(`./gradlew help -Dorg.gradle.debug=true`), so a build command that waits
for a person has precedent.

Attack. c10 says "pauses during `jet build`". Each option reads that
differently:

| Option | Reading of "during `jet build`" |
| --- | --- |
| A, B | The pause happens during the build `jet debug --build` runs: the same compile, flags and build-time calls, started from `jet debug` rather than typed as `jet build`. |
| C | Literal: the `jet build` command itself pauses. |
| D | The pause happens during a build the editor starts through the debug adapter. |
| E | The pause happens during a build, but in an interpreter, not in the code a plain build runs. |
| F | Nothing pauses during any build: one call is rerun after it. |

Verdict: holds; A is picked on merit. A and C run the same compile and
the same build-time code, so they stop in the same places with the same
counts. They differ only in which command starts the build.

`jet debug` already has `--break`, `--do`, `--stdin`, `--dap`,
`--record` and `--replay` (`Debug.jet:13-56`). C would duplicate or
forward each of them into `jet build`, and `jet build` is the command
scripts and CI call.

Batch code is O0 at every optimization level, so the build `jet debug
--build` runs is the build `jet build` runs (`Native.jet:1-2, 547`). The
owner should confirm that A's reading meets c10's wording; C is the
literal reading if not.

## Strength 5: one engine (against option E)

Peers' best case. rustc evaluates constants in an in-process MIR
interpreter, a second engine beside its code generator, as the comptime
child design review records
(`Docs/Audits/2026-11-23-comptime-child-design.md:36`). An interpreter can
count steps exactly and step by expression.

Attack. None found against A. The same review rejected a second engine
for drift and lost speed. E would also debug different code from the code
the real build runs.

Verdict: holds.

## Probes

### (a) Marker bodies: traced to their call site

Marker bodies do not run on the compile-time item path, and they never
reach a child process. Registration expands them inside the compiler.

- `sema_user_rule_collect_module` finds each marker on a function and
  expands its body (`Compiler/JetSema/Source/Sema/Registration/UserTemplates.jet:702-716`).
  It does the same on a type (771-781) and a text block (752-765). Each of
  these calls `sema_derive_expand_rule_body`.
- That function expands the body through the template engine derives use
  (`Compiler/JetSema/Source/Sema/Registration/Derives.jet:2100-2129`). The
  file header says package templates "expand through the same typed
  template engine as the Prelude providers" (`UserTemplates.jet:1-5`).
- The engine's only loop is `#loop`, over a known list
  (`Derives.jet:1858-1874`). `while`, `for` and assignment are refused
  with E0956 (`Derives.jet:1970-1971`). Values are read by
  `sema_derive_static_value`, and calls are spliced into generated code,
  not run (`Derives.jet:1189-1195`).

So a marker body has no step limit and needs none. Stopping in one needs a
hook in the template engine, in the compiler's own process. That is
possible under A only because A runs the build in the session.

Result: the first-pass claim that marker bodies run as compile-time items
was wrong. The ballot now says where they run.

### (b) How the count would be taken, and its cost

Nothing counts steps in compile-time machine code today. Three pieces of
existing code show where a count would go.

- Every loop turn and every recursive entry in compiled Jet already starts
  with a turn poll (`Compiler/JetIR/Source/Build.jet:3824-3858`). It is
  emitted when the IR program has stop points, which is the default
  (`Build.jet:605-608`). Only the runtime library and owned-main programs
  turn stop points off (`BuildLibrary.jet:272`, `EntryOwned.jet:427`).
  The batch program is built by `ir_program_new` with no override
  (`Compiler/JetEval/Source/IR/Values.jet:58`). Under the fast O0
  generator, the turn poll's hot path is a load of one runtime word, a
  test and a branch (`Compiler/JetIR/Source/X64Patch.jet:2867-2978`).
- Jet's wasm plugin translator already counts fuel at each function entry
  and loop head (`Compiler/JetPlugin/Source/Plugin/Translate.jet:9-10,
  416-431`). It loads a word, subtracts one and stores it, with a slow path
  every 4096 units (`Compiler/JetRuntime/Source/PluginHost.jet:115-119,
  294-312`). That is the design A would reuse for batches.
- The runtime still exports the counted API of the old evaluator:
  `jet_rt_comptime_begin`, `_step`, `_work` and `jet_rt_iter_charge`
  (`Compiler/JetRuntime/Source/Comptime.jet:59-98`, `IterCharge.jet`).
  No compiler code calls `jet_rt_comptime_begin` outside tests, so these
  are dormant in the native child.

Measured cost, on the dev chain's compiler (a0.0.46-dev). The probe ran
`jet check` on three files, seven interleaved rounds each, and took the
minimum wall time. The files and the table are in the orchestrator's
scratch probe directory.

- The base file runs 400,000,000 build-time loop turns of `sum += i`:
  1.41 ns per turn.
- The counted file adds `left -= 1; if left < 0 -> return -1` to each
  turn: 2.02 ns per turn.
- The counter's work therefore cost 0.61 ns per turn, 43% of this
  near-empty loop.

That counter is written in Jet, with an exact-Int overflow check and O0
stack traffic. A dedicated decrement and branch should cost no more
[INFERENCE: not built]. The host's load average was about 40 on 32
threads, so the absolute numbers are noisy, but the minimums were stable
across rounds.

A recursive call's entry carries the same poll, so its cost should be
the same [INFERENCE: not measured]. Counting statements per block costs
one update per straight-line block, so a loop body with branches pays more
than one per turn [INFERENCE: not measured].

### (c) Waiting breakpoints in `jet debug`

There is no support for a breakpoint that waits for code to load.

- `break_at` resolves a file and line against the loaded image's line
  table only (`Compiler/JetDebug/Source/Native.jet:708-746`).
- `run_to` reports a line with no code as `No code on` and leaves it out
  (`Native.jet:2671-2687`).
- A session follows the threads of one traced process
  (`Native.jet:2148-2199`; `Compiler/JetBackend/Source/Image/Loader.jet:2500-2519`,
  PTRACE_O_TRACECLONE only). It does not follow forked children.

Two pieces of nearby infrastructure exist:

- `fix` places newly compiled functions, with their debug rows, into a
  running process (`Compiler/JetDebug/Source/NativeFix.jet:1-24`).
- `x64_trace_command` follows forks and execs for `jet perf syscalls`
  (`Loader.jet:2521-2545`).

Neither makes breakpoints wait. Waiting breakpoints are new work, named in
the ballot.

## Missing options added

- B, processor-time steps: kept from the first repair as an alternative.
  It needs D-PREP-FUEL1 amended.
- F, rerun one build-time call outside the build (`jet debug --prep
  <place>`). This is how Rust's build scripts are debugged; Cargo builds
  them as programs (doc.rust-lang.org, Cargo book, "Build Scripts"). It is
  also how Roslyn's cookbook tests a generator, driving it from a unit test
  with `CSharpGeneratorDriver` (dotnet/roslyn,
  `docs/features/source-generators.cookbook.md`). It does not pause during
  a build, and a marker body has no call to rerun.

## Other draft claims

- "A breakpoint in a prep block or in `fn build` never fires." Holds,
  refined.
  - A prep block's line has no code in the program, so it is refused as
    `No code on` (`JetDebug/Source/Native.jet:2683`).
  - A prep fn's line does have code, because `--dead-code=keep` keeps every
    function of the package (`Native.jet:6570-6576`). A breakpoint there is
    set in the finished program, but the build already ran that code before
    the session started.
- "`fn build` runs in a second child." Holds
  (`JetEval/Source/IR/Native.jet:17-19, 111-118`;
  `Compiler/JetDriver/Source/Driver/ComptimeIR.jet:98-115`). Prep items run
  through `Compiler/JetDriver/Source/Driver/Pipeline.jet:3284-3300`, where
  `#StepLimit(n)` sets the call's limit (line 3298).
- "`--attach`, `--dap`, `--record` and `--replay` select how it runs."
  Holds (`Debug.jet:13-56`), though the list leaves out `--break`, `--do`,
  `--stdin` and crash dumps.
- Comparisons:
  - Gradle's `--debug-jvm` debugs test and run JVMs, not build logic, so it
    was replaced by `org.gradle.debug`, which is sourced.
  - MSBuild's `MSBUILDDEBUGONSTART` and C#'s `IsRoslynComponent` had no
    source and were dropped.
  - "Procedural macros can't be stepped" overstated the case. The Rust
    Reference says only that they run during compilation with the
    compiler's resources, and that an endless loop hangs the compiler.
  - Wasmtime was added for counted fuel and its cost.
  - Lean and MetaOCaml were left out. I found no sourced build-time
    debugger for Lean metaprograms, and MetaOCaml stages code at run time.

## Verdict

Holds within limits. A is now in-session debugging with counted steps:
one console, one adapter and one engine. It keeps the ratified
D-PREP-FUEL1, a pause spends no steps by construction, and `jet build`
stays non-interactive.

The draft could not have been posted as written. Its problems were:

- a time budget that a pause would spend;
- the retired word "fuel";
- breakpoints that would be refused;
- the wrong account of where marker bodies run;
- missing options;
- losses and why-nots the validator rejects.

The repaired ballot fixes each and validates.

## Open gaps (unverified, with the cost if they fail)

1. A dedicated counter's cost on real build-time code is unmeasured. Only
   a Jet-written stand-in on a near-empty loop was measured: an added
   0.61 ns, 43%.
   - If it fails: builds with heavy prep loops slow by up to that much.
   - Mitigation: build the counter behind a switch and time the
     repository's builds before turning it on.
2. Nobody knows how much existing prep work exceeds ten million counted
   steps. Today's budget is five seconds, about 3.5 billion turns of the
   probe loop.
   - If it fails: those bindings stop with E0952 at cutover until they
     carry `#StepLimit(n)`.
   - Mitigation: a count-only pass over every build lists them first.
3. The template-engine stop hook for marker bodies is unbuilt.
   - If it fails: c10's marker-body case has no stop.
   - Mitigation: the build runs in the session, so the hook is a call
     with no tracing, and the c10 case must include a marker body.
4. Running the build in the session's process is unbuilt. `jet debug`
   runs it as a subprocess today.
   - If it fails: the session must trace a grandchild and cannot hook
     marker bodies.
   - Mitigation: none short of that work.
5. That adding line tables leaves batch machine code byte-identical is
   unprobed.
   - If it fails: a debugged build runs different code, and c11 fails.
   - Mitigation: a case compares batch code with and without line tables.
6. Calls the debugger makes at a stop must not spend steps. This is
   design only.
   - If it fails: inspecting a value changes the count, and c11 fails.
   - Mitigation: the session saves and restores the counter around each
     call it injects.
7. Option B's reading of the child's processor clock from the compiler is
   unprobed. It matters only if the owner picks B.
8. c10 says "during `jet build`", while A pauses during the build `jet
   debug --build` runs. The owner decides; C is the literal reading.

## Finding dispositions

<!-- audit-dispositions:v1 -->
|finding|disposition|target or reason|
|---|---|---|
|S1 build-time code stops under existing consoles|card|#5007 (D-DEBUG-PREP1=A build criteria c10, c11)|
|S2 same budget and results when debugged|card|#5007 (c11)|
|S3 a stop shows the step count|card|#6772 (c15, counted E0952 steps)|
|S4 jet build stays non-interactive|card|#5007 (c10)|
|S5 one engine|card|#5007 (c10)|
|Open gaps|card|#5007 and #6772 (each gap is in the build criteria's proofs)|
<!-- /audit-dispositions -->
