# `env.jet`: executable entry or computed declarative value?

Date: 2026-08-24

Scope: decision audit only. This report does not change behavior, edit `env.jet`, or create work.

## Recommendation

Keep the environment declarative at its boundary, but let ordinary pure Jet code compute it. The canonical form should be one typed `Environment` value, with literals for the common case and ordinary functions, structs, methods, and typed builders available to produce that value. Do not make `jet env` execute a general command entry named `fn env(...)`. Follow the ratified file-root direction and home the value in `package.jet`, rather than preserving `env.jet` as a second reserved root.

In short: make environment configuration code that produces data, not an environment command.

This is a real choice, not a compromise by omission. It accepts the useful part of the proposal—reuse, conditions, builders, refactoring, and one Jet programming model—while keeping a hard boundary at the result. The environment remains inspectable, hashable, bounded, and safe to evaluate before trust is granted.

The strongest argument against this recommendation is coverage. A typed `Environment` model must gain honest forms for wrappers, derived store paths, platform library paths, activation actions, and profiles. Until it does, an expert can express more in the flake than in Jet. A general `fn env` could expose that power sooner and with less schema design.

## The choice is narrower than it first appears

The current form is not merely “advanced TOML.” Except for package-list sugar, its fields already pass through Jet's pure comptime evaluator and may use `if`, immutable values, pure helpers, and other deterministic expressions (`crates/jet-env-model/src/ModuleEval/mod.rs:6-19`; `docs/spec/syntax-decisions.md:2631-2640`). The evaluator also supports explicit typed environment reads, records those reads, and installs them as comptime globals (`crates/jet-env-model/src/ModuleEval/Eval.rs:69-150`; `docs/spec/syntax-decisions.md:7330`). It has a finite step budget and reports `E0952` when the budget is exhausted (`crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,300-328`).

The real decision is therefore:

| Option | Boundary | Consequence |
|---|---|---|
| Keep today's structural surface | A typed plan, but with package sugar and incomplete fields | Safest and easiest to edit, but cannot replace this repository's flake |
| Add a general `fn env(...)` command | Any behavior that the entry can perform | Maximum immediate power, but discovery, trust, caching, and editing all become program execution problems |
| Let pure Jet code produce one typed `Environment` value | Bounded evaluation followed by a complete, inspectable value | Gains the proposal's useful programming features while preserving a declarative contract |

The third option is recommended. A new restricted configuration language would add another mechanism even though Jet already has a pure, fuel-bounded comptime evaluator (`crates/jet-comptime/src/Comptime/mod.rs:1-16`).

## What exists now

The root file declares `module env.dev`, one named Nix source, 20 package references, and a prompt (`env.jet:7-17`). Its own header says the intended scope is the default compiler shell and admits three unmapped groups: the `shellHook`, the `jetDev` and `jetpackDev` wrappers, and the entire opt-in full shell (`env.jet:1-6`).

Jet Foundation defines `env.jet`, the `env` namespace, an `Environment` output kind, and an environment-output entry field (`crates/jet-foundation/src/Syntax/jetpack_config.rs:137-142,249-284,394-402`). The declared environment surface includes prompt, dotenv, unset, enter hooks, checks, reload policy, Git hooks, formatter, presets, languages, files, and integrations (`crates/jet-foundation/src/Syntax/jetpack_config.rs:585-651`). The evaluated plan is already a typed structure containing source identity, explicit environment reads, packages, adapters, prompts, systems, images, fleets, VM tests, services, secrets, lifecycle actions, presets, languages, files, integrations, profiles, active environment, and provenance (`crates/jet-env-model/src/ModuleEval/Types.rs:532-634`).

An environment is consumed as data. Jetpack finds the nearest `env.jet`, evaluates the typed module surface, or falls back to a legacy `pkg.*` directive reader (`crates/jetpack/src/CLI/realize.rs:632-692`). It expands the resulting facts into package references and a `RunPlan` (`crates/jetpack/src/CLI/realize.rs:605-630,695-794`). `jet enter`, `jet dev`, and related paths compose that plan, grant trust if required, run lifecycle hooks, and then start the requested command or shell (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`). The code explicitly states that `jet env` never runs a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1498`).

This boundary matches the ratified architecture. `D-ECO-ENV1=A` says one typed `Environment` output is the source of truth and that imperative actions are capability-scoped and audited (`docs/spec/syntax-decisions.md:4397-4407`). `D-ECO-FILEROOT1=A` says `package.jet` is the one reserved ecosystem file and that environment, workspace, and config facts fold into it through one migration epoch (`docs/spec/syntax-decisions.md:6090-6108`). `D-CONF-SPLIT1=A` separates declared facts from `fn build` actions and says computed contributions must remain recorded and explainable (`docs/spec/syntax-decisions.md:6933-6941`).

## Concrete parity gaps

The root file's package list is 20 references (`env.jet:9-15`). The default flake shell has 24 package expressions (`flake.nix:110-136`), so the current file names 20 of 24, or 83%. The full shell has 50 package expressions (`flake.nix:159-246`), so it names 20 of 50, or 40%. These are expression counts, not claims about the number of transitive Nix packages: some expressions are wrappers, derived tools, or platform-conditioned values.

The 30 full-shell expressions missing from `env.jet` are `rustup`, `rustfmt`, `gnat`, `fpc`, `dart`, `powershell`, `gfortran`, the binary output of `gnucobol`, `go`, `jdk`, `dotnet-sdk_8`, `tcl`, `lua5_4`, `ruby`, `php`, the derived `jetR`, `octave`, `qemu`, `python3`, `wasmtime`, `emscripten`, `lldb`, `jetDev`, `jetpackDev`, `raylib`, and the Linux-only `chromium`, `firefox`, `geckodriver`, `gtk4`, and `bubblewrap` (`flake.nix:162-246`).

| Gap in the repository today | Evidence | Fundamental to declarative data? | What closes it without arbitrary execution? |
|---|---|---|---|
| Four default-shell package expressions are absent: `rustfmt`, `python3`, `jetDev`, and `jetpackDev` | Compare `env.jet:9-15` with `flake.nix:111-136` | No. Two are ordinary missing package references; two are derived wrappers | Add ordinary packages and a typed wrapper/tool output |
| Thirty full-shell expressions are absent, including language toolchains, VM/web/debug tools, Raylib, and five Linux-only browser/graphics tools | Compare `env.jet:9-15` with `flake.nix:162-246` | Mostly no. The ordinary tools are missing profile data; the Linux subset needs a target predicate | Add a named full profile and pure target-conditioned package selection |
| Package selection itself cannot use the same computed expressions as other fields | Package lists use static-text sugar while other fields use comptime evaluation (`crates/jet-env-model/src/ModuleEval/mod.rs:6-15`) | No. This is a surface limitation | Make packages an ordinary typed computed field |
| `JET_ROOT` is found from Git or the current directory and exported | `flake.nix:139-144,249-254` | No | Provide a typed project-root value and environment-variable projection |
| `TZDIR` points to the selected `tzdata` store output | `flake.nix:35,145,255` | No | Let a package output expose a typed path and map it to an environment variable |
| Linux adds Vulkan—and in the full shell Raylib—library paths | `flake.nix:146-148,256-265` | No | Use a target-conditioned library-path builder over typed package outputs |
| The shell runs `clean-nix-tmp` once and guards it with `JET_NIX_TMP_CLEANED` | `flake.nix:150-153,267-270` | No, but it is an action rather than a fact | Represent it as a named, trust-gated, once-per-activation lifecycle task |
| The shell sets `JET_ENV_DISABLE=1` to prevent recursive environment activation | `flake.nix:154-155,271-272` | No | Add a normal typed environment variable |
| `jetDev` and `jetpackDev` find the repo root, prepend runtime tools to `PATH`, check that a debug binary exists, print a tailored failure, and execute it | `flake.nix:62-90` | No, but a raw string field would be too weak | Define typed executable wrappers with a project-root input, path contribution, checked executable, and diagnostic |
| The full shell prints an activation banner | `flake.nix:274-281` | No | Use a presentation hook or shell message field; it does not require general computation |
| There is no Jet equivalent for the full shell as one selectable profile | `env.jet:2-6`; `flake.nix:159-283` | No | Make the complete environment value support named profiles or contributions |
| Flake bridging cannot map `shellHook`, multiple named dev shells, or the `buildInputs`/`nativeBuildInputs` distinction | `crates/jetpack/src/Bridge.rs:412-428` | No. These are admitted translation/schema gaps | Add typed lifecycle, profile, and build/host dependency distinctions, then keep a loss report for genuinely foreign behavior |

None of the admitted gaps proves that the environment itself must be a general program. Every gap is either a missing typed fact, a missing pure computation over facts, or an activation action that should occur only after trust. The wrappers are the hardest case because they combine derived paths, control flow, a diagnostic, and process execution. Even there, the flake is constructing an executable artifact; it is not executing the wrapper while the environment is being discovered (`flake.nix:62-90`). A typed wrapper output preserves that distinction.

The full shell is also not one isolated missing field. It adds 30 package expressions beyond the root `env.jet`, platform-conditioned packages, several derived tools, library-path composition, activation state, and presentation (`flake.nix:159-283`). Reaching parity requires a complete environment model, not merely adding `shellHook: String`.

## What `fn env(...)` would buy

| Claimed benefit | Is it needed here? | Current evidence and assessment |
|---|---|---|
| Platform conditions | Yes | The flake selects five browser/graphics expressions only on Linux and changes the library path by platform (`flake.nix:231-246,256-265`). Pure target input plus `if` is enough |
| Reuse across members | Plausible and aligned | Typed environment modules are discovered and merged in one pass (`crates/jet-env-model/src/ModuleEval/Source.rs:341-395`). Ordinary pure helpers would make reuse clearer without creating an action entry |
| Computed values | Yes | `JET_ROOT`, `TZDIR`, runtime `PATH`, and library paths are derived values (`flake.nix:21-35,62-90,139-148`). Current non-package fields already permit pure computed expressions (`crates/jet-env-model/src/ModuleEval/mod.rs:12-15`) |
| Shared helpers | Yes | The two dev wrappers share root discovery, runtime path setup, existence checking, and execution (`flake.nix:62-90`). One ordinary helper or typed wrapper builder is a direct fit |
| Typed builders, methods, and structs | Yes, if they expose the `Environment` schema | The current result is already a large typed `EnvPlan` (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`). A builder can make construction and validation easier without changing the result boundary |
| Refactoring and IDE support | Yes, with a condition | It follows only if the APIs and result types are statically known. A dynamic entry that emits arbitrary facts is harder, not easier, for tools to understand |
| One mental model with `run.jet` | Partly | Ordinary functions and expressions can be shared. The lifecycle is different: a run entry is an action selected by a command, while an environment is a value needed before command execution and trust |

The benefits are real. They do not require the special entry contract. The smallest complete design is an ordinary pure function such as a project-chosen helper that returns the canonical `Environment` type, called from the environment fact. Jet need not reserve `env` as a lifecycle function to gain functions, builders, methods, structs, or `if`.

## “Tie it in like run, build, dev, and test” is not one mechanism today

The shipped command family currently resolves entries in different ways.

| Command | Shipped resolution | Meaning |
|---|---|---|
| `run` | The package resolver checks `run.jet`, then a legacy entry, then a typed output (`crates/jet-foundation/src/Syntax/package_files.rs:28-33`; `crates/jet-pkg-model/src/Package/mod.rs:830-906`) | Execute a program entry |
| `build` | Package-wide discovery finds `fn build` and rejects duplicates (`crates/jet-pkg-model/src/Package/mod.rs:923-945`) | Override build actions |
| `test` | Bare test scans the package; `fn test` can override the whole test run, with per-target override handling elsewhere (`Source/CmdCompile.rs:1974-2026,2109-2145`) | Execute tests or a test override |
| `dev` | The compiler's bare `dev` path is a file watcher (`Source/main.rs:2283-2307`); Jetpack first realizes the environment, then invokes `fn dev` or falls back to `fn run` (`crates/jetpack/src/CLI/run_enter_dev.rs:2826-2859`) | Watch or execute a development action after environment realization |
| `env` | Jetpack loads and composes the environment plan; it explicitly does not run a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1498`) | Discover facts needed before entering or running |

`D-VERDICT-678-1` ratified `run.jet` as the canonical program entry and linked its name to `jet run` and `fn run`; the shipped constant still reflects that (`crates/jet-foundation/src/Syntax/package_files.rs:28-33`). The later ratified `D-ROLEFILE1=A` and `D-CMDOVERRIDE1=A` rulings on Tower card #1866 amend that direction: optional `@run.jet`, `@build.jet`, `@dev.jet`, and `@test.jet` homes and package-wide optional command overrides are the intended model. Their implementation criteria remain open, so the table above describes shipped mechanics rather than pretending the future ruling already exists.

The governing board state was read without mutation:

```text
$ node plugins/tower/tower.mjs card show '#1866'
"title": "Command files and command-function overrides"
"body": "...Amends D-VERDICT-678-1 (run.jet loses reserved status)..."
"text": "...D-ROLEFILE1=A and D-CMDOVERRIDE1=A, both ratified 2026-08-10... The optional homes become @run.jet, @build.jet, @dev.jet and @test.jet, and an absent command function means stock behavior."
"criteria": [
  { "n": 1, "text": "Syntax.rs entry updated", "status": "open" },
  ...
]
```

The useful rhyme is “ordinary checked Jet functions may implement actions.” It does not follow that every noun used by a command must become an action entry. `env` is closer to a typed package output or configuration contribution: many later commands need its value before their action can start.

The recent entry rename also makes spelling churn a real migration cost, not a hypothetical one. The owner identified a 105-directory sweep; the current migration card now reports an even larger final file count:

```text
$ node plugins/tower/tower.mjs card show '#1032'
"text": "The entry-file retirement ratchet ... reaches ceiling 0: every main.jet in the repo is migrated to run.jet..."
"evidence": "...rg --files -g 'main.jet' = 0 and run.jet = 136."
```

That work does not argue against a better final design under Jet's greenfield rule. It does argue against adopting `fn env` merely for a temporary filename rhyme when the command-file law has already moved again.

## Cost of a general environment program

### Evaluation and static inspection

Today, tools parse and evaluate a restricted model into facts. Module items outside that model are invisible to sema and codegen (`crates/jet-env-model/src/ModuleEval/mod.rs:17-19`). A general entry changes the answer to “what is this environment?” from reading a bounded value graph to compiling and running a program.

Tools could still inspect the final result after execution, but they could not in general know it without execution. That affects editors, dependency scanners, `jet env info`, bridge tools, trust previews, and any command that wants to answer a package question before activating the environment. Pure bounded code does not make all static source analysis possible, but it keeps evaluation safe, deterministic, and cheap enough to be the standard inspection operation. The current plan also retains source files, graph identity, active environment, reads, and provenance for explanation (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

### Reproducibility and caching

Executable syntax does not itself destroy reproducibility. The environment can remain a pure function of its source closure, locked package inputs, selected target, explicit arguments, and declared environment reads. Nix demonstrates that functional code can produce reproducible derivations; the requirement is that every observable input participates in identity, not that the source looks like data.

Jetpack already hashes references, sources, secret declarations, lifecycle facts, presets, variables, language packs, files, services, and integrations into trust identity (`crates/jetpack/src/Trust.rs:235-432`). Its task environment hash includes the environment definition, active environment, provenance, source files, and every composed environment value (`crates/jetpack/src/CLI/run_enter_dev.rs:997-1034`). `jet env info` can list explicit environment reads by name (`crates/jetpack/src/CLI/run_enter_dev.rs:1894-1900`).

A safe computed-value design must extend cache identity to the full evaluated input set: source closure, target/platform, locked file or network inputs, and both the names and values of permitted ambient reads. A general function with undeclared file, process, network, clock, or random access cannot meet that rule reliably. Content-addressed storage remains valid only after the evaluation boundary prevents hidden inputs and effects.

### Sandboxing and trust

The trust order is decisive. Jetpack loads and composes the plan before it asks for entry trust (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`). `E1255` is raised for trust-sensitive references, secrets, or hooks before activation; an interactive acceptance remembers the exact environment and asks again after relevant changes (`crates/jetpack/src/Trust.rs:1000-1105`). This order is safe because plan evaluation is restricted and lifecycle commands are represented as data.

If `fn env` can perform effects, Jet must either run untrusted code before it can show the user what is being trusted, or ask the user to trust code before it can show the packages, secrets, hooks, and effects that the code selected. That is a trust-preview cycle. A pre-evaluation sandbox could reduce damage, but then the language inside `fn env` is necessarily restricted. That returns to the recommended pure value-producing model under a less honest name.

Lifecycle behavior already has the right split. A hook is either a checked `#Job` task or an expert trust-gated command (`crates/jet-env-model/src/ModuleEval/Environment.rs:1967-1981,2651-2713`). Environment computation should select and describe such actions; activation should execute them after `E1255`.

### Non-termination and resource use

An unrestricted Jet program can loop, recurse, allocate, spawn work, or wait. Process timeouts are a poor semantic answer because the same valid environment can become unreliable across machines. The current comptime interpreter instead has a deterministic fuel budget and an explicit exhaustion diagnostic (`crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,300-328`). Preserve that property. A restricted total subset is possible, but Jet already has the required bounded pure evaluator; creating another subset would duplicate machinery.

### `jet add`, `jet remove`, and `jet bridge flake`

These tools expose a current architectural seam that a general program would make worse.

`EnvFile` deliberately uses a structural `pkg.*` directive reader so Phase 1 can edit the file without running the compiler (`crates/jetpack/src/EnvFile.rs:1-22`). It recognizes only a default source, named sources, packages, and prompt (`crates/jetpack/src/EnvFile.rs:39-55,167-195`). `add` and `remove` then render and overwrite the whole file (`crates/jetpack/src/EnvFile.rs:114-153,218-258`; `crates/jetpack/src/CLI/add_remove_push_image.rs:62-145,298-355`). The repository's root file uses the newer typed `module env.dev` form (`env.jet:7-17`), not those legacy directives. The direct code consequence is that the legacy editor can ignore typed fields and rewrite the file into its own legacy surface. This is existing migration debt, not a reason to keep the old format.

An arbitrary function body has no unique source edit for “add this package.” The package may come from a conditional, helper, loop, member contribution, or builder chain. A generated source rewrite would be guesswork.

The canonical computed-value model gives the CLI an honest rule: edit the one explicit package-fact anchor when one exists; if the package closure is wholly computed or has more than one possible owner, refuse with the relevant source and provenance. A generated contribution is acceptable only if it is the same typed `Config`/`Environment` mechanism, not a hidden second database or legacy syntax.

The flake bridge is already a bounded translator. It sorts and deduplicates a typed module shim, reports unmapped facts, prints the result, and does not edit the environment (`crates/jetpack/src/Bridge.rs:398-440,722-730`; `crates/jetpack/src/CLI/add_remove_push_image.rs:434-437`). It should continue to emit the canonical typed value plus a loss report. Generating an arbitrary function body would make later inspection and round trips less reliable.

## Peer lessons

| System | Mechanism | Lesson for Jet |
|---|---|---|
| Nix | The Nix language is lazy, purely functional, and dynamically typed; derivations define inputs used to determine store outputs. The official tutorial also warns that expressions can quickly become complicated ([Nix language tutorial](https://nix.dev/tutorials/nix-language.html); [derivation outputs](https://nix.dev/manual/nix/2.35/store/derivation/outputs/index.html); [content-addressed outputs](https://nix.dev/manual/nix/2.32/store/derivation/outputs/content-address)) | Configuration code and reproducible stores coexist only when effects and inputs are controlled. Power does not remove the readability cost |
| Bazel/Starlark | Starlark replaced Python-like build configuration with a deterministic, hermetic language that has no file system, network, or clock access. Recursion is rejected and loops are finite, so evaluation is not Turing-complete ([Starlark repository](https://github.com/bazelbuild/starlark); [language specification](https://github.com/bazelbuild/starlark/blob/master/spec.md); [design](https://github.com/bazelbuild/starlark/blob/master/design.md)) | Build configuration may look like code, but deliberate limits make parallel evaluation, caching, tooling, and untrusted use tractable. “Ordinary code” is the wrong default boundary |
| Gradle Groovy and Kotlin DSL | Kotlin DSL adds static typing, content assist, refactoring, and documentation, but scripts still compile and cache; dynamic model elements can miss type-safe accessors, and build-logic changes can invalidate caches ([migration guide](https://docs.gradle.org/current/userguide/migrating_from_groovy_to_kotlin_dsl.html); [Kotlin DSL guide](https://docs.gradle.org/current/userguide/kotlin_dsl.html)) | IDE gains come from a known typed model, not merely from putting configuration in a host language. Dynamic extension and script compilation retain costs |
| devenv | Nix-backed modules expose typed options for packages, profiles, scripts, and `enterShell`; documentation recommends tasks over activation hooks where ordering matters ([option reference](https://devenv.sh/reference/options/); [scripts and tasks](https://devenv.sh/scripts/)) | Even a code-backed configuration system converges on a typed schema and named lifecycle actions |
| Devbox | `devbox.json` keeps packages, environment variables, platform filters, hooks, scripts, and includes as editable data; longer behavior moves to named shell files ([configuration](https://www.jetify.com/docs/devbox/configuration); [scripts](https://www.jetify.com/docs/devbox/guides/scripts)) | Common environment facts benefit from a readable edit surface. Imperative escape is clearer as a named action than as manifest evaluation |
| Cargo | `Cargo.toml` remains machine-readable manifest data. `build.rs` is compiled and executed separately, emits declared instructions, uses rerun controls, and should write only to `OUT_DIR` ([manifest](https://doc.rust-lang.org/cargo/reference/manifest.html); [build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html); [metadata](https://doc.rust-lang.org/nightly/cargo/commands/cargo-metadata.html)) | Keep facts inspectable and place procedural probes behind an explicit action boundary. Do not turn the manifest itself into arbitrary runtime code |

The peers do not support a binary choice between TOML and unrestricted code. Their common lesson is stronger: use a typed, explainable fact model; allow computation only under explicit limits; isolate imperative work as named actions.

## Middle paths

| Middle path | Judgment |
|---|---|
| Add more declarative fields only | Insufficient. It can close today's gaps, but repeats schema pressure each time experts need a derived value and underuses Jet's existing pure evaluator |
| Add “typed computed fields” | Good, but describe it honestly as ordinary pure Jet expressions producing the typed value. This is already the direction for most fields (`crates/jet-env-model/src/ModuleEval/mod.rs:12-15`) |
| Create a new total environment DSL | Reject. It duplicates the existing bounded pure comptime mechanism and creates another language to teach and tool |
| Let arbitrary code run, then serialize its result | Reject. Serialization after execution does not solve hidden inputs, pre-trust effects, non-termination, or source editing |
| Require pure bounded code to return `Environment` | Recommend. It is one semantic mechanism, supports literals and expert computation, and retains an inspectable result contract |

The recommended path needs a clear purity contract. Environment evaluation may use ordinary pure Jet values and functions; explicit target and declared environment inputs; locked package and source facts; and a deterministic fuel limit. It may not read arbitrary files, call the network, inspect the clock, use randomness, spawn processes, or execute lifecycle actions. Those operations belong in declared tasks, build actions, or activation hooks with their existing authority boundaries.

## Invariant and mission fit

### I8: one mechanism

Keeping today's declarative form and adding a separate `fn env` entry would create two ways to define the same environment unless one immediately replaces the other. Making the function the only form would remove the literal reader and tool-edit anchor. Both outcomes are worse than one typed value that may be written literally or computed by ordinary expressions.

The canonical mechanism should be: contributions produce one typed `Environment` value. A literal, a builder chain, and a helper call are expression forms for the same value, not parallel environment systems. Split files remain layout choices for the same typed `Config` contribution under the ratified model (`docs/spec/syntax-decisions.md:6090-6108`). The legacy directive reader and typed module authoring surface should not both survive the migration (`crates/jetpack/src/CLI/realize.rs:661-692`).

### I7: named owner gates

Ordinary `fn`, calls, methods, structs, and `if` are existing Jet syntax. A compiler-known lifecycle name `fn env` would still add a reserved entry contract even if it adds no lexer token. It would require an owner-ratified decision for that contract and its signature, discovery rules, duplicate rules, effects, inputs, result, diagnostics, and command behavior. It would also amend the current rule that `jet env` never runs a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1498`).

Keeping `env.jet` as a reserved root would conflict with `D-ECO-FILEROOT1=A` (`docs/spec/syntax-decisions.md:6094-6098`). Introducing `@env.jet` would amend the ratified role-file slate on Tower card #1866. A public `Environment` builder surface—types, methods, wrapper forms, path projections, hooks, and target inputs—also needs an explicit owner decision even where it uses only existing syntax. `D-ECO-ENV1=A` intentionally did not choose the final schema or source placement (`docs/spec/syntax-decisions.md:4409-4413`).

### Beginner and expert

For a beginner reading another project, one visible typed environment fact is the better anchor. Packages, variables, services, and hooks can be read without tracing an entry's control flow. The literal case should require no builder ceremony.

For an expert, pure helpers, typed builders, package-output paths, declared target input, and target conditionals provide the required control. The expert escape should be explicit when it crosses into effects: a named task, build action, wrapper, or activation hook, not an ambient capability available during environment discovery.

This also improves explanation. `jet env info` can show the final value, each contributing source, selected branches, declared inputs, and lifecycle actions before entry. The current plan already carries source files, graph identity, reads, active environment, and provenance (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

### AI-assisted development

A known result type improves completion fidelity, refactoring, repair determinism, and context use because tools can navigate one schema and validate the final value. Bounded pure evaluation adds predictable latency. A general entry increases discovery latency and forces tools to execute user code before they can answer basic questions. It also makes automated edits ambiguous when packages are spread through control flow. These effects follow from the current structural editor and typed-plan seams (`crates/jetpack/src/EnvFile.rs:1-22,114-195`; `crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

## Migration reality

If the owner chooses environment-as-code, the greenfield rule still requires one canonical form and deletion of the replaced form. The migration should not preserve a declarative `env.jet` beside `fn env`, keep the legacy `pkg.*` reader as a fallback, or add aliases.

Under the recommendation, one coherent migration would:

1. Fold the environment contribution into `package.jet` under `D-ECO-FILEROOT1=A`, while allowing layout-neutral typed `Config` contributions as already ratified (`docs/spec/syntax-decisions.md:6090-6108`).
2. Define one complete typed `Environment` result and let pure Jet expressions, helpers, structs, methods, and builders produce it. Make packages an ordinary computed field, not special static sugar (`crates/jet-env-model/src/ModuleEval/mod.rs:6-15`).
3. Model wrappers, package-output paths, target-conditioned path contributions, named profiles, and lifecycle actions as typed values. Run actions only after trust (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1529`).
4. Remove the legacy `EnvFile` parser/renderer and migrate every in-repo caller, fixture, example, document, and test in the same change (`crates/jetpack/src/EnvFile.rs:1-22,114-258`).
5. Change `jet add` and `jet remove` to edit the single explicit package-fact anchor. If no unique literal anchor exists, stop with a source-and-provenance explanation instead of rewriting arbitrary code.
6. Make `jet bridge flake` emit the canonical typed contribution and an exact loss report, preserving its current bounded, stdout-only behavior (`crates/jetpack/src/Bridge.rs:398-440,722-730`; `crates/jetpack/src/CLI/add_remove_push_image.rs:434-437`).
7. Key evaluation and cache identity from the complete source closure, selected target, locked inputs, and declared ambient-read names and values. Preserve finite evaluation fuel (`crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,300-328`).
8. Delete `env.jet` and every replaced spelling after all repository consumers move. Do not ship a compatibility parser or dual discovery path.

If the owner instead chooses a general `fn env`, the same deletion rule applies, but the project must first answer the pre-trust execution cycle, sandbox authority, non-termination policy, complete input identity, static inspection contract, and source-edit contract. Those are design requirements, not later implementation details.

## Tower card #2155

The proposal interacts with live card #2155, but it does not solve it. The card reports two blockers in the current root environment: `default.[cargo]` can resolve to `cargo@default` and collide with a built-in provider despite the ratified `D-JPK-REF1=A` reference grammar, and Nixpkgs references can reach `E1272` because Jetpack lacks a compatible realization output/backend. `D-JPK-REF1=A` defines the email-order `package@source` reference and says built-in provider names are not package names (`docs/spec/syntax-decisions.md:4434-4441`). The root environment therefore cannot currently be entered, but the reported failures concern reference parsing and realization, not whether its source is data or a function.

The card's exact current evidence, read without mutation, is:

```text
$ node plugins/tower/tower.mjs card show '#2155'
"title": "env.jet cannot be entered: a package named like a provider fails E1317, and nixpkgs packages cannot realize"
"body": "Two blockers found on 2026-08-24 while testing whether jetpack can replace the nix devshell.\n\n1. The repository own env.jet cannot be entered at all. ./target/debug/jetpack enter -- echo hi fails with E1317 `cargo@default` puts the provider first / D-JPK-REF1 puts the package or target before @ and the source after it / Fix: write `default@cargo`. Minimal repro: an env.jet whose packages list is default.[cargo] fails, while default.[ripgrep] parses. The name cargo collides with a builtin recipe provider, so the resolver reads the package as a provider. The committed env.jet lists cargo first, so the project shell it describes is unreachable.\n\n2. Even with a parseable env, realization stops: E1272 1 package lacks a supported Nix compatibility output / `ripgrep@default` need a pinned compatibility output. Jetpack does not invoke an installed Nix executable for package realization."
"phase": "planning"
```

Turning the same package references into `fn env` would leave both blockers in place. Conversely, fixing #2155 would make the current form enterable without closing the flake parity gaps listed above. The owner should treat #2155 as evidence that the implementation is buggy and incomplete, not that declarative values are broken by design.

A local read-only probe could not independently reach the card's two errors in this workspace. The command exited earlier:

```text
$ ./target/debug/jetpack enter --no-color -- echo hi
Error [E2604]: Integrity check failed for `Hangar path migration` `legacy` — expected `complete native per-user Hangar`, got `Read-only file system (os error 30)`.
 Why: The cached artifact failed reversible Hangar path migration. Jetpack stopped before reading or changing package state.
 Fix: Inspect the reported Hangar path and migration staging path; move unsafe nodes aside without deleting them, then retry the command.
```

The #2155 account above therefore comes from the current Tower card and matching source/decision evidence, not a successful fresh entry reproduction.

## Decision test

The owner can make the choice with one question: should environment discovery be allowed to perform behavior, or must it only compute and return facts?

If discovery may perform behavior, choose `fn env` and accept a new trust-before-preview model, a sandboxed execution protocol, runtime failure and termination policy, weaker source editing, and a more complex cache-input contract.

If discovery must return facts and effects remain explicit later actions, choose the recommendation. It provides the requested Jet programming tools while keeping one typed, bounded, explainable result.

The evidence in this repository favors the second answer. The flake gaps require more computation and a fuller schema, but none requires arbitrary behavior during discovery. Existing ratified decisions already point to one typed `Environment`, one ecosystem root, pure computed contributions, and separate action functions (`docs/spec/syntax-decisions.md:4397-4413,6090-6130,6933-6957`).

## Findings

1. The root `env.jet` covers only 20 of 24 default-shell package expressions and 20 of 50 full-shell expressions; it also omits every admitted hook, wrapper, derived-path, and full-profile behavior (`env.jet:1-17`; `flake.nix:110-283`).
2. The apparent data-versus-code choice is false. Jet already evaluates most environment fields with a pure, deterministic, fuel-bounded interpreter; package computation and schema coverage are the main missing pieces (`crates/jet-env-model/src/ModuleEval/mod.rs:6-19`; `crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,300-328`).
3. A general entry creates a pre-trust execution cycle because the plan is evaluated before `E1255`, while current effects are represented as data and executed afterward (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`; `crates/jetpack/src/Trust.rs:1000-1105`).
4. Current authoring already has two mechanisms: typed modules and the legacy directive reader. The legacy whole-file editor cannot safely evolve into an arbitrary-code editor (`crates/jetpack/src/CLI/realize.rs:661-692`; `crates/jetpack/src/EnvFile.rs:1-22,114-258`).
5. The reserved `env.jet` root is already on a ratified removal path through `D-ECO-FILEROOT1=A`; a redesign should converge on `package.jet`, not deepen the old root (`docs/spec/syntax-decisions.md:6090-6108`).
6. Tower card #2155 is a live implementation blocker, not evidence for either source shape. A source redesign does not fix reference collision or package realization.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
|---|---|---|
| F1: Repository environment lacks default and full flake parity | no-action | audit-only: decision input; the owner forbade card creation |
| F2: Pure computed environment values already have a ratified semantic base | decision | D-MODCOMPUTE1=A |
| F3: Arbitrary pre-trust environment execution conflicts with the current authority boundary | decision | D-ECO-ENV1=A |
| F4: Typed and legacy authoring/editing surfaces are already split | no-action | audit-only: migration consequence; the owner forbade implementation and cards |
| F5: `env.jet` is not the ratified long-term ecosystem root | decision | D-ECO-FILEROOT1=A |
| F6: Current repository environment cannot be entered for separate parser and realization reasons | card | #2155 |
<!-- /audit-dispositions -->

## Limits and unresolved design work

The final public `Environment` schema is not determined. `D-ECO-ENV1=A` explicitly leaves schema and placement details open (`docs/spec/syntax-decisions.md:4409-4413`). In particular, this audit cannot name the final wrapper, derived-path, profile, or builder APIs without making the owner decision it was asked to inform.

The shipped command-entry mechanics and the newer ratified command-role rulings are temporarily out of sync: Tower card #1866 records the newer direction, but its implementation criteria remain open. The recommendation relies on the semantic distinction between facts and actions, not on the temporary `run.jet` file spelling.

The local entry probe stopped at sandbox-sensitive Hangar migration `E2604`, so this audit did not freshly reproduce the two #2155 failures. No other material fact remained undetermined.
