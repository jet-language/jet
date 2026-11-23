# D-CORE-CHECK1 adversarial review (2026-11-23)

Ballot D-CORE-CHECK1 (card #6851) asks how a library declaration attaches its
own build-time check and registered error to each call, so that the eleven
Core-call checkers in `Compiler/JetSema/Source/Sema/Calls` can be deleted
(#6851 c3 to c6). The draft recommends A, call rules: a marker's `impl
Marker` gains `check: fn(self, call: CompilerCall)`, run at each call of a
marked declaration. This review attacked each strength the draft claims.
Probes ran on dev 3019343199 with the batch-7 s1 compiler (that commit plus
#6851 c3ar/c3br and #6846 c5 to c10, built at -O1).

## Measured facts

- The eleven files hold 10,390 lines: ProcessHandles 688, StreamHandles 712,
  UIHandles 421, TimeHandles 544, Hardware 498, Data 2142, Geometry 349,
  Services 1047, TaskGroups 1452, HTTPRouting 1482, and all of HTTP.jet
  (1055), of which the draft counts only the web-form part. Ten of them name
  47 distinct literal codes; HTTP.jet names 4 more. The draft's "about
  10,000 lines" and "49 codes" hold as orders of magnitude.
- `CompilerCall` already exists (`Core/Prelude/Prelude.jet:355`, the `.Call`
  payload a `.Calls{to: f}` watch hears). `reject` outside a marker's
  `expand` is E0962 (`Sema/Resolution.jet:139`). `#Pre(#run …)` fails with
  E0180 (`Sema/Contracts.jet:1144, 1153`). These claims hold.
- Build-time code runs natively in a comptime child process
  (`JetEval/Source/IR/Native.jet`, `x64_comptime_spawn`/`_load`). A rule
  call is therefore an exchange with that child, unless calls are batched
  (the comptime-child design's `CALL_BATCH`, one settle per wave; see
  `2026-11-23-comptime-child-review.md`).
- The nearest built mechanism, `meta.events(watch: .Calls{to: f})`, runs only
  under `jet build`. Copied out of the repository,
  `Examples/Modules/Packages/NoPrintInLib` reports `lints.NoPrintInLib`
  in its golden, yet `jet check .` prints "no problems" and `jet run .`
  runs the program with no finding. A probe whose `.Calls{to: helper}`
  handler panics stops `jet build` and passes `jet check`. That handler's
  panic is reported as `Jet.ArithmeticOrBoundsFailure`. Recorded as #6846
  c14.
- Per-call cost: see "Cost probe" below.

## Peer practice the draft leaves out

The draft compares only Zig `comptime` parameters and Rust
`#[diagnostic::on_unimplemented]`. Stronger prior art:

- **C# Roslyn analyzers.** A NuGet package ships analyzers that run inside
  the compiler and the IDE. Each finding has the package's own stable
  diagnostic ID, severity, suppression and code fix. This is the best case
  for "packages get the same check power as the platform", and it has run
  at scale for years. Its known costs: analyzer time shows up in every
  build and keystroke, which is why Roslyn reports per-analyzer timing and
  lets users switch analyzers off.
- **Swift macros.** A package's macro runs in a separate plugin process and
  can emit diagnostics with fix-its at chosen nodes. This is the same
  process boundary as Jet's comptime child. Its known costs: process
  launch and message exchange per expansion, which Swift has had to tune.
- **C++20 `consteval` checked literals** (`std::format_string`). The
  parameter's type has an immediate-function constructor that validates the
  literal while compiling. The check rides on the type, so no per-call hook
  exists, and a call whose argument is not a constant fails to compile.
- **TypeScript template-literal types** (Hono and tRPC typed routes). Route
  grammar and parameter names are checked by the type system with no user
  code running at all.

## Claims

### Claim 1: every one of the 49 codes keeps its tested message

**Attack.** Under A a rule runs only where the compiler runs rules. Today
the eleven checkers run in `jet check` and so in the editor. The only
call-hook mechanism built, `meta.events`, runs under `jet build` alone (see
Measured facts). Unless the ballot requires call rules to run under every
command that checks, A moves these errors out of `jet check` and the
editor. The users' messages would hold under `jet build` and vanish
everywhere else, which is a visible change.

Separately, #6852 c7 (decided 2026-11-23) moves the Args family into Core
source and retires E1301 to E1304 into ordinary method-arity errors. The
draft lists those codes among those kept (option B's "Kept in Sema"), so its
counts no longer hold.

**Verdict.** Holds only if A states that a call rule runs under `jet check`,
`jet run`, `jet build` and the language server with one meaning, and the
code count is corrected.

### Claim 2: packages get the same check power as Core

**Attack.** True of the surface: a package writes the same `check`. The
draft's mitigation for cost, though, is "Core's rules ship already
compiled", and no such mechanism exists: build-time code, Core's included,
is compiled at O0 into the comptime child for each build. If Core's rules
were precompiled and run another way, Core would hold a privilege packages
lack, against #6851's own title.

**Verdict.** Holds once "ship compiled" is removed or replaced by a
mechanism every package's rules share (batched calls through the one
comptime child).

### Claim 3: rules run while building but cost little

**Attack.** A `.Type` rule hears every method call on the marked type, so a
Core marker on a common handle type fires at a large share of a program's
calls. The compile-speed target is 30 million lines per second from cold,
about 33 ns per line. A per-call exchange with a child process is orders of
magnitude above that.

**Verdict.** Fails as written. The ballot must name the batching rule
(every rule call of one checking wave goes to the child in one batch) and
the measured cost per rule call against the compile-speed budget.

### Claim 4: wrong-count calls and missing methods are covered

**Attack.** The draft answers both: a rule runs before argument binding,
and a `.Type` rule hears calls of members the type lacks, before E0311.
That ordering must also fix what happens when a rule rejects: today each
snapshot shows one error, so a rejecting rule must stop the ordinary checks
for that call. The draft says only that a rule that rejects nothing leaves
the call to the ordinary checks.

**Verdict.** Holds once the ballot states that a rejection replaces the
ordinary checks of that call.

### Claim 5: the four options are the full space

**Attack.** One option is missing: checked parameter types (the C++
`consteval` and Zig shape). A parameter of type `RoutePath` converts from a
literal through a build-time constructor that may `reject`. It covers the
literal-grammar family (routes, format strings, queries, asset paths) with
cost per literal, not per call. It does not cover typestate (E2501), so on
its own it is weaker than A. As a hybrid, it could carry the literal family
while a `.Type` rule carries typestate, which shrinks the number of calls
that run a rule.

**Verdict.** Fails until the ballot lists checked parameter types as an
option or a hybrid and says why A wins.

## Cost probe

One generated program of 21,211 lines makes 20,000 calls of `helper` from 200
functions. One copy has a `.Calls{to: helper}` watch whose handler rejects
nothing; the other has no watch. Each was built cold with `jet build
main.jet` five times, alternating, with a fresh store each run, on the
batch-7 compiler while no other heavy job ran.

|Program|Median wall|Runs (s)|
|---|---:|---|
|no watch|0.801 s|0.78 0.80 0.80 0.80 0.81|
|watch|1.888 s|1.62 1.90 1.93 1.87 1.89|

The watch adds 1.09 s, about 54 µs per heard call, and more than doubles
the build. The compile-speed target allows about 33 ns per line. Calls are
therefore delivered one exchange at a time today, not batched. A
`.Type` rule on a common Core handle type would add this cost at every
method call on that type. Claim 3 fails as written: the ballot must make
batching part of the design and set a per-call budget that a probe like this
one checks.

## Verdict

A remains the strongest direction, but the draft cannot post as written.
Claims 3 and 5 fail; claims 1, 2 and 4 hold only within the limits above.
Before posting, the draft must:

1. Require call rules to run under `jet check`, `jet run`, `jet build` and the
   language server with one meaning (and depend on #6846 c14 for
   `meta.events`).
2. Replace "Core's rules ship already compiled" with batched rule calls
   through the one comptime child, and state the measured cost per call.
3. State that a rejection replaces that call's ordinary checks.
4. Correct the code count after #6852 c7 retires E1301 to E1304.
5. Add checked parameter types as an option or hybrid, and add Roslyn
   analyzers, Swift macros, C++ `consteval` literals and TypeScript
   template-literal types to the comparisons.
