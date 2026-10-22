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
2. How user code should take part in compilation. Earlier drafts compared a
   Jai-style message loop, hooks, and a hybrid. This revision starts again
   from what users need and proposes something smaller than any of them.

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
Section 2 assumes they may.

Risks: the evaluator runs build programs at compile time, so every Core build
method must be evaluator-callable with the same meaning as the compiled form.
That is the existing rule for compile-time code, not a new one. The cutover
removes the host-method router in one change, per the greenfield rule.

## 2. Taking part in compilation, from first principles

### What people actually need

Ask what a programmer is trying to get done, not which compiler mechanism to
expose. Four jobs come up again and again:

|Job|Examples|Jet today|
|---|---|---|
|Collect: build a table from marked declarations|HTTP routes, CLI commands, plugin registries, game systems, migrations|Missing|
|Derive: write per-type code|Serialize, compare, debug print|Derives|
|Bind: turn an outside file into code|Protobuf, C headers, SQL schemas|`b.generate`, generated modules|
|Enforce: apply a project rule|No allocation in `#Hot` functions; every public error has a code|Missing|

Only Collect and Enforce are missing. Neither one needs to change the
program or invent new declarations. Collect produces a value, a table.
Enforce produces errors. That observation is the whole design.

### The rule that keeps it readable

**Nothing runs, and no code exists, unless a line you can read asks for it.**

A reader who sees `ROUTES` can jump to its definition and find the one line
that builds it. No code runs because a library was imported. No function
appears that no file contains. Message loops and hooks both break this rule:
with hooks, the reason a piece of behavior exists lives in a library you
never call; with a loop, it lives in a stateful event handler you have to
simulate in your head.

### Collect: a query that returns a list

A program asks for its own marked declarations with one compile-time call and
gets back an ordinary list of values:

```jet
#Route("GET /users/{id}")
fn get_user(id: Int) -> User { … }

#Route("POST /users")
fn create_user(body: NewUser) -> User { … }

ROUTES :: Program.functions(marked: Route)

fn main() {
    web.serve(port: 8080, routes: ROUTES)
}
```

Each element holds the function value and the marker's checked arguments.
`web.serve` takes a plain list. It does not know where the list came from,
and a test can pass it a hand-written one. A CLI library, a plugin registry
and a game's system list all work the same way: mark the declarations, ask
for them, and pass the list to a normal function.

The same query exists for types (`Program.types(marked: …)` and
`Program.types(implementing: …)`), which covers registries keyed by type.

### Enforce: the build program reports errors

Project rules live in `fn build` as ordinary code over the checked program,
and fail the build with Jet's usual what, why and fix:

```jet
fn build(b: Build) {
    app :: b.executable("app", sources: b.find("Source/**/*.jet"))

    loop f in b.program().functions(marked: Hot) {
        if f.allocates {
            b.error(at: f.span, what: "`{f.name}` allocates", why: "`#Hot` functions must not allocate", fix: "move the allocation out of the function or remove `#Hot`")
        }
    }

    b.default(app)
}
```

A library can ship a rule as a function, `web.check_routes(b)`, and the
application turns it on by calling it. The call is the audit trail: a
reviewer reads one file to see every rule a project enforces.

### Why there are no events, rounds or ordering rules

Collect queries read declarations (names, signatures and markers), never
function bodies, and they return values, not declarations. Nothing a query
returns can change what another query sees, so:

- **There is one pass.** The compiler checks declarations, answers queries,
  then checks bodies. There are no rounds and no fixed point.
- **It stays parallel and cached.** A query is a pure function of the
  declarations it reads, and compile-time calls are already cached by their
  inputs. The editor and the build compute the same answer.
- **It is deterministic.** Results come back in declaration-path order.
- **It cannot loop.** A declaration whose signature depends on a query result
  is an ordinary cycle error, reported like any other.

Enforce runs once, after checking is complete. It sees full facts, including
inferred effects, and can only add errors.

### What it deliberately leaves out

- **Generating new functions or types from the program.** Every case found so
  far is a table (Collect) or per-type code (Derive). S26 already draws the
  line here. A real counterexample gets its own ballot.
- **Code that runs because you imported a library.** That convenience is what
  makes hook systems hard to read and audit.
- **A message loop.** It serializes the compiler and asks the reader to run a
  state machine in their head. D-METAMUTATE1 rejected it, and nothing here
  reopens it.

### What this replaces

Earlier revisions of this document compared a message loop, hooks, and a
hybrid of the two. All three answered "how should user code join the
compiler's pipeline?" The better question is "what does the user want to
produce?" Collect and Enforce answer it with one query, one error method, and
no new execution model.

### Questions for the owner

1. Are `Program.functions(marked:)`, `Program.types(marked:)`,
   `Program.types(implementing:)`, `b.program()` and `b.error(...)` the right
   spellings?
2. Is "no new declarations generated from program facts" the right line?
3. Should project rules live in `fn build`, in `fn test`, or in both?
