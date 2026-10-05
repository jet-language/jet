# Adaptive runtime specialization: when a JIT-like tier beats AOT, and how Jet gets it with one backend (2026-10-04)

Card #4571. Ballot draft: `~/.cache/jet-dev/ballots/READY/D-ADAPTIVE-TIER1.json`.

Owner questions, verbatim:

> "would it be possible for jit to outperform aot? if we are trying to squeeze
> absolute maximum possible performance out of every possible scenario, is there
> any benefit of keeping a jit in some form, even an evolutionary modern hybrid
> form?"

> "maybe add a point in there about having jit report hot paths so that aot can
> optimize in a prebuilt binary? or maybe aot can have something in runtime that
> serves like a jit optimizer onboard?"

Ruling for this session: the Cranelift `jet-jit` crate is not ported. D-EXEC1=A
retires it, and nothing below revives it. Every mechanism here uses the one
Jet code generator (O0/O1, with LLVM at O2 first) and the one compiled runtime.

## Answer

Yes, a runtime tier can beat ahead-of-time code. The cases are narrower than JIT
marketing suggests. The headline multipliers (PyPy about 4x CPython, TurboFan
4.35x on JetStream) compare a JIT with an *interpreter* of a *dynamically typed*
language. Most of that win comes from discovering types at run time. Jet knows
types, ownership and layouts statically, and it monomorphizes generics and runs
comptime. So that win is already in Jet's AOT code.

What remains, ranked by evidence:

1. **Profile knowledge** (hot paths, branch bias, call targets, loop trip
   counts). This is worth about 2-20% on real programs. AOT can capture most of
   it by recording a run and rebuilding: Go measures 2-14%, Google AutoFDO
   10.5% geomean, and .NET dynamic PGO up to 20%. Post-link layout adds 1-8% on
   large binaries (BOLT, Propeller). This is the owner's "JIT reports hot paths
   so AOT can optimize" idea, and it is mature practice.
2. **Workload drift.** Traffic changes after the build. Only a runtime tier
   follows it without a redeploy. Java's newest design (JEP 544, JDK 28) keeps
   a JIT on top of AOT code for exactly this reason.
3. **Runtime constants.** These are values that become fixed only after startup,
   such as configuration, a route table, a regular expression or a query plan.
   A runtime tier can fold them into code. Examples are V8 Maglev's
   de-facto-constant globals, HotSpot's `static final` folding, PCRE2's regex
   JIT and HyPer's compiled query plans. AOT plus comptime covers only values
   known at build time.
4. **Exact hardware.** Jet's ratified `#Multiversion` (D-CPU-DISPATCH1=A)
   already captures most of this ahead of time.
5. **Dynamic loading, REPL and notebooks.** These are already served by O0's
   in-memory image under D-EXEC1.

AOT wins on startup, memory, predictability, security and platform reach.
JEP 544 reports 65-80% less startup with cached AOT code. Julia 1.9 cut CSV.jl's
time to first execution from 11.66 s to 0.08 s by caching native code. Edge
reports that about 45% of V8 CVEs since 2019 were JIT bugs. iOS, consoles and
smart TVs forbid run-time code generation.

Recommended direction (ballot option B):

- Every release build can consume a recorded hot-path file, which is A.
- Long-running processes may opt into an onboard re-specializer. It is the same
  Jet code generator linked into the binary, and it rebinds hot functions
  through the slot table the backend already designs for `jet dev`.
- The onboard part ships only after a paired cell shows it beating A's prebuilt
  binary.

## 1. Where JITs beat AOT, and by how much

### 1.1 Measured JIT gains (primary sources)

| System | What the JIT adds | Measured effect | Baseline compared | Source |
|---|---|---|---|---|
| V8 TurboFan | Speculative optimizing tier using type and shape feedback | 4.35x on JetStream, >1.5x on Speedometer | Ignition interpreter | v8.dev/blog/maglev (2023-12-05) |
| V8 Sparkplug | Non-optimizing baseline compiler | +45% JetStream and +41% Speedometer over Ignition; +8% and +22% with TurboFan present | Interpreter (+TurboFan) | same |
| V8 Maglev | Mid-tier SSA JIT | Compiles about 10x slower than Sparkplug and 10x faster than TurboFan. Energy -3.5% (JetStream) and -10% (Speedometer) | Three-tier V8 | same |
| V8 jitless | Interpreter only; no executable memory at run time | Speedometer 2.0 about 40% slower; Web Tooling 80% slower; YouTube TV app 6% slower; heap -1.7% median | Default V8 | v8.dev/blog/jitless |
| Edge "Super Duper Secure Mode" | JIT disabled | Speedometer down "as high as 58%". Page-load regressions average about 17%. Most lab tests unchanged | Default Edge | microsoftedge.github.io/edgevr/posts/Super-Duper-Secure-Mode |
| PyPy | Tracing JIT for Python | "about 4 times faster" than CPython 3.11 on average | CPython interpreter | pypy.org (speed.pypy.org) |
| LuaJIT 2 | Trace JIT | Archived per-benchmark ratios from 1.52 (sum-file) to 134.71 (md5) | Standard Lua interpreter `[INFERENCE: baseline label not in the extracted text]` | web.archive.org of luajit.org/performance_x86.html (2020) |
| .NET dynamic PGO | Tier-0 instrumentation feeds tier-1 JIT | "can improve the performance of your apps up to 20%"; on by default in .NET 8 | .NET JIT without dynamic PGO | devblogs.microsoft.com/dotnet/announcing-dotnet-8 |
| .NET ReadyToRun | Prebuilt code that the JIT later replaces | "Ahead-of-time generated code is not as highly optimized as code produced by the JIT", so tiering replaces hot R2R methods. R2R files are 2-3x larger | Tier-1 JIT code | learn.microsoft.com/dotnet/core/deploying/ready-to-run |
| HotSpot + AOT cache (JEP 515) | Training-run profiles loaded at JIT start | Example warm-up 90 ms to 73 ms (19%); cache +250 KB (2.5%) | AOT cache without profiles | openjdk.org/jeps/515 |
| HotSpot + AOT code (JEP 544, JDK 28) | AOT code at start; JIT re-optimizes when the workload changes | Startup -65% to -80% with AOT code (-50% to -70% without) on five framework apps; javac first iteration about -75% | No AOT cache | openjdk.org/jeps/544 |
| GraalVM Native Image PGO | Instrumented training run, then AOT rebuild | Game of Life 1.67 s to 0.97 s (1 iteration) and 24.02 s to 13.25 s (100). Binary 7.9 MB to 6.7 MB. The docs say the gain "is not representative" | Native Image without PGO | github.com/oracle/graal docs/reference-manual/native-image/PGO-Basic-Usage.md |

How to read the table: the large factors (4x, 4.35x, 134x) compare against
interpreters of dynamic languages. Where the baseline is already compiled code,
the gains shrink to 8-22% (Sparkplug next to TurboFan), up to 20% (.NET dynamic
PGO) and 19% warm-up (JEP 515).

JEP 544 makes this point itself. A static compiler's peak "can even be
competitive with HotSpot if the static compiler's optimization work is guided by
accurate profiles gathered during prior runs". Dynamic compilation keeps three
advantages: it is *agile* (it follows workload change), *portable* (it fits the
running CPU) and *compatible* (it handles dynamic loading).

### 1.2 What a runtime tier knows that an AOT compiler does not

| Fact | Who exploits it at run time | How much AOT already has in Jet |
|---|---|---|
| Value types and object shapes | V8, JSC, PyPy, LuaJIT (speculation plus deopt) | All of it. Types and layouts are static (TIR/MIR), and generics are instantiated |
| Branch bias, hot functions, loop trip counts | Every JIT; HotSpot since JDK 1.2 (JEP 515) | None today. A recorded run supplies it (section 2) |
| Indirect call targets (closures, trait objects) | HotSpot and .NET guarded devirtualization | None today. A recorded run supplies it |
| Values fixed after startup | Maglev embeds "de-facto constant" globals and deopts if they change. HotSpot folds `static final` at JIT time; JEP 544 notes AOT code "must generate code that explicitly loads the field" | Comptime covers build-time values only. Run-time constants need a runtime tier (section 3) |
| Exact CPU features | HotSpot (JEP 544 "portable") | `#Multiversion` levels picked at start (D-CPU-DISPATCH1=A; ratified, not built) |
| Workload phase changes | HotSpot deopt/reopt (JEP 544 "agile") | None. Needs a runtime tier or a rebuild |
| Code loaded after start | HotSpot, ORC | O0 in-memory image under D-EXEC1 |

### 1.3 Where AOT wins

| Axis | Evidence |
|---|---|
| Startup and warm-up | JEP 544: the cache cuts startup 50-70%, and 65-80% with AOT code. Julia 1.9 native-code caching took TTFX from 11.66 s to 0.08 s (CSV) and 17.39 s to 0.38 s (DataFrames), at +10-50% precompile time (julialang.org/blog/2023/04/julia-1.9-highlights). Android Baseline Profiles make code paths "about 30%" faster from first launch by AOT-compiling them (developer.android.com/topic/performance/baselineprofiles/overview). |
| Memory and size | .NET Native AOT: "lower app size, memory usage, and startup time" (learn.microsoft.com/aspnet/core/fundamentals/native-aot). R2R files that carry IL plus native code are 2-3x larger. V8 jitless heap is 1.7% smaller. |
| Predictability | JITs tier up by counters: JSC LLInt→Baseline at 500 points, Baseline→DFG at 1000, DFG→FTL at 100000, with exponential back-off on recompiles and thresholds raised as executable memory fills (webkit.org/blog/10308). .NET tier-1 needs 30 calls after a 100 ms quiet period (dotnet/runtime docs/design/features/tiered-compilation.md). Latency until the code is optimized therefore depends on the run. |
| Security | About 45% of V8 CVEs since 2019 were JIT-related. The JIT kept Intel CET disabled, and ACG off because of RWX pages (Edge SDSM post). |
| Platform reach | iOS App Store guideline 2.5.2 forbids apps that "download, install, or execute code". V8: "iOS, smart TVs, game consoles prohibit write access to executable memory" (v8.dev/blog/jitless). macOS Hardened Runtime disallows `MAP_JIT` unless the app holds `com.apple.security.cs.allow-jit` (developer.apple.com). Unity: AOT platforms cannot use `System.Reflection.Emit` (docs.unity3d.com/Manual/scripting-restrictions.html). |
| Maintenance | JEP 410 removed HotSpot's Graal AOT/JIT because it "has seen little use" and "the effort required to maintain it is significant". |

### 1.4 Verdict for Jet

Jet already has a JIT's biggest advantage, because the compiler knows the
types. A runtime tier can still add three things that AOT plus comptime cannot:

- fresh profile data after a workload change;
- folding of run-time constants;
- per-deployment specialization without a redeploy.

The profile half moves into AOT through a recorded run (option A). The other
two need an in-binary tier (option B).

## 2. Profile feedback into AOT

### 2.1 Gains and workflows

| System | Collection | Measured gain | Workflow notes | Source |
|---|---|---|---|---|
| Go PGO (1.21+) | CPU pprof samples from production | "around 2-14%" (Go 1.22 benchmarks) | `default.pgo` in the main package is used automatically (`-pgo=auto`). The guide recommends committing it, since "profiles are an input to the build important for reproducible (and performant!) builds". It is designed for *source stability* (old profiles still match) and *iterative stability* (no oscillation across PGO builds) | go.dev/doc/pgo |
| Google AutoFDO | Hardware-counter sampling in production | Geomean 10.5%. Reaches "85% of the gains of traditional FDO". "Over half of CPU cycles" at Google run in FDO-optimized binaries | Profile "stale by design", with compiler features for stable speedup across releases | CGO 2016, research.google/pubs (AutoFDO) |
| Clang/LLVM instrumented PGO | `-fprofile-generate`, run, `llvm-profdata merge`, `-fprofile-use` | Users report PGO-built Clang/LLVM compile "20%" faster | Instrumentation is detailed and "reproducible", with runtime overhead. Sampling is "very low runtime overhead". `-fprofile-update=atomic` is accurate but has overhead; the default `single` "can be inaccurate under thread contention". Temporal profiling orders functions by first call to cut startup page faults | llvm.org/docs/HowToBuildWithPGO.html; clang.llvm.org/docs/UsersManual.html |
| LLVM CSSPGO | Sampling plus pseudo-probes (`-fpseudo-probe-for-profiling`) and profile inference (`-fsample-profile-use-profi`) | Not re-measured here | Closes part of the gap between sampled and instrumented profiles by anchoring samples to probes | clang UsersManual |
| BOLT (post-link) | Sampled (LBR) profile, binary rewriting | Up to 8.0% on top of PGO function reordering and LTO (Facebook services). Up to 20.4% on GCC/Clang on top of FDO+LTO, and 52.1% without them | A separate step after linking | arXiv:1807.06735 |
| Propeller (relinking) | Sampled profile, basic-block sections, relink | 1.1-8% beyond PGO+ThinLTO. Clang +7%, MySQL +1%. Uses 30-70% less memory than BOLT on large binaries | Fits distributed and cached builds because it relinks instead of rewriting | ASPLOS 2023, research.google/pubs (Propeller) |
| Android ART cloud and baseline profiles | Profiles from the JIT on devices plus developer test runs | About 30% faster code from first launch. R8 profile rewriting adds about 15%; startup profiles add about 15% more startup | The JIT dumps a profile, and the `dex2oat` daemon compiles from it on the device. Play aggregates "Cloud Profiles" for install-time compilation | source.android.com/docs/core/runtime/jit-compiler; developer.android.com baselineprofiles |
| .NET ReadyToRun + tiering | Static R2R code, then the dynamic-PGO JIT | See section 1.1 | AOT for startup and JIT for peak in the same process; this is the hybrid in option B | learn.microsoft.com ready-to-run |
| GraalVM Native Image PGO | `--pgo-instrument`, run, then `--pgo=file.iprof` | Example in section 1.1. PGO builds use `-O3`, and PGO is only in Oracle GraalVM | "PGO enables an AOT compiler to perform similar optimizations as a JIT compiler" | graal docs PGO.md, PGO-Basic-Usage.md |
| HotSpot JEP 515 | Training-run profiles in the AOT cache | 19% faster warm-up in the example | Production still profiles on line: "Profiles cached during training runs do not prevent additional profiling" | openjdk.org/jeps/515 |

### 2.2 What to record

| Record | Feeds | Collection method |
|---|---|---|
| Function entry counts / CPU samples per function | Inlining budget, hot/cold function ordering in the linker, which functions get `#Multiversion` advice | Sampling (default) |
| Branch and edge counts per MIR block | Block layout (hot fall-through), hot/cold splitting, `likely` lowering | Sampling with branch records (LBR/BRBE) where available, or counting |
| Loop trip-count histogram | Unroll and vectorize choices, remainder strategy | Counting |
| Indirect call targets (closures, `dyn` trait calls, function values) | Guarded devirtualization: compare the target, call it directly or inline it, else fall back to the indirect call | Counting (top N per site), or samples with call stacks |
| Allocation site sizes and counts | Capacity presizing and buffer reuse (#4566's capacity work); stack versus heap choices stay proof-driven | Counting |
| Scalar value profile (opt-in): integer, enum and Bool parameters that are nearly always one value | Specialized clone with an entry guard. The clone runs the existing constant folding as if the value were a comptime constant | Counting, explicit |
| CPU level that ran | `jet explain` advice to add a `#Multiversion` level. Never an automatic copy, because D-CPU-DISPATCH1 rejected compiler-chosen clones (its option D) | Startup probe |

Not recorded: string, byte or collection contents, or any value of a type that
is not a scalar. Recording is local and writes a file the user owns. Nothing is
sent anywhere (D-TELEMETRY1=A, `Docs/spec/reference/network-policy.md:3-4`).

### 2.3 Overhead

- Sampling has the lowest overhead. Clang describes it as "very low runtime
  overhead", and AutoFDO is deployed on production machines.
- Counting is exact. Clang calls it "reproducible ... to the extent that the
  code behaves consistently across runs". It adds instrumented-code overhead,
  and atomic counters cost more under thread contention.
- Jet has already ratified this split for its profiler: "Sample by default;
  count explicitly" (D-PROFILE-METHOD1=A), and source locations plus function
  totals (D-PROFILE-DETAIL1=A).
- Counts are taken on MIR blocks, the one lowering (D-TIER-ONEIR1). So a
  counting run at O0 under `jet perf run` gives the same counts as a run of the
  O2 binary. Only sampled *time* is tier-sensitive.

### 2.4 Determinism and reproducible builds

- **The record is an explicit, hashed build input.** Go recommends committing
  it for reproducibility. In Jet it joins the action key like every other input
  (D-BUILDCACHE1=A), and the build receipt names it. The same source, record and
  toolchain give byte-identical binaries, at 1 job and at N jobs (#4128
  criterion 4).
- **Canonical form.** Sort records by function identity and quantize counts
  into log2 buckets relative to the run total. Then a re-recording that differs
  only by noise produces the same bytes and the same binary. This is Go's
  "iterative stability", stated as a format rule.
- **Staleness degrades gracefully.** Records match by the function's stable
  symbol plus its MIR body digest. A function whose body changed gets no
  profile, as with Go's source stability. `jet explain` reports the share of hot
  samples that matched.
- **The record never changes meaning.** It only chooses among lowerings the
  compiler already proves equivalent. It never reaches comptime, `prep if` or
  `$build` facts. A program built with and without a record must produce
  identical goldens. That is an I9-style differential added to the test matrix.
- **No ambient profile.** A build never picks up a record from the environment
  or a cache. It reads one only from the package or from an explicit flag. A
  trusted shared cache keeps working because the record is part of the key
  (D-JPK-REPROCACHE1=D).

## 3. An onboard optimizer inside an AOT binary

### 3.1 Mechanisms in the field

| Mechanism | How the switch happens | Deoptimization | Source |
|---|---|---|---|
| .NET tiering over ReadyToRun | Call counter: 30 calls after a 100 ms quiet timer, then a background tier-1 compile. "The Tier1 version is made active" | Speculation guarded inside tier-1 code; on-stack replacement for loops | dotnet/runtime tiered-compilation.md |
| HotSpot AOT code (JEP 544) | Loads AOT code at start and replaces it with C2 code when profiles disagree. "Shifting from AOT-compiled code to JIT-compiled code is invisible to applications" | Full deopt to the interpreter | openjdk.org/jeps/544 |
| Android ART | Interpreter and JIT with profiling; profile dumped; `dex2oat` recompiles the app AOT while the device is idle | Falls back to the interpreter | source.android.com jit-compiler |
| LLVM ORC / LLJIT / LLLazyJIT | Lazy reexports: a stub calls into the JIT, which compiles the body and updates the stub. Concurrent compilation; removable code | Left to the client | llvm.org/docs/ORCv2.html |
| V8 Maglev / TurboFan, JSC DFG/FTL | Counters and feedback; "OSR exit" when a speculation fails. JSC recompiles after `100 * 2^R` exits | Frame-state maps back to the interpreter or baseline | v8.dev/blog/maglev; webkit.org/blog/10308 |
| PCRE2 JIT | The library compiles one pattern to machine code on request; "yields exactly the same results" | None needed; the interpreter is the fallback | pcre.org pcre2jit |

### 3.2 Costs

- **Code generation in the binary.** The optimizer, the IR it reads and the
  loader ship in every process. R2R shows the size effect of carrying an IR next
  to native code (2-3x). Jet's hello-world size cap is 512,000 bytes
  (`Docs/spec/reference/binary-size.md:20-22`), so the optimizer must be opt-in.
  Today's Jet backend plus optimizer source is about 14,700 lines
  (`Compiler/JetBackend/Source`, `Compiler/JetOptimizer/Source`, counted
  2026-10-04).
- **W^X and code signing.**
  - The Jet loader never maps a page writable and executable at once: it maps
    RW, copies, then `mprotect`s to RX
    (`Docs/research/jet-backend-design-2026-10-01.md:346-352,507-511`).
  - macOS still requires the Hardened Runtime `allow-jit` entitlement with
    `MAP_JIT`.
  - iOS App Store 2.5.2 forbids executing new code. Consoles and smart TVs
    forbid writable executable memory (V8 jitless post).
  - Windows ACG blocks dynamic code (Edge post).
  - WebAssembly cannot generate code inside a module.
  - D-EXEC1's own trade-off already notes that "locked-down platforms may need
    a small evaluator" for O0.
- **Memory.** This covers the code cache, the compiler's working set and the
  retained IR. JSC raises tier-up thresholds as executable memory fills, and a
  Jet design needs the same cap.
- **Startup.** None if the optimizer starts lazily after warm-up. The binary
  starts on its prebuilt code, as R2R and JEP 544 do.
- **CPU.** A background compile thread. Maglev exists because TurboFan's
  compile cost hurt; it cut energy by 3.5-10%. JEP 544 ran its tests on two
  cores because the JIT "is likely to compete with the application for CPU
  time".
- **Security.** It adds an in-process code generator, which accounts for about
  45% of V8 CVEs. Jet's case is narrower: it compiles only the program's own
  checked MIR, never input-supplied code, and its only speculation is entry
  guards.
- **Predictability.** Speed changes during a run, and benchmarks need a
  warm-up phase.

### 3.3 When it pays

Let $g$ be the speedup fraction the re-specialized code adds over the prebuilt
code on the hot functions, $f$ the share of run time in those functions, $L$ the
remaining process lifetime, $s$ the sampling overhead fraction and $C$ the
compile cost. The optimizer pays when

$$g\,f > s \quad\text{and}\quad L > \frac{C}{g f - s}.$$

With a recorded run (A) already in the build, $g$ is small for steady
workloads. It is large in three cases:

- the workload drifted away from the record;
- a function processes data that is fixed after startup, such as routes,
  configuration, a regex or a query plan;
- the deployment CPU has features beyond the declared `#Multiversion` levels
  and the result rule allows them.

So the optimizer is for long-running servers and data engines. It is not for
CLIs, tests or games' frame loops. `[INFERENCE: no Jet measurement exists yet;
the paired cell in section 4.6 decides.]`

## 4. Jet: one backend, one runtime

### 4.1 What already exists

- **One lowering and one runtime.** D-TIER-ONEIR1=A, D-TIER-FORM1=A and
  D-EXEC1=A. Every level calls the same compiled Prelude and Core, so a
  re-specialized function cannot change meaning by calling different runtime
  code.
- **Slot-table recompilation is already designed.**
  `Docs/research/jet-backend-design-2026-10-01.md:513-541`, "Tiered
  recompilation", describes each in-memory image calling module functions
  through a per-function entry slot. "A swap compiles the new body into a fresh
  image ... and rebinds the function's entry: table page read+write, one store,
  back to read-only." "Frames still running the old body finish there; new
  calls take the new body." `x64_rebind_slot` is exercised today on an import
  slot. "Static executables and objects keep direct rel32 calls."
- **One trace artifact.** `jet perf run|test|attach` writes `.jettrace`
  (`jet.trace` v1, self-contained with source identities and source maps;
  D-PERFSESSION1=D, D-ARTIFACT-EXT1=A, `Docs/spec/observability.md:23,36-52`).
  Its profile rows today attribute only to `fn run`; the observe snapshot "has
  no current-function or stack sample" (`observability.md:50-52`).
- **Profiler law.** D-PROFILE-METHOD1=A ("Sample by default; count
  explicitly") and D-PROFILE-DETAIL1=A.
- **CPU copies.** D-CPU-DISPATCH1=A `#Multiversion` with bit-identical copies
  (ratified, not built; cards #4167/#4168).
- **Performance law.** `AGENTS.md:165-168`: "A surface added for performance
  must arrive with a paired two-program cell comparing it with the plain
  spelling it replaces. Ratification waits until the surface strictly beats the
  plain form."

### 4.2 Option A in Jet: the record-and-rebuild loop

The workflow follows the record from collection to release.

1. **Collect.** Either profile a deployed binary with `jet perf attach <pid>
   --source app.jet` (sampling), or run a representative workload with
   `jet perf run app.jet --out app.jettrace` (add counting for exact edges and
   value profiles).
2. **Commit.** Put the record in the package. Go's `default.pgo` convention
   carries over: the package's record is used automatically, and an explicit
   flag (spelling below) overrides or disables it.
3. **Build.** `jet build --release` reads the record. The receipt prints its
   digest and the match share.
4. **Repeat.** Record again from the new binary. Canonical quantization keeps
   rebuilds stable.

Spelling. `--profile` already names build profiles (`--profile release`,
`Docs/spec/reference/cli.md:166`, D-BUILDPROFILE1). The record flag therefore
needs another word. The ballot shows `--hot <file.jettrace>` as an illustration
only; the spelling settles when its CLI row lands.

Consumers:

- **O1 (Jet backend, default `jet build`).** Block layout and hot/cold
  splitting in LIR. A hotness-weighted inline budget in the bounded O1 passes
  (#4135, SP20). Guarded devirtualization. Value-profile clones with entry
  guards. Hot-function ordering in the in-process linker (SP19). The linker
  ordering is Propeller's relink approach: Jet owns the linker, so it needs no
  binary rewriting.
- **O2 (LLVM first, D-EXEC1).** Once O2 emits LLVM IR, the record becomes
  `!prof` branch weights, function entry counts and value-profile (`VP`)
  metadata. On today's rustc bridge (`cli.md:166`) the record could at most
  become cold and inline hints `[INFERENCE]`. Full O2 PGO therefore waits for
  direct IR emission. Feeding rustc `-Cprofile-use` is not recommended: the
  profile would key on generated Rust symbols that the emitter churns, and the
  emitter retires.
- **O0.** It ignores the record. Meaning is identical on every level, and only
  speed differs, which I9 permits.

Budget: applying the record is a per-function lookup (memory-mapped, keyed by
symbol and digest) with no whole-program pass. It fits inside SP21's 2 s
bench300k budget. The cell records a separate `record_apply` phase.

### 4.3 Option B in Jet: the opt-in onboard re-specializer

Build: `jet build --release --adaptive app.jet` (illustrative spelling). The
binary is A's binary plus four parts:

1. **Slot calls for eligible functions.** Functions that are hot in the record,
   or all non-inlined functions if there is no record, get slot calls. The cost
   is one indirect call per entry. Every other call stays a direct `rel32`.
2. **A compact MIR slice** for those functions. It is the same serialized MIR
   the build used, with no second IR (D-TIER-FORM1).
3. **The Jet O1 pipeline and code generator,** linked from the same Jet source
   the compiler uses. It is the one backend compiled as a library into the
   runtime bundle, not a second implementation.
4. **A sampler thread and the loader** (`Image/Loader.jet`).

Policy:

- The optimizer starts after warm-up and samples at a low rate.
- It picks functions where any of these holds:
  - the live profile diverges from the build record;
  - a scalar argument, or an immutable captured value, is nearly always one
    value;
  - the running CPU has features the result rule allows and the build did not
    use.
- It compiles each one at low priority on one background thread, within a CPU
  share cap and a code-memory cap. Thresholds rise as the cap fills, as in JSC.
- It maps the code RW, copies it, flips it to RX and rebinds the slot.
- At exit, or periodically, it writes a `.jettrace` record, so B also feeds A's
  next build.

No mid-function deoptimization:

- Specializations guard at function entry only. A failed guard calls the
  prebuilt copy, which is always kept.
- There is no on-stack replacement and no frame-state maps. Loops already
  running in the old body finish there, as in the slot design.
- Jet's immutability facts make constant folding safe without Maglev's
  "deoptimize if the global mutates" machinery. An immutable value cannot
  change, so the only guard needed is on which value arrived.

Platforms:

- `--adaptive` is refused at build time for targets that forbid run-time code:
  iOS, consoles, web/wasm, and freestanding targets. The diagnostic names the
  reason.
- On macOS the package must declare the `allow-jit` entitlement. The build
  checks for it and the packager signs with it.
- A runtime switch (`JET_ADAPTIVE=0`, illustrative) disables the optimizer. The
  program then runs the identical prebuilt code.

I9 and determinism:

- **Results.** Re-specialized code comes from the same MIR, through the same
  backend, calling the same runtime. D-CPU-DISPATCH1's result law
  (bit-identical copies; FMA-style contraction only where allowed) applies to
  every runtime copy.
- **Test mode.** A test mode compiles every eligible function immediately with
  synthetic profiles and guards forced both ways. This is Jet's equivalent of
  HotSpot `-Xcomp`, and it runs the goldens.
- **Speed.** Speed varies within a run, but results do not. Benchmarks report
  warm-up and steady state separately.

What is not in B:

- No interpreter.
- No Cranelift.
- No speculative type tier (Jet has static types).
- No user-visible API. Explicit run-time staging, a library call that compiles
  a function for a given value, is a separate surface and would need its own
  ballot.

### 4.4 Option C: static only

Keep AOT, `#Multiversion` and comptime, and ignore records. This is the
simplest option. It gives up the measured 2-20% class of profile gains, has no
response to workload drift, and cannot fold run-time constants.

### 4.5 What the native backend (#4128 and siblings) and the speed plan need

| Need | A | B | Where |
|---|---|---|---|
| Stable function identity and MIR body digest emitted with the binary's source map | yes | yes | #4128 lowering; `.jettrace` source identity |
| Stack-sampling support in release binaries (frame pointers or unwind tables) and a real PC/stack sampler in the shared runtime; today's rows attribute only to `fn run` | yes | yes | runtime; `observability.md:50-52` |
| Counting instrumentation lowering (edge, trip-count, indirect-target and allocation-size counters) | yes | optional | #4128 lowering (MIR to LIR) |
| Branch weights on LIR edges; profile-driven block layout and hot/cold splitting | yes | yes | X64 select and layout |
| Hotness-weighted inline budget, guarded devirtualization and value-specialization clones inside O1's bounded per-function passes | yes | yes | #4135, SP20 |
| Hot-function ordering in the in-process linker | yes | n/a | SP19 |
| LLVM `!prof`, entry-count and `VP` metadata when O2 emits IR | yes | n/a | O2 path |
| Slot calls inside static executables for selected functions (today static images keep rel32) | no | yes | `jet-backend-design:541`; Image |
| Backend and O1 pipeline as a library linked into the runtime bundle; serialized MIR slice | no | yes | D-TIER-FORM1; D-CORE-BOUNDARY1 bundle |
| Loader on macOS (`MAP_JIT`, `pthread_jit_write_protect_np`) and Windows (`VirtualProtect`); today the OS layer is Linux only | no | yes | `OS/Linux.jet` siblings |

Speed-plan constraints: no whole-program pass for applying a record (SP16/SP20
style per-function work); byte-identical 1-job and N-job artifacts with a
record (#4128 criterion 4); `record_apply` reported as its own phase in the
throughput cell.

### 4.6 Measurement plan (paired cells, `AGENTS.md:165-168`)

- **A.** Same program and same workload, built without and with a record. Use
  the gauntlet's run-time cells plus one large binary, such as the self-hosted
  compiler compiling bench300k; Clang's own PGO build is the precedent. The
  record must come from a *different* input than the measured one, so a
  profile cannot be tuned to its own test.
- **B.** A long-running HTTP service whose traffic mix shifts mid-run, plus one
  run-time-constant workload (route table or regex). Compare B's binary with
  A's prebuilt binary built from a record of the first phase. Report warm-up
  and steady state separately. B's surface is not ratified as shipped until it
  strictly beats A on that cell.
- **Peers.** HotSpot with JEP 544, and .NET with R2R plus dynamic PGO, on the
  same service. The performance gate counts these as matched peers.

## 5. Ballot

Three options, recommended B:

- **A: learn from real runs, then rebuild.** The record-and-rebuild loop only.
- **B: A plus an opt-in in-program optimizer** for long-running processes.
- **C: static only.** AOT plus `#Multiversion` and comptime.

Why B:

- A captures most profile gains at no runtime cost, so every program gets them.
- The owner's goal is maximum speed in every scenario. Two scenarios only a
  runtime tier serves are workload drift and run-time constants, and B covers
  them for long-running processes.
- B needs no second implementation: it is the same MIR, the same Jet backend,
  the same runtime and the slot-table rebind already designed for `jet dev`.

B keeps two losses, each mitigated:

- **A bigger, opt-in binary that spends CPU and memory warming up.** It is off
  by default, capped, and ships only after its paired cell wins.
- **Run-time code generation that some platforms forbid.** The build refuses it
  there; elsewhere it is W^X, and switching it off runs identical prebuilt code.

## Sources (read 2026-10-04)

- go.dev/doc/pgo
- openjdk.org/jeps/410, /515, /544
- v8.dev/blog/maglev; v8.dev/blog/jitless
- microsoftedge.github.io/edgevr/posts/Super-Duper-Secure-Mode/
- webkit.org/blog/10308/speculation-in-javascriptcore/
- devblogs.microsoft.com/dotnet/announcing-dotnet-8/
- learn.microsoft.com/dotnet/core/runtime-config/compilation
- learn.microsoft.com/dotnet/core/deploying/ready-to-run
- learn.microsoft.com/aspnet/core/fundamentals/native-aot
- github.com/dotnet/runtime docs/design/features/tiered-compilation.md
- github.com/oracle/graal docs/reference-manual/native-image/PGO.md and PGO-Basic-Usage.md
- pypy.org
- web.archive.org (2020) luajit.org/performance_x86.html
- julialang.org/blog/2023/04/julia-1.9-highlights/
- llvm.org/docs/ORCv2.html; llvm.org/docs/HowToBuildWithPGO.html
- clang.llvm.org/docs/UsersManual.html (PGO sections)
- arXiv:1807.06735 (BOLT)
- research.google/pubs AutoFDO (CGO 2016) and Propeller (ASPLOS 2023)
- source.android.com/docs/core/runtime/jit-compiler
- developer.android.com/topic/performance/baselineprofiles/overview
- developer.apple.com com.apple.security.cs.allow-jit
- developer.apple.com/app-store/review/guidelines (2.5.2)
- docs.unity3d.com/Manual/scripting-restrictions.html
- pcre.org pcre2jit
- vldb.org/pvldb/vol4/p539-neumann.pdf (HyPer query compilation)

Not verified, so not used: LLVM CSSPGO paper numbers, Chromium's PGO blog
numbers (the page body did not extract), and HP Dynamo's speedups (no readable
primary copy). No Jet compiler, test or benchmark was run for this report.
