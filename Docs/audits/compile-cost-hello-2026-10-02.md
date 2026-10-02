# Compile cost of hello world: Jet against C, Rust, Go, Zig and Odin (2026-10-02)

Dated measurements of what it costs to compile and run a one-line program with
each Jet compiler and with peer toolchains, where the time and memory go inside
the Jet-written compiler, and the root causes. This is evidence, not work
state: the plan and acceptance criteria live on the Tower card that cites this
audit.

## Setup

- Machine: 32 cores, 61 GB RAM, Linux 7.0.11 (CachyOS), x86-64.
- Program: print `hello` once, written idiomatically in each language.
- Harness: `Tools/perf/hello-compile/bench.py` (wall time; peak resident
  memory summed over the compiler's process group, sampled every 50 ms; binary
  size as built and after `strip`; best of five runs of the binary).
  `Tools/perf/hello-compile/measure-jetc.sh` measures the Jet-written compiler
  (`jetc0`) with resident memory every second and a CPU profile every 10 s.
- "Cold" clears the toolchain's own build cache where it has one. For
  `jet build` the prebuilt runtime library was already in the store, so its
  cold numbers exclude building the runtime itself.
- Toolchains: gcc 15.3, clang 21.1, rustc 1.97.1, Go 1.26.7, Zig 0.16.0,
  Odin dev-2026-05, Jet night12 release build of the Rust reference compiler,
  and `jetc0` from the stage-one loop (built 2026-10-02 15:15 from
  `Compiler/`).

## Results

| Toolchain | Compile, cold | Compile, warm | Peak memory | Binary | Stripped | Run |
|---|---:|---:|---:|---:|---:|---:|
| Assembly, no libc (`hello.S`, gcc -nostdlib -static) | 0.10 s | 0.03 s | — | 9.0 KB | 8.6 KB | 0.15 ms |
| Assembly, minimal ELF (`-s -n`, no build-id or relro) | 0.03 s | 0.03 s | — | 744 B | 744 B | 0.15 ms |
| C, gcc -O2 | 0.12 s | 0.05 s | 35 MB | 16 KB | 14 KB | 0.36 ms |
| C, clang -O2 | 0.32 s | 0.06 s | 157 MB | 16 KB | 14 KB | 0.36 ms |
| Rust, rustc -O | 0.48 s | 0.12 s | 166 MB | 531 KB | 394 KB | 0.64 ms |
| Rust, rustc (debug) | 0.10 s | 0.10 s | 164 MB | 527 KB | 390 KB | 0.50 ms |
| Go, go build | 2.12 s | 0.12 s | 372 MB | 2.4 MB | 1.6 MB | 0.93 ms |
| Odin, -o:speed | 1.59 s | 1.39 s | 470 MB | 200 KB | 191 KB | 0.38 ms |
| Zig, ReleaseSmall | 2.41 s | 1.35 s | 282 MB | 133 KB | 133 KB | 0.33 ms |
| Zig, ReleaseFast | 10.72 s | 8.75 s | 433 MB | 3.7 MB | 530 KB | 0.28 ms |
| Jet, `jet build` (Rust reference, default) | 7.03 s | 1.33 s | 179 MB | 58.8 MB | 45.0 MB | 7.87 ms |
| Jet, `jet build --release` (Rust reference) | 41.21 s | 1.51 s | 2.37 GB | 575 KB | 575 KB | 0.55 ms |
| Jet, `jetc0` (Jet-written compiler) | 64.6 s, then 19 s rustc | — | 11.6 GB | 17 MB (debug) | — | — |

Jet loses every column to every peer except release-mode run time, where it
sits between Rust and Go. The default build's 45 MB stripped binary and 7.9 ms
start-up are the worst results in the table by more than an order of
magnitude.

The two assembly rows are the floor, not a peer. `Tools/perf/hello-compile/hello.S`
makes two system calls (`write`, then `exit`) with no C library. Its 0.15 ms
run time is the cost of starting a process on this machine, measured the same
way as every other row, so no language can run hello faster. Its 744-byte
file is mostly ELF headers; a hand-packed ELF is about 150 bytes, which is the
practical size limit. The assembly rows were measured later the same day; the
sampler misses processes shorter than 50 ms, so they show no peak memory.

## Where `jetc0`'s time and memory go

Resident memory and CPU profile for one `jetc0` compile of hello:

| Window | Resident memory | Dominant work |
|---|---|---|
| 0–18 s | 0.8 GB → 11.6 GB | Restoring the embedded compiler image: the 178 MB image of the compiler's own MIR is decoded into about 11 GB of owned objects, and the whole archive is SHA-256 checked (7% of early samples). |
| 18–64 s | flat at 11.6 GB | Checking, lowering and emitting hello together with the Prelude and Core it reaches. Samples: `JetInt::clone` 20–37%, `jet_int_release` 3–16%, malloc and free about 25%, `MIRType::clone` 5–6%. |
| end | — | Writing 6.3 MB of Rust: the runtime and CoreLib prefix, identical for every program, plus 19 lines for `run`. |
| +19 s | — | rustc building that output (the runtime crate is shared and cached). |

The stage-one self-compile (`jetc0` over all of `Compiler/`) shows the same
baseline and then the checker's copying: 12.2 GB at 60 s, 26.7 GB at 181 s,
with 22% of samples spent freeing whole AST expression trees. It needs about
27 GB and was killed three times at a 20 GB cap.

## Root causes

1. **Whole-image restore for header facts.** Runner start-up
   (`Compiler/Bootstrap/Tests.rs`, runner mode), the compile lease
   (`Compiler/Bootstrap/Host/Native.rs`) and native emission
   (`Compiler/Bootstrap/Runner.rs`) all call
   `__jet_bootstrap_compiler_image()`, which decodes the full program and
   source program (`Compiler/Bootstrap/Host/CompilerImage.rs`,
   `restore_compiler_image`). Most callers read only the artifact identity,
   entry function and source-authority digest. Cost: about 18 s and 11 GB on
   every compile.
2. **Every `Int` operation is an out-of-line call.** `JetInt`
   (`crates/jet-foundation/src/Numeric.rs`) has an inline small-integer
   representation, but its `Clone`, `Drop` and `compare` are not inlined
   across crates, so each copy of an index or counter in generated code is a
   function call with a pointer test. The Jet compiler uses `Int` for every
   index and position, so this dominates the profile.
3. **Copy-on-every-read code generation.** Value semantics lower to owned
   copies wherever the compiler cannot prove a borrow: module constants,
   indexed reads of mutable locals, receivers of field projections, whole-
   program passes that rebuild the AST. The profile shows it as `MIRType`,
   `Expr` and `String` clones and the malloc and free that go with them.
4. **A fixed 6.3 MB runtime prefix in every program.** The Rust emitter
   prepends the entire runtime and CoreLib (about 2.1 MB of it Unicode tables
   written as Rust literals) whether or not the program uses them. In debug
   builds nothing removes the unused code, hence the 58.8 MB binary and the
   7.9 ms start-up. See `~/.cache/jet-luna/sol/prep/Runtime-Prefix-Plan.md`
   for the measured breakdown.
5. **Release builds compile the runtime with full optimization on first use**
   (41 s, 2.4 GB), because the runtime crate is built per profile rather than
   shipped prebuilt with the toolchain as D-EXEC1 requires.

## What beating the peers requires

The performance gate in `AGENTS.md` requires Jet to beat every matched peer on
every required cell. Build time is a required foundation cell. For hello, that
means at least:

- compile under 0.10 s warm and 0.12 s cold (gcc), with peak memory under
  35 MB;
- a release binary under 14 KB stripped and a default binary under 133 KB
  (Zig ReleaseSmall), with start-up under 0.28 ms (Zig ReleaseFast);
- the same cells for the Jet-written compiler, not only the Rust reference.

The fixes in order of effect: restore only the image header (cause 1); inline
the `JetInt` small-integer paths and use native integers where the checker
proves the range (cause 2); borrow instead of copy for inspection-only reads
across the remaining cases (cause 3); prune the runtime prefix to what a
program reaches and move tables to binary data (cause 4); ship the shared
runtime prebuilt per D-EXEC1 and generate code with the Jet backend (cause 5).
Re-run `Tools/perf/hello-compile/bench.py` after each and record the new
numbers as a dated receipt on the card.
