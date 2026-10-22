# Build programs as Jet code: Core entry types and compiler events

Status: discussion document, 2026-10-22, revised the same day after owner
direction. The owner has set the direction for question 1: `Build`, `Dev` and
`Test` become Core types, and `BuildPlan` folds into `Build`. Question 2 is
still open. Every spelling below is an illustration; a chosen surface goes on
a Tower ballot before any code. This document replaces the earlier
"verification and build programs" proposal, which answered the wrong
question.

The document covers two language questions:

1. How `fn build(b: Build)`, `fn dev(d: Dev)` and `fn test(t: Test)` receive
   real Core types, and why they don't already.
2. Whether user code takes part in compilation through a message loop,
   through hooks, or through a hybrid of the two, and what the hybrid would
   look like.

The two questions meet in one place. Whatever mechanism lets user code see
the compilation, it is reached through the value the entry point receives. If
`Build` stays a compiler-private name, every answer to question 2 becomes
another table inside sema.

## 1. Build, Dev and Test as Core types

### What exists now

The entry points are recognized by file name and by a text scan for a
top-level `fn build(`, `fn dev(` or `fn test(`
(`Compiler/JetCli/Source/Cli/Entry.jet`, `Front.jet`, `PackageFiles.jet`,
`Compiler/JetDriver/Source/Driver/Loader.jet`). The scaffold templates in
`Compiler/JetCli/Source/Cli/Host.jet` write `fn build(b: Build)`,
`fn dev(d: Dev)` and `fn test(t: Test)`.

`Build`, `BuildPlan`, `Dev` and `Test` are not declared anywhere in Core. They
are bare nominal names (`Type.Named{name: "Build"}`) that the compiler knows
about:

- Sema owns the method surface. `Compiler/JetSema/Source/Sema/Calls/Build.jet`
  is a hand-written table of method names, arities and parameter types. The
  same arities appear a second time as error text in
  `sema_build_context_arity_description`. `Calls/Dev.jet` does the same for
  `watch`, `serve` and `reload`. `Test` has no methods at all.
- Methods are overloaded by argument count. `b.action` takes 5 or 7 to 15
  positional arguments, `b.legacy` takes 6 to 17, and each extra position is
  a different typed control (toolchain, probes, signing identity, and so on).
  Labels are rejected (E0764).
- Codegen turns every call into one string-keyed host call,
  `jet_comptime_host_method` with the route `"Build.<method>"`
  (`Compiler/JetCodegen/Source/Codegen/BuildMethods.jet`). The build session
  dispatches on that string (`Compiler/JetBuildSession/Source/Session.jet`,
  `Compiler/JetDriver/Source/Driver/BuildAdapter.jet` and
  `Compiler/JetDriver/Source/Driver/DevQueries.jet`).
- The Rust implementation has its own `BuildContext` struct
  (`crates/jet-comptime/src/Comptime/Build/context.rs`).
- The prose disagrees with itself: the spec writes
  `fn build(b: Build) -> BuildPlan`, a decision entry writes
  `fn test(suite: Test)`, and the template writes `t: Test`.

### Why it is not already a Core type

There is no language reason. The build surface started in the Rust compiler
as a host context for the compile-time evaluator. At that time Core could not
declare a type whose methods call into the runtime, so the methods became a
compiler table and a string router. That limitation is gone. Core already
declares host-backed types the same way a build type would need:

```jet
struct FileReader {
    path: String

    fn stat(self) -[FS.Read]> Stat IOError! {
        stat_from_row(core.files.__reader_stat(self))
    }
}
```

`FileReader` (in `Core/Files/Files.jet`) is an ordinary Core declaration with
documented methods and effects. Its bodies call private runtime entry points.
Nothing prevents `Build` from following the same pattern. The current shape
survives only because nobody has done the cutover.

### What the current shape costs

- **The user cannot see the API.** Hover, go-to-definition, documentation and
  completion work from declarations, and there is none. The only description
  of `b.action` is a sema table and a diagnostic string.
- **It breaks one mechanism (I8).** Every other Core API is a declaration
  with one meaning in the shared runtime (I9). `Build` is a second mechanism:
  sema type rules, a codegen route and a session-side string switch that must
  agree by hand. Adding one method means editing three or four places in two
  languages.
- **Arity overloading hides meaning.** A call with eleven positional
  arguments cannot be read without the table. Jet already has named
  parameters with defaults and option records; the build surface cannot use
  them.
- **Libraries cannot extend the build.** A helper such as
  `fn add_shader_pipeline(b: Build, dir: String)` in a reusable package has no
  declared type to accept, no method set to document, and no version contract.
- **Test and Dev are empty.** `t: Test` carries no capability, so a test
  entry cannot register suites, fixtures, golden directories or tiers through
  a typed value. `Dev` has three string-routed methods.
- **The failure type is ad hoc.** `BuildError` is also a bare name. Users
  cannot match on its cases.

### Proposed shape

Declare the three entry types, and the values they hand out, in Core. Each
method is a normal declaration with labeled parameters, defaults, effects and
a failure type. The bodies call the runtime's build session through private
entry points, exactly as `FileReader` does. Illustrative only:

```jet
/// The build program's view of one package build.
struct Build {
    fn executable(self, name: String, sources: [String], deps: [BuildTarget]{}, toolchain: Toolchain?{None}) -> BuildTarget BuildError! { … }
    fn library(self, name: String, sources: [String], deps: [BuildTarget]{}) -> BuildTarget BuildError! { … }
    fn action(self, name: String, run: [String], inputs: [String], outputs: [String], env: [String]{}) -> BuildAction BuildError! { … }
    fn generate(self, name: String, items: Generated) -> Unit BuildError! { … }
    fn find(self, glob: String) -[Build.Read]> [String] { … }
    fn default(self, target: BuildTarget) { … }
    fn graph(self) -> BuildGraph { … }
}

enum BuildError {
    MissingInput{path: String}
    ToolFailed{action: String, status: Int}
    Conflict{target: String, reason: String}
}

/// The `jet test` program's view of one package's tests.
struct Test {
    fn suite(self, name: String, dir: String) -> TestSuite { … }
    fn golden(self, dir: String, tiers: [Tier]{Tier.all()}) { … }
}

/// The `jet dev` loop.
struct Dev {
    fn watch(self, glob: String) { … }
    fn serve(self, port: Int) { … }
    fn reload(self, target: String) { … }
}
```

### BuildPlan folds into Build

Today `b.plan(target)` returns a `BuildPlan` handle. The handle does two
things: it names the default target, and `plan.graph()` projects the checked
`BuildGraph` (`Compiler/JetBuildSession/Source/Graph.jet`,
`Compiler/JetDriver/Source/Driver/BuildAdapter.jet`). Neither needs its own
type:

- `b.default(target)` records the default target on the session, which is
  where the plan handle already stores it.
- `b.graph()` returns the same checked `BuildGraph` projection. The session is
  the plan; a second handle that points back at it adds nothing.

So `fn build(b: Build)` registers everything through `b` and returns `Unit`.
There is no `BuildPlan` type, no `-> BuildPlan` signature and no `plan`
method. The cutover removes `TYPE_BUILD_PLAN`, the `BuildPlan` reserved name,
the `"BuildPlan"` session handle kind and the `BuildPlan.graph` host route in
the same change, and corrects the spec line that still shows
`fn build(b: Build) -> BuildPlan`. The LSP's request-local use of the plan
(`Compiler/JetLSP/Source/LSP/Navigation.jet`) reads the session's selected
generated modules instead.

### What changes for the compiler

- Sema's `Calls/Build.jet` and `Calls/Dev.jet` disappear. Calls type-check as
  ordinary method calls on declared Core types.
- Codegen's `BuildMethods.jet` route disappears. Calls lower like any other
  Core call, and the runtime build session keeps the policy (I9).
- The evaluator's `Build.*`, `Dev.*` and `BuildPlan.graph` host routes go
  away with the router. The evaluator runs the Core bodies like any other
  compile-time code.
- Discovery stops scanning text. The driver finds the entry by checking for a
  function named `build`, `dev` or `test` whose single parameter has the
  declared Core type, which `DevQueries.jet` already half does for `Dev`.

Owner decisions still open for question 1: the exact method set and
spellings, the `Test` surface, and whether build values may be passed into
library functions (which makes the build API a public, versioned contract).
The hybrid design in section 2 assumes they may.

Risks: the evaluator runs build programs at compile time, so every Core build
method must be evaluator-callable with the same meaning as the compiled form.
That is the existing rule for compile-time code, not a new one. The cutover
removes the host-method router in one change, per the greenfield rule.

## 2. Compiler message loops, hooks, or a hybrid

### The question

Jet lets user code read the program (`T.reflect()`, whole-program read-only
facts from the build entry) and add code (derives, `b.generate`, generated
modules). It does not let user code take part in compilation as it happens.
The question is which shape that participation should take, if any.

Existing rulings bound the answer. S26 keeps compile-time code value-only: it
never creates or selects types and never changes dispatch. The metaprogramming
ceiling allows read-only reflection and structured diagnostics and rejects AST
mutation and macros. D-METAMUTATE1=A rejected "Jai-style AST mutation, message
loop and user macros" as one package. The options below keep the first two
rulings intact. Options A and C reopen the message-loop part of the third, so
they need an owner decision. None of them mutates existing code: every option
only observes typed facts and adds checked items that re-enter the front end
(R11).

### Option A: message loop

The root build program drives compilation and receives events in order, the
way Jai's `compiler_wait_for_message` does:

```jet
fn build(b: Build) {
    session :: b.compile("app")
    routes := [Route]{}
    loop event in session.events() {
        if event == {
            .Typechecked(decl) -> if decl.has_marker("route") -> &routes.push(route_of(decl))
            .TypecheckedAll -> session.generate("routes") { … routes … }
            .Complete -> break
            else -> {}
        }
    }
}
```

Benefits:

- **Full control and state.** One program sees every event in order and keeps
  whatever state it wants. Whole-program decisions (build a route table from
  every marked function, refuse an allocation in a marked hot path, emit a
  registry of every type implementing a trait) are just code.
- **Iterative generation is explicit.** Jai's `TYPECHECKED_ALL_WE_CAN` shows
  the pattern: the loop learns that the compiler is stuck, adds code, and the
  compiler continues. The user sees the rounds.
- **Easy to explain.** It is a loop over a stream; no registration rules.

Costs:

- **One consumer.** Only the root build program owns the loop. A library
  (a web framework, an ECS, a serializer) cannot ship its own participation;
  every application must paste the framework's loop body into its own. That
  is the main reason Jai programs end up with one large hand-merged
  metaprogram.
- **Serializes the compiler.** Every event is a round trip into user code
  that holds mutable state. To keep results deterministic, the compiler must
  deliver events in a fixed order, which defeats parallel sema, and the speed
  target that comes with it.
- **Defeats incremental and LSP caching.** The loop's state is opaque, so any
  edit must replay the whole stream. An editor cannot run an arbitrary loop on
  every keystroke, so the editor and the build disagree.
- **Determinism is a user burden.** A loop that depends on event order or
  timing produces different output on different machines unless the compiler
  freezes the order.
- **Weak collect-all.** A loop that stops at its first surprise hides later
  diagnostics. The compiler's keep-going modes cannot see inside it.
- **Wide API surface.** Every event kind and its payload becomes a public,
  stable contract that the compiler's internal phases must keep producing.

What it unlocks: whole-program generation and enforcement with arbitrary
state, in exchange for parallelism, caching and composition.

### Option B: hooks

Packages declare functions that the compiler calls for a typed event that
matches a filter. A hook is pure: it receives facts and returns additions or
diagnostics; it holds no state between calls.

```jet
/// Called once for each checked function marked `#Route`.
#Hook(.Typechecked, marker: "Route")
fn route_entry(decl: FunctionFacts) -> Generated {
    ...
}
```

Benefits:

- **Composable across packages.** Each library ships its own hooks. An
  application that imports a web framework and a serializer gets both without
  writing any glue. This is how attribute processors work in Kotlin KSP and
  Java annotation processing, and why they scale to ecosystems.
- **Parallel.** The compiler calls hooks on its worker threads as facts become
  ready. Nothing waits for one user loop.
- **Cacheable.** A pure hook's output depends only on its input facts, so the
  compiler can key it by the hash of those facts. Incremental builds and the
  LSP rerun only hooks whose inputs changed, and the editor sees exactly what
  the build sees.
- **Deterministic by construction.** Outputs merge in a canonical order (for
  example by declaration identity), never by arrival order.
- **Collect-all friendly.** Each hook call is a separate item; a failing hook
  becomes one diagnostic row and the rest keep going.
- **Sandboxable and narrow.** A hook sees only the facts its event carries,
  and its effects are checked like any other compile-time code (Tier 0 by
  default).

Costs:

- **No cross-event state.** A route table needs every route at once. Pure
  hooks need a second form, an aggregate (reduce) hook that runs once over all
  the outputs of a per-declaration hook.
- **Ordering between hooks.** When two packages generate code that the other
  reacts to, the compiler needs round rules: generated items re-enter the
  front end, fire events for new declarations only, and stop at a fixed point
  or a bounded round count with a diagnostic.
- **Less raw power.** Anything that needs arbitrary state or a decision across
  unrelated events must be expressed as map plus reduce.

What it unlocks: ecosystem-scale code generation (routes, serializers,
registries, bindings, test discovery) that stays parallel, cached and
identical in the editor and the build.

### Option C: hybrid

One typed fact model in Core serves two scopes:

- **Package passes** (option B) for per-declaration work and per-package
  aggregates. They are the default and the only form a library may use.
- **The root build program** for application-wide decisions. It sees the
  merged facts after the package passes ran, not the live phase stream, so it
  never serializes sema.

Both scopes produce the same two outputs: checked `Generated` items and
structured diagnostics. Neither may edit existing items, so S26 and the
no-mutation ceiling hold.

The first draft of this option accepted three costs: two forms to teach,
round rules, and a large public event contract. The design below removes most
of each instead of accepting them.

### The hybrid interface, concretely

#### Facts: what user code can see

Facts are read-only Core values. They are projections of what sema already
records, generated from one registry so the compiler and Core cannot drift.
Illustrative:

```jet
/// One checked function, as a pass sees it.
struct FunctionFact {
    id: DeclId
    name: String
    module: String
    markers: [MarkerFact]
    params: [ParamFact]
    returns: TypeFact
    effects: [String]
    span: SourceSpan
}

/// One checked nominal type.
struct TypeFact {
    id: DeclId
    name: String
    module: String
    markers: [MarkerFact]
    fields: [FieldFact]
    implements: [String]
    span: SourceSpan
}

/// A marker written on a declaration, with its checked arguments.
struct MarkerFact {
    name: String
    args: [ConstValue]
}
```

There are no phase events in the public surface. A pass names the facts it
wants, and the compiler decides when they are ready. This is the central
move: the public contract is a small set of fact types, not the compiler's
phase order, so phases can be split, merged, parallelized or reordered
without breaking user code.

#### Outputs: what user code can produce

```jet
/// Everything a pass may hand back.
struct PassOutput {
    items: [Generated]{}
    diagnostics: [PassDiagnostic]{}
}

/// A user diagnostic with the same what, why and fix shape as the compiler's.
struct PassDiagnostic {
    code: String
    at: SourceSpan
    what: String
    why: String
    fix: String
}
```

`Generated` is the existing generated-item value that `b.generate` takes. A
pass cannot return anything else: no file writes, no edits to existing items,
no type selection.

#### Package passes: one form, chosen by parameter type

A package pass is an ordinary function with a `#Pass` marker. Its parameter
type alone decides its granularity, so there is one form to teach, not two:

```jet
/// Runs once for each checked function marked `#Route`, in parallel.
#Pass(marker: "Route")
fn route_stub(route: FunctionFact) -> PassOutput { … }

/// Runs once over every checked function marked `#Route` in the program.
#Pass(marker: "Route")
fn route_table(routes: [FunctionFact]) -> PassOutput { … }

/// Runs once for each checked type that implements `Serialize`.
#Pass(implements: "Serialize")
fn serializer(ty: TypeFact) -> PassOutput { … }
```

A single fact means "each"; a list means "all". The list arrives sorted by
declaration identity, so the reduce step is deterministic without the author
doing anything.

A pass that needs state across many declarations and still wants to run in
parallel declares a fold: a state type with `step`, `merge` and `finish`. The
compiler splits the facts across workers, calls `step` on each slice,
combines the slices with `merge` in canonical order, and calls `finish` once.

```jet
/// Collects allocation sites in `#Hot` functions, in parallel.
#Pass(marker: "Hot")
struct HotAllocations: PassFold<FunctionFact> {
    sites: [SourceSpan]{}

    fn step(self, f: FunctionFact) -> HotAllocations { … }
    fn merge(self, other: HotAllocations) -> HotAllocations { … }
    fn finish(self) -> PassOutput { … }
}
```

`merge` must be associative. The test mode checks this by merging the same
slices in two groupings and comparing the results. That gives Jai-style
accumulated state without a serial loop.

#### The root build program: configure, then decide

The root package's `fn build(b: Build)` is where the application takes part.
It does not get a message loop. It gets two things through `b`:

```jet
fn build(b: Build) {
    app :: b.executable("app", sources: b.find("Source/**/*.jet"))

    // Configure package passes the application imported.
    b.passes.configure("web.route_table", options: RouteOptions{prefix: "/api"})
    b.passes.disable("orm.migrations")

    // A whole-program step over merged facts, after every package pass.
    b.after_check((program: ProgramFacts) -> PassOutput {
        checks := [PassDiagnostic]{}
        loop f in program.functions(marker: "Hot") {
            if f.effects.contains("Alloc") -> &checks.push(hot_alloc_error(f))
        }
        PassOutput{diagnostics: checks}
    })

    b.default(app)
}
```

`ProgramFacts` is the merged view: every function, type and marker after the
package passes' generated items have been checked. `b.after_check` takes a
function value, so it follows the same purity, caching and effect rules as a
package pass; only its scope differs. It stays behind the existing
root-package opt-in (`metaprogramming: unrestricted`) because it sees the
whole program.

#### Rounds become strata

Generated items can define declarations that other passes react to. Instead
of a fixed-point loop with a round limit, each pass states what it produces:

```jet
#Pass(marker: "Route", produces: .Functions(marker: "Handler"))
fn route_stub(route: FunctionFact) -> PassOutput { … }
```

The compiler builds a graph from consumed markers and traits to produced ones,
orders the passes into strata, and runs each stratum after the previous
stratum's output has been checked. A pass that produces something it did not
declare gets a diagnostic. A cycle in the graph is a compile error naming
every pass on the cycle, reported before any pass runs. Rounds are finite,
ordered and inspectable (for example `jet inspect passes`, an illustrative
command), and no user has to reason about convergence.

### Designing away the downsides

|Downside|Design answer|
|---|---|
|Two forms to teach|One form. A pass is a function, and its parameter type (one fact, a list, or a fold) picks the granularity. The root step is the same function shape handed to `b.after_check`.|
|The root step serializes the compiler|It runs once per stratum on merged facts, after the parallel passes. Sema never waits on user code in the middle of a phase.|
|Cross-declaration state needs a loop|Folds give state with parallel `step` and canonical `merge`. Lists give whole-set reduces. The root step gives arbitrary logic over the merged program.|
|Round rules and non-termination|Passes declare what they produce. The compiler orders them into strata and rejects cycles before running anything.|
|Determinism is the user's job|Inputs arrive in declaration-identity order, folds merge in canonical order, and outputs merge by producing pass and declaration identity, never by arrival time.|
|Caching and editor parity|Passes are pure functions of the facts they receive. The compiler records which facts each call read and keys its output by their hash, the same query model the LSP needs. The editor reruns exactly the calls whose inputs changed, so it shows what the build shows. The root step is cached the same way.|
|A public phase-event contract|There are no phase events. The contract is a small set of fact types generated from sema's fact registry and versioned with Core.|
|Packages colliding|Generated items are namespaced by the producing package. Two passes producing the same name is a diagnostic that names both passes.|
|Generated code is hard to see|Every generated item records the pass and input facts that made it. Hover and go-to-definition land on the generated source, and the pass inspector lists every call with its inputs and outputs.|
|Hard to test|A pass is an ordinary function over fact values. A test builds `FunctionFact` values directly and checks the `PassOutput`, through the `Test` entry like any other test.|
|Collect-all|Each pass call is one item. A failing or panicking call becomes one diagnostic row, and every other call still runs.|
|Security|Passes are compile-time code with checked effects, Tier 0 by default. They read no files or network unless the root build grants it through `b`, and every granted read is recorded as a cache input.|
|Speed|Per-declaration passes run on sema's worker pool, and untouched calls are cache hits. A program with no passes pays nothing.|

What remains: the fact types become a public, versioned API, and one marker
(`#Pass`) and one fold contract enter the language. Those are real costs, but
they are the size of one Core module, not the size of the compiler's phase
pipeline.

### Trade-offs side by side

|Axis|A: loop|B: hooks|C: hybrid|
|---|---|---|---|
|Control and state|Full|Per call, plus aggregates|Per call, lists, parallel folds, and full logic over merged facts at the root|
|Composition across packages|Root only|Every package|Every package, configured by the root|
|Parallel compilation|Serialized by event order|Parallel|Parallel; the root step runs once per stratum|
|Incremental and LSP caching|Replays everything|Per hook, by input hash|Per call, keyed by the facts it read, root step included|
|Determinism|User's job|By construction|By construction|
|Generation rounds|Explicit in the loop|Compiler-driven fixed point|Declared strata; cycles rejected up front|
|Collect-all diagnostics|Loop can hide later errors|One row per failing call|One row per failing call or root step|
|Sandboxing|Whole loop is trusted code|Per hook effect check|Per call effect check; root behind opt-in|
|Public API surface|Every phase event|Selected events and fact types|Fact types only|
|Fit with current rulings|Reopens D-METAMUTATE1 (message loop)|Within the ceiling, if hooks count as additive generation|Adds `#Pass`; the root step relaxes the message-loop ruling only for merged facts|

### What each option unlocks for users

- Route tables, RPC stubs and command-line parsers from marked functions: all
  three; B and C let the framework ship it.
- Serializers and bindings derived from types across packages: B and C
  without application glue.
- Program-wide policy ("no allocation in `#Hot` functions", "every public
  error type has a code"): A and C directly; B through an aggregate hook.
- Test discovery and golden registration: B, through `Test` once it is a Core
  type.
- Build graphs that depend on the checked program (generate a target per
  service declared in source): A and C.

### Recommendation for discussion

Question 1 has its direction: do the `Build`, `Dev` and `Test` cutover with
`BuildPlan` folded into `Build`. It removes a second mechanism and is a
prerequisite for any compiler-participation API, because passes are reached
and configured through a declared `Build`.

For question 2, adopt the hybrid as designed above and build it in this order,
each step useful on its own:

1. The fact types and per-declaration and list passes (option B's core).
2. Strata with declared `produces` and up-front cycle errors.
3. Folds, once a concrete pass needs parallel state.
4. The root `b.passes` configuration and `b.after_check` step over merged
   facts.

Option A's live loop gives up the properties Jet ranks highest (one meaning,
determinism, speed, editor parity) for power that the hybrid provides without
them.

### Questions for the owner

1. What is the exact method set for `Build`, `Dev` and `Test`, and may build
   values be passed into library functions?
2. Is `#Pass`, with granularity chosen by parameter type, allowed under the
   metaprogramming ceiling as additive generation, or does it need its own
   ruling?
3. Is the message-loop rejection relaxed for `b.after_check` over merged
   facts, or does the root get only `b.passes` configuration?
4. Which fact types are public in the first cut: `FunctionFact`, `TypeFact`,
   `MarkerFact` and `ProgramFacts`, or more?
5. Do folds ship in the first cut, or wait for a concrete need?
