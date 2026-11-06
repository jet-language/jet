# A portable Jet target better than WebAssembly, and what it may learn about the host

Date: 2026-10-30. Card #6345, criteria 1 to 3. Author: MemDesign-2 lane
(design only). Probes live in `$JET_SCRATCH_ROOT/lanes/MemDesign-2/probes/`
(`g/` for facts, `w/` for targets, `perf/` for the timing cell) and ran with
`$JET_SCRATCH_ROOT/discovery/base/latest/jet` (commit 4e514f96e1) against a
`git archive` export of dev 4e514f96e1, at o0, o1 and o2 through
`probes/tiers.sh`. Unless a row says otherwise, all three levels gave the same
result.

The owner asked for a portable target that replaces WebAssembly without being
a lowest common denominator: portable code should reach tensor cores, wide
vector units and GPUs through some metadata the sandbox host exposes. Most of
that is already ratified. This note shows what is ratified, what runs, how the
pieces fit, where the security boundary sits, and what the paired cells
measure. It raises two owner choices, filed as `D-HOST-FACTS-VIEW1` and
`D-PORTABLE-WEB1`.

## Ratified and open decisions

Tower decisions:

| Decision | Status | What it settles for this card |
|---|---|---|
| D-PORTABLE1 | ratified A, 2026-10-02 | The portable file holds verified Jet code; the host checks it, grants each power and compiles it locally. Technical: a "versioned portable encoding of D-TIER-FORM1 MIR; host verifier; brokered unforgeable handles; interpreter then local native cache". Browsers need a separate adapter. |
| D-PORTABLE2 | ratified A, 2026-10-07 | The host checks the final machine code against the wall rules, so a compiler bug "can then make the app wrong, but cannot let it out". |
| D-HOST-CAPS1 | ratified A, 2026-10-07 | Typed hardware descriptions read through `$build.hardware`; outputs list `requires:` and `uses:`; packages add hardware kinds with `satisfies` and `coarse` methods; "host observations, grants and kind/probe package identities determine launch admission/cache identity, never a producer's asserted facts". |
| D-CPU-DISPATCH1 | ratified A, 2026-10-02 | `#Multiversion(levels...)` builds one copy per CPU level; `$build.cpu` describes the level inside each copy. |
| D-LANES1 | ratified A, 2026-10-02 | `Lanes<T, N>` exact widths and `Lanes<T>` native width; "Fixed reduction order is part of the Core definition". |
| D-CORE-BOUNDARY1 | ratified A, 2026-10-05 | Chip instructions are library entries with a plain Jet body. |
| D-CORE-FEATURE-FLOW1 | ratified A, 2026-10-05 | Chip features are automatic by default with explicit expert control. |
| D-ACCEL-LIB1 | ratified A, 2026-10-28 | Accelerators are libraries over a public kernel IR, checked against the CPU result. |
| D-ADAPTIVE-TIER1 | ratified B, 2026-10-05 | Recorded runs plus an opt-in in-program optimizer, refused for browsers and WebAssembly. |
| D-EXEC3 | ratified A, 2026-10-07 | Build levels share speed-ups and are checked against each other. |
| D-EFFTYPE-KERNEL1 | ratified A, 2026-10-30 | The compiler builds in machine powers; Core declares the rest. |
| D-EFFTYPE-TARGET1 | ratified A, 2026-10-30 | A build's effect limit is read from its target profile. |
| D-CORE-PLUGINLIMITS1 | ratified A, 2026-09-28 | A host sets a sandbox component's fuel, memory and time limits. |
| D-WASISRV1 | ratified A, 2026-08-12 | wasm32-wasip2 is a full server target. |
| D-LEARN-WEB1 | ratified A, 2026-10-07 | Jet's checker and runner compile to WebAssembly and run in a static page. |
| D-TARGET-ORDER1 | ratified A, 2026-10-28 | Targets return in the order wasm32-wasip2, then aarch64, then embedded. |
| D-AUTOPIN1 | ratified A, 2026-09-28 | Every automatic choice can be seen and locked. |
| D-COMPTIME-WORD1 | ratified A, 2026-10-17 | `comptime` forces compile-time work; with D-PREP-SURFACE2=A, `prep` is retired. |
| D-MATH-BITS1 | open, recommendation A | `core.math` returns the same correctly rounded bits everywhere. |

Decisions kept in `Docs/spec/syntax-decisions.md`: D-TIER-FORM1=A (`:9735`),
D-FREESTAND-FACTS1=A (`:9098`), D-DEP-WASM1=A (`:8843`), D-SIMD3=B (`:3762`),
D-EXEC2=A, JIT retired (`:3773`), D-COMPUTE-BACKEND1=D (`:6953-6959`). The
glossary row for `comptime` records that `prep` is retired
(`syntax-decisions.md:35`).

## What runs today

| Probe | Program | Result on o0, o1, o2 |
|---|---|---|
| w/hello | `jet build --target=portable`, `web`, `wasm32-wasip2`, `wasm32-unknown-unknown`, `sandbox` | E2104 "is not built yet" for each. |
| h6b | `print($build.os)`, `print($build.profile)` | `Linux`, `dev`. |
| h1, h2, h6 | `$build.cpu`, `$build.hardware`, `$build.arch` | E0107 "Nothing named `$build` exists here". |
| h3 | `#Multiversion` | E0927, unknown marker. |
| h4 | `Lanes<F32, 4>` | E0003. |
| h8 | `F32x4(1.0, 2.0, 3.0, 4.0)` added to itself | `F32x4 { lanes: [2.0, 4.0, 6.0, 8.0] }`. |

No `.jetapp` code exists in `Compiler/` or `Core/` (a search for `jetapp`
finds nothing). The target table knows the wasm triples
(`Compiler/JetFoundation/Source/Target/TargetMachine.jet:573`, `:594`, `:615`)
and maps `wasm32-unknown-unknown` to the browser profile (`:527`).
`Compiler/JetWasm/Source/Wasm/` holds a decoder, validator, text form and
component model (`Bytes.jet`, `Component.jet`, `Decode.jet`, `Module.jet`,
`Opcodes.jet`, `Text.jet`, `Validate.jet`): Jet reads WebAssembly, it does not
yet write it.

One naming hazard: `core.hardware` already exists as the embedded-board
capability `Target.Hardware` (`TargetMachine.jet:666`, `:714`), available only
when the target has register blocks or interrupts (`:827`). D-HOST-CAPS1's
Core `Hardware` description is a different thing with the same word. The
implementing card should keep them apart.

## What WebAssembly gives and lacks

- **Fixed-width vectors.** WebAssembly SIMD has one vector type, the 128-bit
  `v128` (WebAssembly Core Specification 2.0, vector types). A machine with
  AVX-512 or SVE gets four F32 lanes at most.
- **Relaxed SIMD trades determinism for speed.** "This proposal introduces
  non-deterministic instructions"; the same instruction may give different
  results on different engines (WebAssembly relaxed-simd Overview,
  "Consistency").
- **Threads** need shared memory imported or defined by the host
  (WebAssembly threads proposal Overview, "Shared Linear Memory"); creating
  threads is left to the embedder.
- **No GPU outside the browser.** In browsers, GPU access is WebGPU through
  JavaScript. For WASI, WebGPU is a Phase 2 proposal, not a standard
  interface (WebAssembly/WASI `docs/Proposals.md:33-43`).
- **Walls from the engine.** The engine validates and bounds-checks; the
  compiler that made the module is untrusted.

## What peers do

- **Mojo.** A precompiled `.mojoc` package "contains non-elaborated code, so
  you can share it across systems"; it becomes machine code only when imported
  into a program built with `mojo build`, and it "is tied to the exact version
  of the compiler that produced it"
  (`Mojo/docs/site/manual/packages.md:150-157`, corpus commit 85356f6562ed).
  It is a portable compile input, not a sandbox format.
- **SPIR-V.** A module declares every capability it uses with `OpCapability`;
  "A client API is allowed to reject modules declaring capabilities it does
  not support" (Khronos SPIR-V specification, section 2.1). This is the model
  for `requires:`.
- **JVM and .NET tiered compilation.** .NET starts on quick JIT or
  precompiled ReadyToRun code and recompiles hot methods with full
  optimization (learn.microsoft.com, "Tiered compilation"). Jet's portable
  host does the same in two steps: interpreter, then a cached native build
  (D-PORTABLE1 technical).
- **Capability APIs.** Java's Vector API has `FloatVector.SPECIES_PREFERRED`
  and promises "graceful degradation" where vectors are missing (JEP 529).
  .NET has `Vector.IsHardwareAccelerated`. Both are open to any code.
- **eBPF.** The kernel's verifier checks a program in two steps, a control
  flow check and then a walk of every path, before it may load
  (docs.kernel.org, "eBPF verifier"). Jet's host verifier plus machine-code
  checker is the same idea for whole applications.
- **WebGPU.** "A user agent must not reveal more than 32 distinguishable
  configurations or buckets"; limits may be binned; adapter identifiers stay
  broad unless the user consents; timing fingerprinting is
  "low-signal and relatively intractable" to prevent (W3C WebGPU editor's
  draft of 27 October 2026, sections 2.2, 2.2.1, 2.2.3 and 2.2.6).

## The form Jet ships

The owner's three candidates were an IR, a bytecode, or ahead-of-time code per
host. Ratified law already picks a combination, with one encoding:

1. **The file is IR.** A `.jetapp` is the versioned, content-hashed encoding of
   Jet's mid-level IR (D-PORTABLE1, D-TIER-FORM1). There is no separate
   bytecode; the MIR encoding plays that role, which keeps I8.
2. **GPU parts travel inside it** as the public kernel IR (D-ACCEL-LIB1), so
   the host's device library lowers them for the GPU it has.
3. **The host compiles it ahead of time, locally.** The host verifies the file,
   starts it in the interpreter, and caches a native build compiled for this
   machine (D-PORTABLE1). Before native code runs, the machine-code checker
   proves it stays inside the walls (D-PORTABLE2).

Local compilation is what beats WebAssembly. At local compile time the host
knows the real machine, so `Lanes<T>` lowers to the widest register the CPU has
(D-LANES1), Core's chip copies are built for this CPU only rather than for a
list of levels (D-CPU-DISPATCH1), and kernels lower to the GPU that is present.
Every one of those paths has a plain Jet meaning tested equal
(D-CORE-BOUNDARY1), and lane reductions have a fixed order (D-LANES1). So
unlike relaxed SIMD, the faster path never changes the answer, and I9 holds
across the interpreter, the cached build and every machine.

## Hardware facts as typed facts

Under D-HOST-CAPS1 a program reads hardware facts the same way on every target.

- **Admission.** `requires:` lists what the app cannot run without. The host
  checks it at install and at each launch, like SPIR-V's `OpCapability`, and
  refuses with the missing fact named.
- **Specialization.** `uses:` lists what the app can use when present. The app
  branches with `comptime if $build.hardware.has(…)`; the branch folds at
  local compile, so the built code contains no run-time query.
- **Cache identity.** The answers join the cache key. When the hardware
  changes, the host recompiles.
- **Coarse facts.** Facts are lower bounds; "a coarse projection never
  overstates capability" (D-HOST-CAPS1 explainer).

The ratified examples in D-HOST-CAPS1 and D-CPU-DISPATCH1 spell the branch
`prep if`, which is retired (`syntax-decisions.md:35`); the current spelling is
`comptime if`, which works today for `$build.os` (probe h6b and
`Examples/features/comptime/fact_plane.jet:3-11`).

## The security boundary

What a portable app can learn about its host, under ratified law plus the
recommended option of `D-HOST-FACTS-VIEW1`:

1. **Declared answers only.** In a portable bundle, code may ask
   `$build.hardware.has(q)` only for a question listed in `uses:`. The host
   answers each from facts it observed and granted, as one bit. Reading a
   hardware value in a portable bundle is a build error.
2. **A budget.** By default an app gets at most five answers, so it can tell
   apart at most 32 kinds of machine, matching WebGPU's limit. The user may
   raise it for one app.
3. **A host may answer no.** Because every chip path has one meaning, a "no"
   costs speed, not correctness.
4. **Package rounding is not trusted.** The package that defines a hardware
   kind also writes its `coarse` method and could return precise values. Under
   option A only one bit per question reaches the app, whatever `coarse` does.
5. **Same answers everywhere.** Under I9 and D-MATH-BITS1=A, results do not
   differ by machine, so arithmetic itself does not reveal the chip. Relaxed
   SIMD does.
6. **Facts never grant powers.** Powers come from the host's grants
   (D-PORTABLE1); a hardware answer cannot open a file or a socket.
7. **Native programs are trusted** and keep every fact.

Limits that remain, under every option:

- **Timing.** An app can time a loop and guess the chip. WebGPU reaches the same
  conclusion (section 2.2.3). Separate processes per app, the side-channel
  hardening D-PORTABLE2 requires when untrusted apps share a process, and
  coarse timers are host policy.
- **Resource failures.** D-HOST-CAPS1 checks free GPU memory before each job.
  A refused job tells the app something about free memory. The implementing
  card should report such refusals in coarse buckets.

## The browser

D-PORTABLE1 says a browser needs a separate adapter. Ballot `D-PORTABLE-WEB1`
asks whether that adapter should be an in-page Jet host that runs the same
`.jetapp`: verified, then translated to WebAssembly in the page with the same
back end as the web export (#5201), with an optional ready-made WebAssembly
copy keyed by the app's hash. D-LEARN-WEB1 already puts Jet's checker in the
page, so the in-page host extends ratified work.

## Paired performance cells

**P1, measured: one loop, native against WebAssembly.** `y = 0.999 * x + y`
over 1,048,576 F32 values, 400 passes, then a Float checksum. Sources:
`probes/perf/saxpy.c` and `probes/perf/saxpy.jet`. Every build printed checksum
629040256. The machine was shared and loaded (load average about 92 on 28
cores), so these are indicative; the table shows the minimum of five runs
unless marked.

| Build | Command | Time (s) |
|---|---|---|
| C native, `-march=native` | gcc 15.3 `-O2 -ffp-contract=off -march=native` | 0.044 |
| C native, baseline | gcc 15.3 `-O2 -ffp-contract=off` | 0.059 |
| C to WebAssembly, SIMD | zig 0.15.2 `cc -target wasm32-wasi -O2 -msimd128`, Wasmtime 48 `run -C cache=n` | 0.122 |
| C to WebAssembly, scalar | the same without `-msimd128` | 0.324 |
| Jet native, o2 | `jet build --optimize=full` | 1.72 (single runs 1.7 to 3.4) |
| Jet native, o1 | `jet build --optimize=basic` | 2.50 |
| Jet native, o0 | `jet build --optimize=none` | 7.08 (one run) |
| Jet portable | `jet build --target=portable` | E2104, not built |

Two findings. First, WebAssembly costs about 2x to 2.8x against native C here,
including Wasmtime's compile at load; that is the gap a locally compiled
`.jetapp` should close. Second, Jet's own native scalar F32 loop is 29x slower
than baseline C at o2. That gap belongs to native code generation (#6205,
#2414) and must close before any portable cell can show Jet beating
WebAssembly.

**P2, specified: the portable file against native and WebAssembly.** The same
workload as a `.jetapp`, run three ways on one machine: Jet native at the same
level, the `.jetapp` after local compile (cold and cached reported
separately), and the C WebAssembly SIMD build above. Gates: the cached
`.jetapp` within 5% of Jet native; the `.jetapp` faster than WebAssembly SIMD
on a CPU wider than 128 bits, through `Lanes<F32>`; equal checksums
everywhere.

**P3, specified: one meaning under specialization.** A matmul app with
`uses: [GPU.feature(.TensorCores)]`, run once with the host answering yes and
once with it answering no. Gate: equal checksums under the default profile,
with both times reported. This cell proves that a "no" answer costs only speed.

## Criteria and cards this note feeds

- #4195 criteria 1 to 3 (portable app, two hosts, hardware requirements) and
  #6126 (per-target facts): `$build.cpu` and `$build.hardware` are E0107 today.
- #1914 and D-TARGET-ORDER1: every wasm target is E2104 today.
- #5201: the web back end that the in-page host would reuse.
- #6205 and #2414: the 29x scalar F32 gap in cell P1.
- The `core.hardware` naming overlap and the resource-failure bucket above.
- The retired `prep if` spelling in the D-HOST-CAPS1 and D-CPU-DISPATCH1
  examples.

## Ballots

- `D-HOST-FACTS-VIEW1`: what a sandboxed app may learn about the machine.
  Options: A, declared yes-or-no questions within a budget of five (recommended);
  B, coarse descriptions of every device, as ratified; C, no facts in portable
  apps.
- `D-PORTABLE-WEB1`: whether the same `.jetapp` also runs in a browser.
  Options: A, an in-page Jet host translates it to WebAssembly (recommended);
  B, a separate web build, as ratified.

Builders and JSON are in `$JET_SCRATCH_ROOT/lanes/MemDesign-2/ballot/`.
