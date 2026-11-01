# Projects and packages: one clean model for Jet's config files

Status: proposal for card #6368, 2026-10-30. Design only; nothing here is built.
Reads with: `Docs/research/2026-10-30-project-package-config-today.md` (the map of today's files, fields, words and confusions, and the peer comparison).
Owner choices: six ballots, listed in section 11. The workspace-to-project rename is already the owner's ruling and is not balloted.

## Summary

Jet's config is confusing because one file does four jobs and one fact has several homes. This proposal gives every fact exactly one home, chosen by one question.

- **Two kinds of folder.** A **package** is code that others can depend on: it has a `package.jet`. A **project** is the folder you work in, usually the repository root: it owns `project.jet`, the committed `jet.lock` and the disposable `.jet/` folder. A small program is both at once and needs only `package.jet`, or no file at all.
- **One question sorts every fact.** *Would someone who depends on this package need it?* Yes: `package.jet` (name, version, deps, outputs, the effects it needs, its lint rules). No, it is about how we work here: `project.jet` (which packages are in the project, build and test profiles, dev tools, toolchain pin, overrides, team policy). Only for this one run: a command-line flag.
- **Facts are data, work is code.** `package.jet` and `project.jet` hold values only. Code that computes or runs lives in three fixed files beside `package.jet`: `build.jet` holds `fn build`, `test.jet` holds `fn test`, `dev.jet` holds `fn dev`. Nothing is found by searching.
- **Primitives first, wrappers on top.** The data files and the entry code make the same Core values (`Output`, `BuildAction`, `BuildOptions`, `TestProfile`, `Environment`). One primitive call, `b.add(...outputs)`, can add any output; `b.add_executable("app", "run.jet", "util.jet", deps: [log])` is a one-line wrapper over it (the variadic sources form needs D-CALL-VARTAIL1=A; otherwise sources stay a list). Calls use positional arguments for what every call needs, a variadic tail for lists you type inline, and labels for every option.
- **One settings order.** stock → project → package (source markers, `package.jet`, `build.jet`) → flag. A pin anywhere, or a project `forbid`, refuses any conflicting value and names both lines.
- **"Workspace" leaves the package system.** No file, field, flag or message uses it. It keeps only its Jai meaning: one compilation with its own build options.
- **Custom linking (#5009).** A link hook returns a typed build action that Jet runs in place of its own link step for the outputs whose `link` option names it. A write hook may post-process the written file the same way. Both are option rows, so every one can be set, audited, pinned and forbidden. Programs that do not use them pay zero bytes.
- **Migration.** About 420 files change, almost all in `tests/` and `Examples/`. `jet fix` makes every move. A leftover old spelling is an error that names its new home.

## 1. The nouns

| Noun | What it is | Its file | Published? |
|---|---|---|---|
| **module** | a source file or folder of source files | any `.jet` | with its package |
| **package** | the unit you name, version, depend on and publish | `package.jet` (or an inline `package { }` block in a one-file program) | yes |
| **output** | one thing a package makes: one of the nine kinds of D-TARGET-ONE1 (Executable, Service, Library, Check, Bundle, Environment, Image, System, Fleet) | an `outputs:` entry or a `b.add(...)` call | with its package |
| **project** | the folder you work in: its packages, profiles, tools, lock, cache and policy | `project.jet` (optional), `jet.lock`, `.jet/` | never |

Rules that make these exact:

1. **Every command runs in exactly one project.** The project root is the nearest folder upward that holds `project.jet`. With no `project.jet`, it is the folder of the nearest `package.jet` (or the lone file's folder). So a one-package program is a project without writing one.
2. **A package belongs to one project.** A multi-package project lists its packages in `project.jet` `packages:`. A `package.jet` never lists other packages.
3. **Projects do not nest.** A `project.jet` inside another project's folder makes its own project; the outer project cannot list packages inside it. The error names both files.
4. **Nothing in `project.jet` is ever published or read by a consumer.** When your package is someone else's dependency, your `project.jet` does not exist for them. Cargo already behaves this way for profiles ("Profile settings defined in dependencies will be ignored", [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)); Jet makes it visible by giving those facts their own file.

The ladder (philosophy pillar 4) keeps every rung:

| Rung | Files | When |
|---|---|---|
| 0 | `hello.jet` | a script; `jet run hello.jet` (D-ECO14) |
| 1 | `hello.jet` with a `package { }` block | a one-file program that needs a name, deps or effects |
| 2 | `package.jet` + source files (+ `build.jet`, `test.jet`, `dev.jet` when needed) | a package |
| 3 | `project.jet` + packages | custom profiles, dev tools, team policy, or more than one package |

Moving up a rung never rewrites a fact: the same field, with the same text, moves to the next file. `jet fix` and `jet init` do the move (D-LOOSE-GROW1).

## 2. Which file? The beginner's rule

Ask, in order:

1. **Is it only for this run?** → a flag (`jet build --profile release`, `--set port=9090`).
2. **Is it work, meaning code that computes something or runs a tool?** → `build.jet`, `test.jet` or `dev.jet`, beside the `package.jet` it serves.
3. **Would someone who depends on or installs this package need it?** → `package.jet`. Otherwise → `project.jet`.

That is the whole rule. Each line below is "X goes in F because …".

| Fact | File | Because |
|---|---|---|
| name, version, edition, license, description, repository | `package.jet` | it identifies the package to everyone who uses it |
| `jet: "0.4"` (the toolchain range the package needs) | `package.jet` | a consumer's toolchain must satisfy it |
| a dependency, `deps: { http: "1.4" }` | `package.jet` | the package does not compile without it |
| "every package here uses the same `http` version" | `project.jet` `catalog:`, and `deps: { http: project }` in each package | it is a team agreement; each package opts in explicitly |
| an output, `outputs: { app: .Executable{ entry: run } }` | `package.jet` | it is what the package makes |
| an output's own needs, such as `os: web` for a web build or a C library to link | `package.jet`, in that output's `options:` | the output is wrong without it, whoever builds it |
| which output bare `jet run` picks, `defaults: { run: cli }` | `package.jet` | it is a choice among the package's own outputs |
| effects the code uses, `authority: { allow: [IO] }`, and grants to its deps | `package.jet` | a consumer must approve them |
| effects `build.jet` needs (today `build: { allow: … }`) | `package.jet` `authority: { build: [FS, Exec] }` | a consumer runs that build code too |
| lint and code rules for this package (`policy:`, `boundaries:`) | `package.jet` | they describe this package's code |
| a test suite that is plain data (a golden folder), `suites:` | `package.jet` | the suite is part of the package, like an output |
| a test suite that needs code (a fuzz driver) | `test.jet`, `t.suite(...)` | it is work |
| generated code, a code-made output, a custom tool step | `build.jet` | it is work |
| dev-loop behaviour of one service (port, watch, reload) | `dev.jet`, or that output's `options:` | it belongs to that output |
| the packages in this repo, `packages:` | `project.jet` | a consumer never sees your repo layout |
| `version`, `edition`, `license` that every package shares | `project.jet` | a package that leaves one out takes the project's (D-CORE-HEADER1, now written in one place) |
| build profiles, `release: BuildOptions{ optimize: full }` | `project.jet` `profiles:` | how you choose to build; a consumer's build never reads yours |
| test profiles, `quick: TestProfile{ suites: [tests], time_limit: 60s }` | `project.jet` `profiles:` | how you choose to test; same namespace as build profiles (D-TEST-PROFILE1) |
| which profile bare `jet build` / `jet test` uses | `project.jet` `defaults:` | a team choice |
| dev tools, shells and local services (today `env.jet`) | `project.jet` `environments:` | they serve the people working here |
| the exact toolchain the team builds with | `project.jet` `toolchain:`; the resolved version goes in `jet.lock` | a team choice; `package.jet` `jet:` is only the range consumers need |
| a local replacement for a dependency (overlay, patch) | `project.jet` `overlays:` | it must never leak to consumers |
| team policy: forbid an option or effect, pin a value, allow a dependency's hooks | `project.jet` `policy:` | it is a ceiling over every package here |
| the resolved dependency graph and toolchain | `jet.lock` (written by Jet, committed) | D-LOCKFILE1 |
| build output, cache, generated files | `.jet/` (disposable, git-ignored) | D-WORKSPACE-CMD1 |
| `optimize: size` just this once | a flag | rule 1 |

`policy:` exists at both levels with the same shape: the package's block governs that package's code, the project's block is a ceiling over every package. The stricter one wins, and a conflict names both lines. `defaults:` also exists at both levels, but each level picks only among its own things: a package picks among its outputs, a project picks a package and profiles.

Large teams that want to split `project.jet` do not need a `configs:` mechanism. Both data files are Jet values, so a field can take a value from an ordinary module of the project: `environments: team_envs.all`. The special `Config{}` records and `configs:` lists retire.

## 3. Code files: `build.jet`, `test.jet`, `dev.jet`

- `fn build(b: Build)` lives in `build.jet`, `fn test(t: Test)` in `test.jet`, `fn dev(d: Dev)` in `dev.jet`, each beside its `package.jet`. One of each per package at most (D-CONF-ENTRY1's count rule, kept).
- The only other home is a program with no `package.jet` (rungs 0 and 1): there, everything is in the one file.
- `project.jet` has no entry functions. A project builds by building its packages, in dependency order (D-WORKSPACE-CMD1). A thing made from several packages, such as a release bundle, is an output of a small package that depends on them.
- `package.jet` and `project.jet` hold values only, so `jet env`, editors and policy checks read them without running project code (D-JPK-DEVCOMPOSE1).

Why fixed homes, when D-CONF-ENTRY1 allowed any file: `fn build`, `fn test` and `fn dev` run on the developer's machine, before the program exists, with powers such as `Exec` and `FS`. A reviewer must be able to see from a diff that build-time code changed. Cargo (`build.rs`), Zig (`build.zig`), Gradle (`build.gradle.kts`) and Bazel (`BUILD`) all give build code one name for this reason. It also removes discovery: today `Examples/features/tooling/build_entry_discovery` needs a README to say where its build lives.

`jet new --template overrides` stubs change from "replace the stock default" to the shape D-ENTRY-SHAPE1 ratified: the entry sets values, then the stock work runs once.

## 4. How settings combine

One list replaces the three in force today (D-CONF-MERGE1, D-SETTING-ORDER1, and D-BUILD-REUSESET1's root layer). Later wins:

1. **stock**: Jet's defaults and the stock profiles;
2. **project**: `project.jet`, including the profile selected by `--profile` or `defaults:`;
3. **package**: source markers, then `package.jet`, then `build.jet`, for that package's outputs. Inside this layer an output's own `options:` beat package-wide values (D-SETTING-ORDER1);
4. **deployment**: environment, system and fleet layers, for Image, System and Fleet outputs only (D-CONF-MERGE1);
5. **flag**: the command line.

Two rules sit across the list:

- **Pin.** `b.pin(...)`, a `pin:` in `package.jet`, or a `pin:` in `project.jet` policy fixes a value. Any other layer that sets a different value is an error naming both lines (D-JAI-BUILDOPT1). A package pins what its output cannot work without; a project pins what the team requires.
- **Forbid.** `project.jet` `policy: { forbid: [...] }` names options or effects that no layer in the project may set. The error names the policy line (the owner's build-options ruling: project or package policy can forbid any option).

Why the project sits under the package: the project sets the team's defaults and profiles, and a package's stated need for one of its outputs must not be overridden silently by a team default. When the project must win, it pins, and the conflict becomes a visible error instead of a silent override. This is D-BUILD-REUSESET1's order, with the profile moved into the project.

`jet explain setting optimize --output app` prints every layer that set the value, with file and line.

## 5. Composition: primitives first, wrappers on top

The owner's rule for every API applies here first: a strong set of primitives that can express everything, then high-quality wrappers for the common cases. In config this means:

### 5.1 One set of values

Data files and entry code build the same Core values, so a fact moves between them unchanged:

| Value | Written in data as | Made in code by |
|---|---|---|
| `Output` (nine kinds) | `outputs: { app: .Executable{ entry: run } }` | `b.add(.Executable{ name: "app", entry: run })` |
| `BuildOptions` | `profiles: { release: BuildOptions{ optimize: full } }` | `b.options()`, `b.profile("release")` |
| `TestProfile` | `profiles: { quick: TestProfile{ suites: [tests] } }` | `t.profile("quick")` |
| `Environment` | `environments: { dev: Environment{ tools: ["git"] } }` | (data only; `jet env` never runs code) |
| `BuildAction` | `options: { link: wasm.link }` (a function that returns one) | `b.action("stamp", run: [...], inputs: [...], outputs: [...])` |
| `Generated` | — | `b.generate("table") { ... }` (D-BUILD-GENVALUE1) |

### 5.2 The primitives

- `b.add(outputs: ...Output) -> [Output]`: add any outputs. A library can return `Output` values from ordinary functions and the caller adds them: `b.add(...web.targets("game", "editor"))`. This is the call every `add_*` wrapper reduces to.
- `b.output(name) -> &Output`: change a data-made or code-made output until the plan is sealed (D-TARGET-ONE1, D-JAI-CONFIG1).
- `b.action(name, *, run:, inputs:, outputs:, allow:, …)` → `BuildAction`: every field of a declared build action, by label (D-BUILD-CORE1, D-BUILDACTION1).
- `b.generate(name) { … }` → `Generated`: the generated code as a value. It can be passed anywhere a source is, so nobody types `.jet/generated/...` paths again.
- `b.pin(...)` and the generated setting methods, one per option row (D-SETTING-ORDER1).

### 5.3 The wrappers

Each wrapper is an ordinary Core function whose body is one primitive call. It has no power the primitive lacks, and its doc comment shows the expansion. `jet explain` prints it.

```jet
// Core/Build: a wrapper is one line over the primitive
fn add_executable(b: &Build, name: String, sources: ...Source, *,
                  deps: [Package]{[]}, options: BuildOptions{}, entry: Entry?{None}) -> Output BuildError! {
    b.add(.Executable{ name: name, sources: sources, deps: deps, options: options, entry: entry })[0]
}
```

The data files are the zero-code wrapper: an `outputs:` entry is the literal form of the value `b.add` takes. The stock defaults are the zero-file wrapper: no `package.jet` means `run.jet` is the program.

### 5.4 Argument shape rules for config and build APIs

These follow Jet's call rules (S61, D-CALLPOS1, D-VARIADIC1) and use them on purpose:

1. **What every call needs is positional:** the name, then the main input.
2. **A list you would type inline is a variadic tail:** sources, command arguments, suites. `b.add_executable("app", "run.jet", "util.jet")`, not `["run.jet", "util.jet"]`. Spread passes an existing list: `b.add_executable("app", ...find("src/**/*.jet"))`.
3. **Every option is label-only with a default:** `deps:`, `options:`, `entry:`, `link:`. Callers never count positions.
4. **No arity families.** One signature per method, never "5 or 7 to 15 positional arguments".
5. **A wrapper is one primitive call** (5.3).

Today the build handle breaks all five. The checker rejects every label with E0764 "`Build.{method}` takes positional arguments … the build method contract does not declare labels or variadic slots" (`Compiler/JetSema/Source/Sema/Calls/Build.jet:148`). `b.action` takes "5 or 7 to 15" positional arguments, and `add_*` takes "3 to 7" (`Build.jet:94-96`). D-BUILD-CORE1 already ratified labels; this section adds the variadic tail and the primitive.

Rule 2 needs one small language change. Jet requires a variadic parameter to be last (E1310), and a variadic after `*` becomes label-only. So `fn f(name: String, sources: ...String, *, deps: [String]{[]})` is E1310 today, and `fn g(name: String, *, deps: [String]{[]}, sources: ...String)` makes the call `g("app", "run.jet", deps: ["core"])` an E0769 (probed on dev; section 12). Python ([PEP 3102](https://peps.python.org/pep-3102/)), Swift ([Functions: variadic parameters](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/#Variadic-Parameters)) and Kotlin ([vararg](https://kotlinlang.org/docs/functions.html#variable-number-of-arguments-varargs)) all allow named parameters after a variadic one. Ballot D-CALL-VARTAIL1 asks whether label-only parameters may follow a variadic tail.

## 6. Before and after: four real projects

### 6.1 Hello

Before (`jet new hello` today, `Host.jet:1529-1531,1607,1618`):

```text
hello/package.jet   name, version, jet, description: "", license, repository: "",
                    authority: { allow: [IO, Mem.Alloc] }, deps: {}
hello/run.jet       fn run() { print("hello") }
hello/.jet/lock     written, and git-ignored by .gitignore
hello/.gitignore    .jet/build/  .jet/lock  .jet/cache/
```

After:

```text
hello/package.jet   name: "hello"
                    version: "0.1.0"
                    authority: { allow: [IO] }
hello/run.jet       fn run() { print("hello") }
hello/jet.lock      written by Jet, committed (D-LOCKFILE1)
hello/.gitignore    .jet/
```

No `project.jet`: the package folder is the project. Smaller still, `hello.jet` alone runs with `jet run hello.jet`. Changes: the lock is committed at the root, as ratified; empty placeholder fields are not written.

### 6.2 A library with tests

Before (D-TEST-PROFILE1's ratified shape, which today's parser rejects with E1206):

```jet
// textkit/package.jet
name: "textkit"
version: "0.3.0"
edition: "2026"
license: "MIT"
deps: { unicode: { git: "https://example.com/unicode", tag: "v1.2.0" } }
outputs: { textkit: .Library{ entry: textkit } }
build: { release: Build{ optimize: full } }
test: {
    quick: TestProfile{ suites: [tests], time_limit: 60s }
    full: TestProfile{ suites: [tests, golden], optimize: [none, full] }
}
suites: { golden: TestSuite{ golden_corpus: "tests/golden" } }
defaults: { test: quick }

// textkit/test.jet
fn test(t: Test) {
    t.suite("fuzz", fuzz_driver(seed: 7))
    t.run()
}
```

After:

```jet
// textkit/package.jet: what textkit is, needs and makes
name: "textkit"
version: "0.3.0"
edition: "2026"
license: "MIT"
deps: { unicode: { git: "https://example.com/unicode", tag: "v1.2.0" } }
outputs: { textkit: .Library{ entry: textkit } }
suites: { golden: TestSuite{ golden_corpus: "tests/golden" } }

// textkit/project.jet: how this team builds and tests it
profiles: {
    release: BuildOptions{ optimize: full }
    quick: TestProfile{ suites: [tests], time_limit: 60s }
    full: TestProfile{ suites: [tests, golden, fuzz], optimize: [none, full] }
}
defaults: { test: quick }

// textkit/test.jet: only the suite that needs code
fn test(t: Test) {
    t.suite("fuzz", fuzz_driver(seed: 7))
    t.run()
}
```

What a consumer of `textkit` sees is now exactly `package.jet`. The profiles stop looking like part of the library. `Build{ … }` becomes `BuildOptions{ … }` (D-ENTRY-NAMES1). A team that never customizes profiles has no `project.jet`.

### 6.3 A multi-package project: the Jet compiler

Before:

```jet
// env.jet (repo root): two of its environments
module env.compiler {
    sources: { default: NixOS/nixpkgs/nixos-unstable@github }
    packages: [ default.[ cargo, rustc, gcc, clang, lld, git, ripgrep, … ] ]
    prompt: "jet-compiler"
}

// Compiler/package.jet: a package named "compiler" that is also the root of 13 others
name: "compiler"
version: "0.0.1"
edition: "2026"
members: [ "JetFoundation", "JetLexer", …, "JetCli" ]

// Compiler/build.jet: makes the jet executable out of another package's file
fn build(b: Build) {
    jet :: b.add_executable("jet", ["JetCli/run.jet"], [])
    b.default(jet)
}

// Compiler/JetCli/package.jet
name: "jet_cli"
version: "0.0.1"
edition: "2026"
deps: { jet_foundation: ../JetFoundation, … }
authority: { allow: [Env, Exec, …] }
```

After:

```jet
// project.jet (repo root): the repo is the project
packages: [
    ...find("Compiler/*"),
    ...find("Jetpack/*"),
]
version: "0.0.1"
edition: "2026"
environments: {
    compiler: Environment{
        sources: { default: NixOS/nixpkgs/nixos-unstable@github }
        tools: [ default.[ cargo, rustc, gcc, clang, lld, git, ripgrep, … ] ]
        prompt: "jet-compiler"
    }
}

// Compiler/JetCli/package.jet: the package that owns the code owns the output
name: "jet_cli"
deps: { jet_foundation: ../JetFoundation, … }
outputs: { jet: .Executable{ entry: run } }
authority: { allow: [Env, Exec, …] }
```

`env.jet`, `Compiler/package.jet` and `Compiler/build.jet` are gone. `jet build` at the root builds every package; `jet build jet` builds the one output; `jet run jet -- --version` runs it. Members take `version` and `edition` from the project. The package named "compiler", which had no code of its own, no longer exists. That ends the case where a named root silently skips its own code (bug B1, section 12).

### 6.4 A project with build-time code generation

Before (`Examples/features/tooling/build_write_generated`):

```jet
// package.jet
name: "build_write_generated_example"
version: "0.1.0"
edition: "2026"
build: {
    inspect: Build{ write_generated: on },
}

// build.jet
fn build(b: Build) {
    b.generate("table") {
        fn table_size() -> Int { 5 }
    }
    app :: b.add_executable("run", ["run.jet", ".jet/generated/build_write_generated_example/table.jet"], [])
    b.default(app)
}
```

After:

```jet
// package.jet
name: "build_write_generated_example"
version: "0.1.0"
edition: "2026"

// project.jet
profiles: { inspect: BuildOptions{ write_generated: on } }

// build.jet
fn build(b: Build) {
    table :: b.generate("table") {
        fn table_size() -> Int { 5 }
    }
    app :: b.add_executable("run", "run.jet", table)
    b.default(app)
}

// The same build written with the primitive only:
fn build(b: Build) {
    table :: b.generate("table") {
        fn table_size() -> Int { 5 }
    }
    b.default(b.add(.Executable{ name: "run", sources: ["run.jet", table] })[0])
}
```

The generated code is a value, so the build no longer names a path inside `.jet/`, and the cache layout can change without breaking it. The trailing `[]` (an empty positional action list) is gone. The two spellings build the identical plan; `jet inspect build` cannot tell them apart.

## 7. Custom link and post-link steps (#5009)

D-JAI-EVENTS1=A (2026-10-17) added the stages `build.link(LinkInputs) -> Artifact`, `build.before_write(Artifact) -> replacement` and `build.after_write(Artifact) -> findings`, run as build actions. The owner asked for a ballot on whether a typed build action may replace one output's link step and post-process the written file. In this model the answer fits in three pieces:

1. **The hook returns a typed action; Jet runs it.** A `#On(build.link)` function receives `LinkInputs`: the object files, libraries, link arguments, the output path, and `default`, the exact `BuildAction` Jet would run. It returns a `BuildAction` (D-BUILDACTION1): a declared command with inputs, outputs and `Exec`/`FS` capabilities. Jet runs it in place of its own link step, checks that it wrote the declared file, and records it. A `#On(build.before_write)` function returns a `BuildAction` that turns the linked file into the final one (strip, compress, patch). `#On(build.after_write)` stays read-only and returns findings. Hooks never write files themselves, so every byte that reaches the output came from a declared, cached, recorded action.
2. **Which outputs use it is an option row.** `link: default | <hook>` and `post_link: [<hook>, …]` are rows of the one option table, so each has a flag (`--link=default`), a `package.jet` field (an output's `options:`), a `project.jet` profile field and a handle method (`b.output("game").link(wasm.link)`). Owner directive #4913 holds: no option without all three. Experts get full control: `LinkInputs.default` is the stock command, so a hook can edit one argument and keep everything else.
3. **Policy and audit.** `project.jet` or `package.jet` `policy: { forbid: [link, post_link] }` refuses both. A dependency's hook is held until `jet trust grant package-hook:<package>#<version>` records an exact grant in `policy.trust`, the grant every dependency hook already needs (D-HOOK-STAGES1, D-JPK-BUILDSCRIPT1). `jet inspect build` shows, per output, the link action's argv, its capabilities, and the line that chose it. A failed or missing link output is one registered build diagnostic naming the output, the hook and the setting line.

**Binary size (D-SIZE-PAYUSE1).** Hooks run at build time and nothing is linked into the program, so a program that does not use them is byte-for-byte unchanged. `LinkInputs.default` is the stock link command with all of Jet's link arguments, so a hook that edits one argument keeps the rest, including the ones that keep the file small. The build record reports the artifact size after the last post-link step, so size reports measure the file that actually ships.

```jet
// wasmtools/link.jet: a hook package
#On(build.link)
fn link(i: build.LinkInputs) -> BuildAction {
    build.action("wasm-ld",
        run: ["wasm-ld", "--no-entry", "--export-all", ...i.objects, "-o", i.output],
        inputs: i.objects,
        outputs: [i.output],
        allow: [.Exec])
}

#On(build.before_write)
fn optimize(a: build.Artifact) -> BuildAction {
    build.action("wasm-opt", run: ["wasm-opt", "-Oz", a.path, "-o", a.next], inputs: [a.path], outputs: [a.next], allow: [.Exec])
}

// game/package.jet: only the web output links with wasm-ld
outputs: {
    game_web: .Executable{ entry: run, options: { os: web, link: wasmtools.link, post_link: [wasmtools.optimize] } }
}

// a dependency's hook is held until granted once, recorded in policy.trust
$ jet trust grant package-hook:wasmtools#0.3
```

Ballot D-LINK-STEP1 has the options and the same program under each.

## 8. Words

- **project**: the folder you work in (section 1). It replaces "workspace" in the package system, in CLI help and diagnostics, and in Canvas's `"mode": "workspace"` (`Docs/spec/reference/canvas-workspace-architecture.md`).
- **workspace**: only its Jai meaning, one compilation with its own build options (`compiler_create_workspace`). In Jet that is one output built under one set of options (D-JAI-CONFIG1). No file, field or flag uses the word.
- **Jetspace** (D-ECO-ROOTNAME2=D) retires: "the project" names the checked graph of a project's packages.
- **packages:** is the `project.jet` field that lists packages (it replaces `members:`). The old `package.jet` `packages:` map of output kinds folds into `outputs:`.
- **profile**: a named `BuildOptions` or `TestProfile`, written only in `project.jet` `profiles:`. A platform preset in `targets:` says `platform:`, not `profile:`.
- **build**: the command, the `Build` handle and `build.jet`. The `build:` field retires (profiles moved), and so does `Build{ … }` as a record (D-ENTRY-NAMES1).
- **target**: the platform only (D-TARGET-ONE1).

Ballot D-PROJECT-WORDS1 settles the words that follow from the rename.

## 9. Migration and the `jet fix` story

Counts are `git ls-files` at `e784bb1ffe`.

| Today | After | Files | `jet fix` |
|---|---|---|---|
| `workspace.jet` (`module workspace { members: X }`, overlays) | `project.jet` (`packages: X`, `overlays:`) | 22 | rename and reshape; a `fn` inside moves to its package's `build.jet`/`run.jet` |
| `env.jet` (`module env.<n> { packages: … }`) | `project.jet` `environments: { <n>: Environment{ tools: … } }` | 159 | merge; 154 sit where no `package.jet` exists and become `project.jet` files, not nameless packages |
| `members:` in `package.jet` | `project.jet` `packages:` | 5 | move; a named root with no code of its own is deleted after its outputs move to the package that owns the code |
| `build:` profiles in `package.jet` (35) and inline blocks (3) | `project.jet` `profiles:`; `build: { allow: }` → `authority: { build: }` | 38 | move; `Build{}` → `BuildOptions{}` |
| profile keys of `defaults:` | `project.jet` `defaults:` | ≤ 18 | split by key |
| `packages:` output map in `package.jet` | `outputs:` | 80 | rewrite each entry as an output value |
| `config.jet`, `configs:` files, `Config{}` records | the field's home in `package.jet` or `project.jet` | 32 + 5 | inline by field |
| `fn build`/`fn test`/`fn dev` outside its home, in a package with `package.jet` | `build.jet` / `test.jet` / `dev.jet` | 38 (21 in `run.jet`, 8 in `main.jet`, 3 in `package.jet`, 2 in `tools/build_entry.jet`, 4 others) | move the function; same package, so names still resolve |
| committed `.jet/lock` | `jet.lock` | 22 | move; `.gitignore` becomes `.jet/` |
| `pkg.jet` | `package.jet` | 5 | rename (already retired) |
| `jet new` templates and stubs | the new shapes | code | rewrite (`Host.jet:1513-1531`) |

Total: about 420 files, almost all fixtures under `tests/` and `Examples/`. Core's 604 inline `package { }` blocks are unchanged except 3 with `build:` and 3 with `configs:`. The 29 entry functions already in their home and the 104 in one-file programs do not move.

`jet fix` is mechanical for every row: each rewrite moves text between files without changing its meaning, and `jet fix --dry-run` prints the moves. After the cutover, every old spelling is a registered error that names its new home and the fix, for example: "`members:` belongs in project.jet as `packages:`. Run `jet fix`." There are no aliases and no fallback reads (clean cutover).

## 10. What this amends

| Ruling | Change |
|---|---|
| D-MANIFEST-ONE1=A (10-30) | Members and environments move to `project.jet`, not `package.jet`; a nameless `package.jet` is no longer a root (D-PROJECT-ROOT1) |
| D-TEST-PROFILE1=A | `test:` profiles go in `project.jet` `profiles:` with build profiles; `suites:` stays in `package.jet` |
| D-BUILD-REUSESET1=A, D-SETTING-ORDER1=A, D-CONF-MERGE1=A | one settings order (section 4) |
| D-CONF-ENTRY1=A, D-BUILDSCOPE1=A | entry functions live in `build.jet`/`test.jet`/`dev.jet` (D-ENTRY-HOME1) |
| D-BUILD-CORE1=A | adds `b.add(...outputs)`, variadic sources, `Generated` as a source (D-BUILD-PRIM1) |
| D-VARIADIC1, S61 | label-only parameters may follow a variadic (D-CALL-VARTAIL1) |
| D-JAI-EVENTS1=A, D-HOOK-STAGES1=A, D-BUILDACTION1=A, D-JAI-CONFIG1=A | link and before-write hooks return actions selected by option rows (D-LINK-STEP1) |
| D-ECO-ROOTNAME1=I, D-ECO-ROOTNAME2=D, D-WORKSPACE1=B | "project" is the root noun; Jetspace retires (D-PROJECT-WORDS1) |
| D-CORE-HEADER1=A | unchanged in effect; the shared fields are written in `project.jet` |
| D-LOCKFILE1=A, D-WORKSPACE-CMD1=A | unchanged; `jet new` is fixed to follow them |

One existing conflict this proposal does not settle: D-OPTFLAG1=A removed `--optimize`, while D-SETTING-ORDER1=A and owner directive #4913 require a flag for every option row. Main should route it.

## 11. Owner choices

Each is a Tower ballot on card #6368, except D-LINK-STEP1, which is on card #5009.

| Ballot | Question | Recommended |
|---|---|---|
| D-PROJECT-ROOT1 | Where do project facts live after the rename? | A: `project.jet`, sorted by the one question; no file means one-package project |
| D-PROJECT-WORDS1 | Which words follow from the rename? | A: project, `packages:`, Jetspace retired, workspace only in the Jai sense |
| D-ENTRY-HOME1 | Where do `fn build`, `fn test` and `fn dev` live? | A: `build.jet`, `test.jet`, `dev.jet` only |
| D-BUILD-PRIM1 | What are the build API's primitives and wrappers? | A: `b.add(...outputs)` primitive, one-line wrappers, generated code as a source |
| D-CALL-VARTAIL1 | May label-only parameters follow a variadic tail? | A: yes, after `*` |
| D-LINK-STEP1 (#5009) | How may a build replace an output's link step and post-process the file? | A: hooks return typed actions, selected by option rows |

## 12. Bugs and drift found while probing

Probes ran on dev `e784bb1ffe` with the base compiler. Repros are in the research doc, section 6.

- **B1. A named root with members skips its own code.** Root `package.jet` with `name: "p3"`, `members: ["a"]` and an `outputs:` app; root `run.jet` refers to an undefined name; member `a/run.jet` prints "hi". `jet check .` and bare `jet check` print "ok: …/a/run.jet has no problems"; `jet run` prints `hi`. Only `jet check run.jet` reports E0107. Expected: check every package, including the root, and run the root's own default.
- **B2. E1206 for `test:` explains the wrong thing.** Its "Why" line says "Identity is bare `name:` and `version:` … there is no `identity:` or `payload:` wrapper".
- **B3. One mistake, two codes.** Two `fn build` in one package (`build.jet` and `tools/build.jet`): `jet check .` gives E0105 "Imported names share the module namespace", `jet build` gives E3520 "Two build entries for the package".
- **B4. Members listed twice are accepted.** A root with `members: ["a"]` in `package.jet` and `module workspace { members: ["a"] }` in `workspace.jet` checks clean.
- **Drift D1.** `jet new` writes `.jet/lock` and git-ignores it, against D-LOCKFILE1 (committed root `jet.lock`).
- **Drift D2.** `jet new --template overrides` stubs say "replace the stock default", against D-ENTRY-SHAPE1.
- **Drift D3.** Six `.jet/last-run/*` files are committed under `Examples/features/tooling/build_entry_discovery`, though `.jet/` is disposable.
- **Language gap.** A variadic cannot be followed by label-only parameters (E1310), and a variadic after `*` is label-only (E0769). Not a bug; D-CALL-VARTAIL1 asks the owner.

## Sources

Peer facts and citations are in the research doc, section 7. Additional: [PEP 3102 keyword-only arguments](https://peps.python.org/pep-3102/); [Swift functions, variadic parameters](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/#Variadic-Parameters); [Kotlin varargs](https://kotlinlang.org/docs/functions.html#variable-number-of-arguments-varargs). Repo: `Jetpack/PackageModel/Source/Manifest/PackageParse.jet`, `Compiler/JetSema/Source/Sema/Calls/Build.jet`, `Compiler/JetCli/Source/Cli/{Entry,Host,Request}.jet`, `Docs/audits/jai-parity-2026-10-16.md` §D, `Docs/proposals/build-programs-and-compiler-events-2026-10-22.md`.
