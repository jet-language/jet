# Build programs as Jet code: Core entry types and compiler events

Status: discussion document, 2026-10-22, revised the same day after owner
direction. The owner has set the direction for question 1: `Build`, `Dev` and
`Test` become Core types, and `BuildPlan` folds into `Build`. Question 2 is
still open on its exact surface, but the goal is set: full Jai power. Every
spelling below is an illustration; a chosen surface goes on
a Tower ballot before any code. This document replaces the earlier
"verification and build programs" proposal, which answered the wrong
question.

The document covers two language questions:

1. How `fn build(b: Build)`, `fn dev(d: Dev)` and `fn test(t: Test)` receive
   real Core types, and why they don't already.
2. How Jet offers full Jai-level compiler control (message loop, adding and
   rewriting code, macros, unrestricted compile-time execution) while
   keeping code easy to reason about and the power easy to limit and audit.

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

## 2. Full compiler control, designed the Jet way

### The goal

Anything Jai's metaprogramming can do, Jet should be able to do. That
includes:

- driving the compiler from a program;
- receiving a message for each declaration as it is checked;
- adding code at any point before checking ends;
- rewriting the body of a checked function;
- expanding macros in place;
- running arbitrary code at compile time;
- controlling build options and outputs.

Read-only reflection plus additive generation is not enough.

The question is how to offer that power without giving up what Jet ranks
highest: code a reader can reason about, a fast and parallel compiler,
deterministic builds, editor parity, and power that a team can see, limit
and audit.

### Four rules that make the power safe to have

1. **Power is a value you hold.** Full control is one Core value, `Compiler`.
   Only the root build program can obtain it. Anything else gets it only when
   it is passed in explicitly. Searching for `Compiler` finds every place in a
   project that has the power.
2. **Every change is visible.** Every added, rewritten or expanded piece of
   code records who made it. `jet expand` prints the final code of any
   function. The editor shows a "rewritten by" lens. A diagnostic inside
   rewritten code points both at the source and at the line that rewrote it.
3. **The compiler still checks everything.** Added and rewritten code goes
   back through sema like hand-written code. Memory safety, type safety and
   `#Unsafe` auditing hold for metaprograms exactly as for programs. Codegen
   only ever sees checked code.
4. **You pay only for what you watch.** The compiler pauses only the
   declarations a metaprogram asked to see. Everything else keeps compiling
   in parallel.

### The ladder: one mechanism, three heights

There is one mechanism, the compiler message stream, reached through
`Compiler`. Everything else is an ordinary Core function built on it, so a
user climbs only as high as the job needs.

**Height 1: everyday forms (no `Compiler` needed).**

Derives, `b.generate`, and two small library functions cover most real uses:

```jet
#Route("GET /users/{id}")
fn get_user(id: Int) -> User { … }

ROUTES :: Program.functions(marked: Route)      // a plain list

fn main() {
    web.serve(port: 8080, routes: ROUTES)
}
```

```jet
fn build(b: Build) {
    loop f in b.program().functions(marked: Hot) {
        if f.allocates -> b.error(at: f.span, what: "`{f.name}` allocates", why: "`#Hot` functions must not allocate", fix: "move the allocation out or remove `#Hot`")
    }
}
```

These are implemented in Core on top of the message stream. They are not a
second mechanism.

**Height 2: macros and inserted code.**

A macro is a compile-time function from code to code. It is called with a
`#` so a reader always knows the call expands:

```jet
macro retry(times: Int, body: Code) -> Code {
    #code {
        loop attempt in 1...#(times) {
            if #(body) == .Ok(value) -> break value
        }
    }
}

user :: #retry(3) { fetch_user(id) }
```

`#insert` places a `Code` value computed at compile time, as Jai's `#insert`
does. `Code` is a typed syntax tree. It can be written as a quote,
`#code { … }` with `#( … )` splices, or built as data, and every name in it
resolves where the macro was written (hygiene) unless it is spliced in.

**Height 3: the compiler loop, with full control (`Compiler` only).**

```jet
fn build(b: Build) {
    app :: b.executable("app", sources: b.find("Source/**/*.jet"))
    meta :: b.compiler(app)

    loop message in meta.messages(watch: .Functions(marked: Traced)) {
        if message == {
            .Checked(.Function(f)) -> {
                meta.rewrite_body(f, #code {
                    trace.enter(#(f.name))
                    defer trace.exit(#(f.name))
                    #(f.body)
                })
            }
            .Stuck(waiting) -> meta.add(module: "generated.handlers", code: handlers_for(waiting))
            .Done -> break
            else -> {}
        }
    }
}
```

This is Jai's `compiler_wait_for_message` loop. `Checked` matches Jai's
TYPECHECKED message. `Stuck` matches TYPECHECKED_ALL_WE_CAN: the compiler
cannot continue until someone adds the names it is waiting for.
`rewrite_body` matches `compiler_modify_procedure`. `meta.add` matches
`add_build_string`. Build options, targets, outputs and post-build steps are
methods on `Build`, which section 1 already makes a Core type.

A library can offer a whole metaprogram, but only as a function that takes
the power from the application:

```jet
trace.install(meta, marked: Traced)
```

That line is the entire audit trail for tracing.

### How each Jai weakness is designed away

|Jai weakness|Jet design|
|---|---|
|The loop serializes compilation|`watch:` filters what the loop sees. Only watched declarations, and the code that depends on them, wait for the loop; everything else compiles in parallel.|
|Message order depends on timing|Messages arrive in a fixed order: by phase, then by declaration path. The same program always gives the same message sequence on every machine.|
|One big hand-merged metaprogram|Libraries ship metaprograms as ordinary functions that take `Compiler` (`trace.install(meta)`). The application composes them by calling them, in the order it writes.|
|Hard to see what code really runs|Provenance on every node, `jet expand`, the editor lens, and diagnostics that name both the source line and the rewriting line.|
|Rewritten code can break anything|It is re-checked by sema. A rewrite that introduces unsafe code needs `#Unsafe` like any other code. Codegen never sees unchecked code.|
|Compile-time code can do anything to the machine|Compile-time effects are checked. Files, network, processes and FFI at compile time need an explicit grant (`b.allow(.Files, .Network)`), and every granted read becomes a cache input.|
|The editor cannot run the loop|The loop runs in the language server in the background, with the same inputs and the same fixed message order, and is cached by the facts it read. The editor shows the same final code as the build.|
|Incremental builds rerun everything|The compiler records which messages the loop received and what it did. If no watched declaration changed, it replays the recorded actions without running the loop.|
|Errors stop the metaprogram|The loop also receives `.Error` messages and can keep going. A failing macro or rewrite becomes a diagnostic row, and the compiler continues with everything else.|
|Anyone can rewrite anyone's code|Rewriting a function from another package requires the application to say so: `meta.allow_rewrite(package: "orm")`. A team can forbid that, or forbid `Compiler` entirely, in the package manifest, and `jet audit meta` lists every power in use with its location.|

### Control for teams

The package manifest sets the ceiling, and CI can enforce it:

```toml
[meta]
compiler = "root"        # "none" | "root" (default) | "any"
rewrite = "own-package"  # "none" | "own-package" | "any"
compile-time-effects = ["Files"]
```

`jet audit meta` prints every `Compiler` use, every macro, every rewrite and
every compile-time effect grant. It shows where each one is and which
declarations it touched.

### What this changes in existing rulings

Full control reopens three standing rulings: D-METAMUTATE1 (AST mutation,
message loop and user macros), S26 (compile-time code never creates or
selects types), and the metaprogramming ceiling. The owner has set the goal;
the exact surface still goes on a Tower ballot. The ballot should cover:

- the `Compiler` value and the message set;
- `Code`, `#code`, `#( … )`, `macro` and `#insert`;
- `rewrite_body` and its cross-package rule;
- the manifest keys.

### Questions for the owner

1. Is "one stream, three heights" the right shape, with heights 1 and 2
   implemented as Core library code over height 3?
2. Do you want macros as a `macro` function called with `#name(...)`, or
   another spelling?
3. Should cross-package rewrites be allowed by default (as in Jai) or require
   `allow_rewrite`?
4. Should the manifest default be `compiler = "root"`, or should a fresh
   package start with `compiler = "none"`?
