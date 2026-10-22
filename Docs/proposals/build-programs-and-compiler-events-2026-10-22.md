# Build programs as Jet code: Core entry types and compiler events

Status: discussion document, 2026-10-22. It replaces the earlier
"verification and build programs" proposal, which answered the wrong
question. Nothing here is ratified. Every spelling below is an illustration
for discussion; a chosen surface goes on a Tower ballot before any code.

The document covers two language questions:

1. Should `fn build(b: Build)`, `fn dev(d: Dev)` and `fn test(t: Test)`
   receive real Core types, and why don't they already?
2. Should user code take part in compilation through a message loop, through
   hooks, or through a hybrid of the two?

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

What changes for the compiler:

- Sema's `Calls/Build.jet` and `Calls/Dev.jet` disappear. Calls type-check as
  ordinary method calls on declared Core types.
- Codegen's `BuildMethods.jet` route disappears. Calls lower like any other
  Core call, and the runtime build session keeps the policy (I9).
- Discovery stops scanning text. The driver finds the entry by checking for a
  function named `build`, `dev` or `test` whose single parameter has the
  declared Core type, which `DevQueries.jet` already half does for `Dev`.
- The return type becomes a decision instead of an accident: either
  `fn build(b: Build)` registers through `b` and returns nothing, or it
  returns a `BuildPlan` value. The two must not coexist.

Owner decisions this needs: the exact method set and spellings, whether
`build` returns a plan, the `Test` surface, and whether build values may be
passed into library functions (which makes the build API a public, versioned
contract).

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

One typed event model in Core serves both forms:

- **Package hooks** (option B) for per-declaration work and per-package
  aggregates. They are the default and the only form a library may use.
- **A root subscription** for the application's whole-program decisions,
  reached through the build value: `loop event in b.events(.TypecheckedAll)`.
  It sees the final, merged facts after the hooks ran, not the live phase
  stream, so it does not serialize sema. It stays behind the existing
  root-package opt-in (`metaprogramming: unrestricted`).

Both forms produce the same two outputs: checked `Generated` items and
structured diagnostics with code, what, why and fix. Neither may edit existing
items, so S26 and the no-mutation ceiling hold.

Benefits: libraries compose through hooks; the application still has one
place for whole-program policy; the compiler keeps parallel, cached phases for
everything except the root subscription, which runs once per round on merged
facts.

Costs: two forms to teach, even with one event model. The round rules must be
specified once and shared by both. The event types become a public contract.

### Trade-offs side by side

|Axis|A: loop|B: hooks|C: hybrid|
|---|---|---|---|
|Control and state|Full|Per call, plus aggregates|Hooks per call; root has full state over merged facts|
|Composition across packages|Root only|Every package|Every package, plus root|
|Parallel compilation|Serialized by event order|Parallel|Parallel except the root step|
|Incremental and LSP caching|Replays everything|Per hook, by input hash|Per hook; root step reruns on merged-fact change|
|Determinism|User's job|By construction|By construction; root step sees a canonical order|
|Generation rounds|Explicit in the loop|Compiler-driven fixed point|Compiler-driven, root step once per round|
|Collect-all diagnostics|Loop can hide later errors|One row per failing call|One row per failing call or root step|
|Sandboxing|Whole loop is trusted code|Per hook effect check|Per hook; root behind opt-in|
|Public API surface|Every phase event|Selected events and fact types|Selected events and fact types|
|Fit with current rulings|Reopens D-METAMUTATE1 (message loop)|Within the ceiling, if hooks count as additive generation|Reopens the message-loop part, scoped to merged facts|

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

Do question 1 first. Making `Build`, `Dev` and `Test` Core types removes a
second mechanism and is a prerequisite for any event API, because events must
be reached through a declared type.

For question 2, start with hooks plus aggregate hooks (option B). It covers the
common ecosystem cases, keeps parallelism, caching and determinism, and stays
closest to the current ceiling. Add the root subscription of option C only when
a concrete whole-program need cannot be expressed as map plus reduce, and do it
on merged facts rather than on the live phase stream. Option A's live loop
gives up the properties Jet ranks highest (one meaning, determinism, speed,
editor parity) for power that option C provides at lower cost.

### Questions for the owner

1. Do `Build`, `Dev` and `Test` become Core declarations, with the sema tables
   and the host-method router removed in one cutover?
2. Does `fn build(b: Build)` register through `b`, or return a `BuildPlan`?
3. Are hooks allowed under the current metaprogramming ceiling as additive
   generation, or do they need their own ruling?
4. Is the message-loop rejection reopened for a root subscription over merged
   facts (option C), or does it stand?
5. Which events are public: per declaration typechecked, all typechecked,
   and build complete only, or more?
