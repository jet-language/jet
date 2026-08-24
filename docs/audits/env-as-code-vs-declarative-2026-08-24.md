# Environment source: pure producer or manifest-shaped value?

Date: 2026-08-24

Scope: decision audit only. This report does not change behavior, edit `env.jet`, or create work.

Revision note: the first version treated `fn env(...)` as a general command whose boundary was any behavior it could perform. The owner proposed a pure producer instead: `fn env(ctx: EnvContext) -> Environment -[]>`. This revision assesses that design. It keeps the return value as the declarative boundary and separates it from an effectful alternative.

## Recommendation

Adopt one package-wide pure producer:

```jet
fn env(ctx: EnvContext) -> Environment -[]> {
    return Environment{ }
}
```

The function must run in the existing fuel-bounded evaluator. It must return one complete typed `Environment` value. It must not execute lifecycle actions or receive ambient filesystem, network, process, clock, random, secret, or live-hardware authority. The hard declarative boundary is the return type, not a manifest-shaped source file.

Home the optional producer in `@env.jet`. Discover it package-wide, as card #1866 already requires for command functions. Require explicit imports from helper modules. This choice amends `D-ROLEFILE1=A`, whose marked namespace is currently closed to four names, but it does not restore plain `env.jet` as an ecosystem root (`docs/spec/syntax-decisions.md:5963-5979,6118-6122`). `package.jet` remains the one Package fact root. `@env.jet` is the optional, visible home of the function that projects those facts into an environment.

This reverses the first draft's recommendation on the entry shape. The owner proposed the same computed-data boundary that the draft recommended, plus a compiler-known and teachable function name. The special entry is useful because it makes the programming model visible: a reader sees normal Jet imports, a typed context, a pure effect row, and a typed return. The current `module env.dev { ... }` surface does not show that most fields use Jet's comptime evaluator, and those module items are otherwise invisible to normal sema and codegen (`crates/jet-env-model/src/ModuleEval/mod.rs:1-19`).

The hard blocker remains coverage. The new entry cannot replace the repository flake, much less describe a JetOS machine, until `Environment` gains typed wrappers, package-output paths, search and library path composition, ordered activation actions, named profiles, and the JetOS system model described below. Code makes composition tractable. It does not remove the need for a complete typed result.

## The choice is narrower than it first appears

The current form is not merely “advanced TOML.” Except for package-list sugar, its fields already pass through Jet's pure comptime evaluator and may use `if`, immutable values, pure helpers, and other deterministic expressions (`crates/jet-env-model/src/ModuleEval/mod.rs:6-19`; `docs/spec/syntax-decisions.md:2631-2640`). The evaluator also supports explicit typed environment reads, records those reads, and installs them as comptime globals (`crates/jet-env-model/src/ModuleEval/Eval.rs:69-150`). It has a finite step budget and reports `E0952` when the budget is exhausted (`crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,300-328`).

The real decision is therefore:

| Option | Boundary | Consequence |
|---|---|---|
| Keep today's structural surface | A typed plan, but with package sugar and incomplete fields | Safest and easiest to edit, but cannot replace this repository's flake |
| Add pure `fn env(ctx: EnvContext) -> Environment -[]>` | Bounded evaluation over recorded inputs, followed by a complete typed value | Makes the one-language model visible and scales to large composition without weakening trust or cache identity |
| Add effectful `fn env` | Any behavior granted by its effect row and context | Matches `BuildContext` authority more closely, but creates a trust-preview cycle and undefined offline/cache semantics |

The second option is recommended. A new restricted configuration language would add another mechanism even though Jet already has a pure, fuel-bounded comptime evaluator (`crates/jet-comptime/src/Comptime/mod.rs:1-16`).

## What exists now

The root file declares `module env.dev`, one named Nix source, 20 package references, and a prompt (`env.jet:7-17`). Its own header says the intended scope is the default compiler shell and admits three unmapped groups: the `shellHook`, the `jetDev` and `jetpackDev` wrappers, and the entire opt-in full shell (`env.jet:1-6`).

Jet Foundation defines `env.jet`, the `env` namespace, an `Environment` output kind, and an environment-output entry field (`crates/jet-foundation/src/Syntax/jetpack_config.rs:137-142,249-284,394-402`). The declared environment surface includes prompt, dotenv, unset, enter hooks, checks, reload policy, Git hooks, formatter, presets, languages, files, and integrations (`crates/jet-foundation/src/Syntax/jetpack_config.rs:585-651`). The evaluated plan is already a typed structure containing source identity, explicit environment reads, packages, adapters, prompts, systems, images, fleets, VM tests, services, secrets, lifecycle actions, presets, languages, files, integrations, profiles, active environment, and provenance (`crates/jet-env-model/src/ModuleEval/Types.rs:532-634`).

An environment is consumed as data. Jetpack finds the nearest `env.jet`, evaluates the typed module surface, or falls back to a legacy `pkg.*` directive reader (`crates/jetpack/src/CLI/realize.rs:632-692`). It expands the resulting facts into package references and a `RunPlan` (`crates/jetpack/src/CLI/realize.rs:605-630,695-794`). `jet enter`, `jet dev`, and related paths compose that plan, grant trust if required, run lifecycle hooks, and then start the requested command or shell (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`). The code explicitly states that `jet env` never runs a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1498`).

This boundary matches the ratified architecture. `D-ECO-ENV1=A` says one typed `Environment` output is the source of truth and that imperative actions are capability-scoped and audited (`docs/spec/syntax-decisions.md:4397-4407`). `D-ECO-FILEROOT1=A` says `package.jet` is the one reserved ecosystem file and that environment, workspace, and config facts fold into it through one migration epoch (`docs/spec/syntax-decisions.md:6118-6122`). `D-CONF-SPLIT1=A` separates declared facts from `fn build` actions and says computed contributions must remain recorded and explainable (`docs/spec/syntax-decisions.md:6963-6977`).

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

## JetOS changes the weight of the choice

The first draft compared one small `env.jet` with two dev shells. Epoch 9 requires a much larger value. Its cards separately own typed options, disks, services, ordered activation, accounts, PAM, upstream systemd composition, user environments, source-backed hardware profiles, and named hardware specialisations. Their plans require one typed system model from source through plan, proof, activation, and inspection, with transactional rollback on failure. This is not an inferred roadmap summary. The board reports these exact slices:

```text
$ for n in 328 329 402 404 405 407 469 842 843; do node plugins/tower/tower.mjs card show "#$n"; done
"title": "jetos disks: declarative partition and filesystem plans"
"title": "jetos services: socket activation"
"title": "jetos activation: ordered activation snippets"
"title": "jetos accounts: passwd and group reconciliation"
"title": "jetos PAM: typed stack rule model"
"title": "jetos systemd: upstream unit composition, drop-ins, and masks"
"title": "jetos users: typed user environment model"
"title": "jetos hardware: source-backed hardware profiles"
"title": "jetos hardware: named specialisations and boot selection"
"plan": "...Use one typed system model from source through plan, proof, activation, and inspection. Activation must be transactional and explainable..."
```

The shipped model shows both the scale and the current weakness. `SystemPlan` has only `target`, `packages`, `services`, and string-valued `options` (`crates/jet-env-model/src/ModuleEval/Types.rs:289-314`). Evaluation accepts only those four fields and slices option values back out of source text (`crates/jet-env-model/src/ModuleEval/System.rs:30-89`). JetOS consumers then recover structure by matching string prefixes. Storage reads keys such as `storage.disk.main.device` and `storage.filesystem.root.type` (`crates/jetpack/src/JetOS/module_storage_workload.rs:48-93`). User realization does the same for home, shell, packages, services, and files (`crates/jetpack/src/JetOS/user_flatpak_perf.rs:13-102`).

That surface will strain under the Epoch 9 model regardless of source shape. A large system needs functions for reusable profiles, target conditions, and composition. The repository's own Nix flake already uses functions and local bindings to derive wrappers, paths, packages, and per-system outputs (`flake.nix:9-35,62-90`). The JetOS evidence therefore strengthens the owner's producer proposal. One pure entry can compose a dev shell and system outputs through normal Jet code. It cannot make string options acceptable or move activation into evaluation.

The claim holds with one limit: “one entry” means one pure source-to-value projection, not one phase that also realizes packages, probes hardware, mutates disks, switches generations, or starts services. Those later phases consume the returned graph under their own trust, capability, and transaction rules.

## What `fn env(...)` would buy

| Claimed benefit | Is it needed here? | Current evidence and assessment |
|---|---|---|
| Platform conditions | Yes | The flake selects five browser/graphics expressions only on Linux and changes the library path by platform (`flake.nix:231-246,256-265`). Pure target input plus `if` is enough |
| Reuse across members | Needed and aligned | Typed environment modules are discovered and merged in one pass (`crates/jet-env-model/src/ModuleEval/Source.rs:341-395`). A named producer plus explicit imports makes the final composition site visible |
| Computed values | Yes | `JET_ROOT`, `TZDIR`, runtime `PATH`, and library paths are derived values (`flake.nix:21-35,62-90,139-148`). Current non-package fields already permit pure computed expressions (`crates/jet-env-model/src/ModuleEval/mod.rs:12-15`) |
| Shared helpers | Yes | The two dev wrappers share root discovery, runtime path setup, existence checking, and execution (`flake.nix:62-90`). One ordinary helper or typed wrapper builder is a direct fit |
| Typed builders, methods, and structs | Yes, if they expose the `Environment` schema | The current result is already a large typed `EnvPlan` (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`). A builder can make construction and validation easier without changing the result boundary |
| Refactoring and IDE support | Yes, with a condition | It follows only if the APIs and result types are statically known. A dynamic entry that emits arbitrary facts is harder, not easier, for tools to understand |
| One visible Jet language | Yes | The current surface looks like a manifest even though most fields use comptime (`crates/jet-env-model/src/ModuleEval/mod.rs:1-19`). A normal function signature exposes its inputs, purity, and result without teaching that hidden rule |
| One entry family with run, build, dev, and test | Yes, at discovery level | `D-ROLEFILE1=A` and `D-CMDOVERRIDE1=A` establish optional `@...jet` homes, package-wide function discovery, stock behavior when absent, and duplicate rejection (`docs/spec/syntax-decisions.md:5963-5979`) |

The benefits are real, and the teaching benefit does require the special entry contract. A project-chosen helper can compute the same value, but it cannot answer the beginner's question “what does `jet env` use?” The compiler-known name gives one search target and one signature. The return type keeps this an environment producer rather than a lifecycle action.

## `D-ROLEFILE1` establishes the proposed pattern

The shipped command family currently resolves entries in different ways.

| Command | Shipped resolution | Meaning |
|---|---|---|
| `run` | The package resolver checks `run.jet`, then a legacy entry, then a typed output (`crates/jet-foundation/src/Syntax/package_files.rs:28-33`; `crates/jet-pkg-model/src/Package/mod.rs:830-906`) | Execute a program entry |
| `build` | Package-wide discovery finds `fn build` and rejects duplicates (`crates/jet-pkg-model/src/Package/mod.rs:923-945`) | Override build actions |
| `test` | Bare test scans the package; `fn test` can override the whole test run, with per-target override handling elsewhere (`Source/CmdCompile.rs:1974-2026,2109-2145`) | Execute tests or a test override |
| `dev` | The compiler's bare `dev` path is a file watcher (`Source/main.rs:2283-2307`); Jetpack first realizes the environment, then invokes `fn dev` or falls back to `fn run` (`crates/jetpack/src/CLI/run_enter_dev.rs:2826-2859`) | Watch or execute a development action after environment realization |
| `env` | Jetpack loads and composes the environment plan; it explicitly does not run a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1498`) | Discover facts needed before entering or running |

The shipped command family still resolves entries in different ways, as the table shows. That implementation state does not decide the new design. The later ratified `D-ROLEFILE1=A` and `D-CMDOVERRIDE1=A` rulings on Tower card #1866 establish the intended pattern: optional `@run.jet`, `@build.jet`, `@dev.jet`, and `@test.jet` homes; package-wide function discovery; stock behavior when absent; and duplicate rejection (`docs/spec/syntax-decisions.md:5963-5979`).

The governing board state was read without mutation:

```text
$ node plugins/tower/tower.mjs brief '#1866' --no-claim
"title": "Command files and command-function overrides"
"body": "...Amends D-VERDICT-678-1 (run.jet loses reserved status)..."
"text": "...D-ROLEFILE1=A and D-CMDOVERRIDE1=A, both ratified 2026-08-10... The optional homes become @run.jet, @build.jet, @dev.jet and @test.jet, and an absent command function means stock behavior."
"criteria": [
  { "n": 1, "text": "Syntax.rs entry updated", "status": "open" },
  ...
]
```

The first draft read this ruling too narrowly. The useful pattern is broader than “functions implement actions.” It says a CLI concept can have one package-wide function, one optional marked home, a stock default, and one duplicate law. A pure environment producer fits that discovery pattern even though its result is data.

`D-ROLEFILE1=A` does not silently authorize `@env.jet`. It says the marked namespace is closed to the current four names (`docs/spec/syntax-decisions.md:5963-5969`). `D-ENVFN1` must amend the set to five. That is an owner gate, not evidence against the shape. `D-ECO-FILEROOT1=A` also remains intact because plain `env.jet` still retires into `package.jet`; the new marked file is an optional function home, not a second Package manifest (`docs/spec/syntax-decisions.md:6118-6122`).

## Boundary of the environment producer

### Evaluation and static inspection

Today, tools parse and evaluate a restricted model into facts. Module items outside that model are invisible to normal sema and codegen (`crates/jet-env-model/src/ModuleEval/mod.rs:17-19`). A pure `fn env` changes the source shape, but it need not change the inspection contract. Tools compile and evaluate one checked function under the comptime fuel budget, then inspect its complete typed result.

Pure code does mean tools cannot know every selected value from syntax alone. That is already true for computed module fields. The required operation is bounded evaluation, not arbitrary runtime execution. The result must retain source files, graph identity, selected branches, context reads, active profile, and field provenance. The current plan already carries most of that explanation frame (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

### Exact `EnvContext` contract

`EnvContext` must be an immutable input value, not an authority object. The host constructs it before evaluation. Accessors may record which supplied values the function observes, but they may not acquire new ambient values.

| Surface | Value exposed to `fn env` | Input identity rule |
|---|---|---|
| `ctx.target` | Selected OS, architecture, ABI, and declared target features | Hash the canonical target record, including absent features |
| `ctx.profile` | Explicit environment or system profile selection, such as default or full | Hash the selected name and the CLI/source that selected it |
| `ctx.package` | Read-only Package identity, members, locked sources, dependencies, and typed output handles | Hash the Package graph identity and each observed output identity; never expose host `PATH` lookup |
| `ctx.project` | A symbolic project-root handle and project-relative source handles | Hash the graph-relative path and content digest; do not put an absolute host path into the environment identity |
| `ctx.environment` | Only explicitly named, typed ambient variables | Record name, type, present/absent state, and a value digest for each actual read; reject enumeration and computed names |
| `ctx.inputs` | Declared project-relative or content-pinned input snapshots already present in the Package lock | Record normalized path or URL identity and content digest; expose bytes or typed data from the snapshot, not a live file or network handle |
| `ctx.hardware` | An optional source-backed hardware snapshot made by a separate detection command | Hash the snapshot schema and content; never probe devices during `fn env` |

The current evaluator supplies a useful base. It lexes explicit `$NAME` reads, records each name and type, snapshots the value into comptime globals, and evaluates with those globals (`crates/jet-env-model/src/ModuleEval/Eval.rs:69-150`). The current `EnvironmentRead` stores only name and type (`crates/jet-env-model/src/ModuleEval/Types.rs:17-24`). `D-ENVFN1` therefore needs a stronger evaluation-input ledger for cache identity: name, type, present state, and value digest. Secret values must never enter this context. Only secret declarations or opaque post-trust handles may appear in the returned plan (`crates/jet-env-model/src/ModuleEval/Types.rs:26-31`).

Recorded source input follows existing comptime law. Project-relative file reads are hashed when evaluated, sorted glob matches each record their content, and pinned fetches record the URL and verified content hash (`crates/jet-comptime/src/Comptime/Methods/dispatch.rs:388-478,480-528,705-773`). `EnvContext` should consume those locked snapshots. It should not expose the live `find`, `embed`, or `fetch` operations themselves. That keeps offline evaluation local and makes a missing snapshot a defined input error instead of an implicit network attempt.

The following capabilities stay out of `EnvContext`: process execution; shell commands; live network or DNS; arbitrary file reads or directory walks; clock; randomness; mutable host state; environment enumeration; secret values; host `PATH`; live hardware probes; writes; task spawning; lifecycle execution; and `BuildContext` itself. The function may return typed descriptions of wrappers, services, activation actions, disk plans, and probes. The relevant engine performs those actions later, after trust and under its own authority.

### `BuildContext` is already effectful

The parity argument must be precise. `BuildContext` provides the right entry shape but the wrong authority ceiling for environment discovery. Its interpreter bridge exposes `find`, `embed`, and SHA-256-pinned `fetch` as recorded build inputs (`crates/jet-comptime/src/Comptime/Build/runtime_bridge.rs:145-177`). It also accepts ambient probes, plugins, and actions with effects when the call sits inside `#Impure("reason")`; otherwise it raises `E3502` (`crates/jet-comptime/src/Comptime/Build/runtime_bridge.rs:182-214`). The public build effect set includes network, filesystem, I/O, database, time, randomness, environment, process execution, logging, and GPU authority (`crates/jet-foundation/src/Authority.rs:115-156`). The spec's worked build entry therefore has a non-empty effect row and an explicit impurity gate (`docs/spec/spec.md:4378-4420`).

This finding cuts against copying `BuildContext` wholesale. `fn build` runs after source selection to construct actions that execute under declared policy. `fn env` must run before `E1255` can show the environment under review. Reuse the typed-context and typed-plan pattern. Do not reuse build authority.

### Reproducibility and caching

Function syntax does not destroy reproducibility. The environment remains a pure function of its source closure and the immutable `EnvContext`. Every observed context value must enter one evaluation-input receipt. The evaluated `Environment`, its input receipt, and the compiler/evaluator version form the content-addressed key.

Jetpack already hashes references, sources, secret declarations, lifecycle facts, presets, variables, language packs, files, services, and integrations into trust identity (`crates/jetpack/src/Trust.rs:235-432`). Its task environment hash includes the environment definition, active environment, provenance, source files, and every composed environment value (`crates/jetpack/src/CLI/run_enter_dev.rs:997-1034`). `jet env info` can list explicit environment reads by name (`crates/jetpack/src/CLI/run_enter_dev.rs:1894-1900`).

A safe producer must extend identity to the source closure, compiler/evaluator version, selected target and profile, Package lock identity, observed package outputs, locked input snapshots, hardware snapshot, and each typed ambient read. An effectful function with undeclared file, process, network, clock, or random access cannot produce a complete stable key. Content-addressed storage therefore supports option B and rejects option C unless option C gives up ordinary caching.

### Sandboxing and trust

The trust order is decisive. Jetpack loads and composes the plan before it asks for entry trust (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`). `E1255` is raised for trust-sensitive references, secrets, or hooks before activation; an interactive acceptance remembers the exact environment and asks again after relevant changes (`crates/jetpack/src/Trust.rs:1000-1105`). This order is safe because plan evaluation is restricted and lifecycle commands are represented as data.

Pure `fn env` preserves this order because evaluation can only transform supplied inputs into data. If `fn env` can perform effects, Jet must either run untrusted code before it can show the user what is being trusted, or ask for trust before it can show the selected packages, secrets, hooks, and effects. That is a trust-preview cycle. A pre-evaluation sandbox reduces damage only by imposing the same no-ambient-authority rule as the recommended producer.

Lifecycle behavior already has the right split. A hook is either a checked `#Job` task or an expert trust-gated command (`crates/jet-env-model/src/ModuleEval/Environment.rs:1967-1981,2651-2713`). Environment computation should select and describe such actions; activation should execute them after `E1255`.

### Non-termination and resource use

Pure Jet can still loop or recurse. Process timeouts are a poor semantic answer because the same valid environment can become unreliable across machines. The current comptime interpreter has a deterministic fuel budget and reports `E0952` on exhaustion (`crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,299-328`). `fn env` must use that evaluator and budget.

### `jet add`, `jet remove`, and `jet bridge flake`

These tools expose the main cost of the producer design.

`EnvFile` deliberately uses a structural `pkg.*` directive reader so Phase 1 can edit the file without running the compiler (`crates/jetpack/src/EnvFile.rs:1-22`). It recognizes only a default source, named sources, packages, and prompt (`crates/jetpack/src/EnvFile.rs:39-55,167-195`). `add` and `remove` then render and overwrite the whole file (`crates/jetpack/src/EnvFile.rs:114-153,218-258`; `crates/jetpack/src/CLI/add_remove_push_image.rs:62-145,298-355`). The repository's root file uses the newer typed `module env.dev` form (`env.jet:7-17`), not those legacy directives. The direct code consequence is that the legacy editor can ignore typed fields and rewrite the file into its own legacy surface. This is existing migration debt, not a reason to keep the old format.

An arbitrary function body has no unique textual edit for “add this package.” The package may come from a conditional, helper, loop, imported value, or builder chain. Two honest options exist:

| Migration option | Contract | Rough cost |
|---|---|---|
| Structured source edit | Use the semantic index to find one literal `Environment.tools` or named package-list anchor, prepare a span edit, re-evaluate the producer, show the diff, and write only if the source revision still matches. Refuse computed or ambiguous ownership with source provenance | About 2-4 engineer-weeks for the bounded anchor, diagnostics, conflict handling, and focused tests. Full rewriting through arbitrary helpers is not a credible promise |
| Retire environment mutation from `jet add` and `jet remove` | Keep package discovery and realization commands, but require users or Canvas to edit `@env.jet` | About 3-5 engineer-days for CLI, diagnostics, docs, and test migration. It saves machinery but removes a beginner convenience |

The structured option should win. Canvas already applies versioned, source-backed transactions, checks the expected source revision, uses semantic anchors, and applies span edits before an atomic replacement (`crates/jet-devserver/src/Canvas/schema_api.rs:440-480,966-1029`; `crates/jet-devserver/src/Canvas/project_transactions.rs:102-188`; `crates/jet-devserver/src/Canvas/source_model.rs:22-76`). `jet add` and `jet remove` should reuse that source-edit contract. They must stop when no unique literal anchor exists. They must not create a hidden package database or append a second environment contribution.

The flake bridge remains bounded. It currently sorts and deduplicates facts, renders `module env.dev`, reports unmapped fields, and prints to stdout without editing the project (`crates/jetpack/src/Bridge.rs:398-429,431-450,722-746`; `crates/jetpack/src/CLI/add_remove_push_image.rs:434-437`). Under option B it should emit a canonical `@env.jet` with one pure `fn env` and one direct `Environment{...}` return, plus the same loss report. This is a 3-5 engineer-day renderer and test change after the `Environment` schema exists. It is not a round-trip editor and must not invent code for unmapped effects.

## Required `Environment` surface

The entry decision is achievable only if the return type can express the real work. The type must grow in staged slices, but each slice must use the same value and composition law.

| Parity target | Required typed surface | Repository evidence |
|---|---|---|
| `devShells.default` | Computed `tools: [Pkg]`; typed package outputs such as `bin`, `lib`, and `share`; symbolic project-root values; environment variables with set, unset, prepend, and append operations; executable wrappers with declared tools, arguments, checks, and diagnostics; target-conditioned library paths; ordered, trust-gated activation actions with a once scope; prompt and shell presentation | The flake derives `jetRuntimePath`, `jetTzdb`, two wrappers, `JET_ROOT`, `TZDIR`, Linux library paths, one cleanup action, and `JET_ENV_DISABLE` (`flake.nix:21-35,62-90,110-157`) |
| `devShells.full` | Named profiles with `extends` and deterministic merge; explicit profile selection; target predicates; build-tool versus host/runtime-tool roles; derived package outputs; multiple library-path contributions; activation messages; provenance for every included and omitted tool | The full shell adds 30 package expressions, five Linux-only tools, Raylib and Vulkan library paths, the same guarded cleanup, and a banner (`flake.nix:159-283`) |
| JetOS system | `systems: [String:SystemEnvironment]`; typed option registry and merge priority; services, sockets, timers, unit dependencies, readiness, restart, hardening, drop-ins, and masks; users, groups, shadow secret references, and user profiles; managed files with owner, group, mode, and mutable-conflict policy; disks, partitions, filesystems, mounts, and persistence; typed PAM stacks; network and firewall facts; kernel, initrd, bootloader, and specialisations; source-backed hardware snapshots; ordered activation graph, health gates, atomic generation switch, rollback, and receipts; images, fleets, and VM tests | `EnvPlan` already carries systems, images, fleets, and VM tests (`crates/jet-env-model/src/ModuleEval/Types.rs:532-564`), but `SystemPlan` flattens the system to four fields and string options (`crates/jet-env-model/src/ModuleEval/Types.rs:289-314`). Epoch 9 cards and current JetOS writers cover the listed domains |

Several supporting types follow from that surface:

- `PkgOutput` must be a content-addressed handle with typed `bin`, `lib`, `share`, and named output paths. It must not be a host path string.
- `PathContribution` must model search paths and platform library paths as ordered typed values. It must not require shell interpolation.
- `ExecutableWrapper` must describe the executable, argument forwarding, supplied tools, environment changes, preconditions, and a registered diagnostic. Evaluation creates the wrapper value; it does not run it.
- `ActivationAction` must name a checked `#Job` or finite action, dependencies, phase, once policy, and authority. Activation runs it only after `E1255`.
- `EnvironmentProfile` must compose packages, variables, paths, wrappers, services, and actions with deterministic merge and provenance.
- `SystemEnvironment` must replace string-key recovery with typed records for services, accounts, storage, PAM, boot, hardware, files, networking, activation, and user profiles.

The current JetOS implementation confirms why the last item matters. It parses option keys and string values back into storage and user structures (`crates/jetpack/src/JetOS/module_storage_workload.rs:48-107`; `crates/jetpack/src/JetOS/user_flatpak_perf.rs:13-102`). Service generation also reads open-record strings to find an executable (`crates/jetpack/src/JetOS/activation_provenance.rs:111-137`). `fn env` would make those strings easier to generate, but only real nested types make the result safe, searchable, and refactorable.

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
| Make pure bounded `fn env` return `Environment` | Recommend. It makes the ordinary Jet mechanism visible, supports literals and expert computation, and retains an inspectable result contract |

The recommended path needs the exact purity contract above. The empty effect row is part of the public signature, not a documentation promise. Recorded context inputs are immutable arguments. Operations that acquire or change ambient state belong in declared tasks, build actions, detection commands, or activation engines.

## Invariant and mission fit

### I8: one mechanism

Keeping today's declarative form beside `fn env` would create two ways to define the same environment. The migration must replace the module/directive authoring surfaces with the producer in one coherent change. A literal remains available as the function's direct return expression, so beginner syntax does not require a builder.

The canonical mechanism should be: one package-wide `fn env` produces one typed `Environment` value. A literal, builder chain, helper call, and imported module are ordinary expression and layout choices inside that mechanism. The legacy directive reader and typed `module env.*` authoring surface must not survive the migration (`crates/jetpack/src/CLI/realize.rs:661-692`).

### I7: named owner gates

Ordinary `fn`, calls, methods, structs, and `if` are existing Jet syntax. A compiler-known producer named `fn env` still adds user-typeable surface and a reserved entry contract. It requires an owner-ratified decision for its signature, discovery, duplicate law, effects, inputs, result, diagnostics, default, and command behavior. It also amends the current rule that `jet env` never runs a project function (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1498`).

Plain `env.jet` conflicts with `D-ECO-FILEROOT1=A` (`docs/spec/syntax-decisions.md:6118-6122`). `@env.jet` is the right home because it extends the ratified optional role-file pattern and sorts beside the other entry points. `D-ENVFN1` must amend the closed name set on card #1866. `package.jet` remains plain and owns Package facts. The public `Environment` records and builders also need ratified names because `D-ECO-ENV1=A` did not choose their schema (`docs/spec/syntax-decisions.md:4409-4413`).

### Beginner and expert

For a beginner, `@env.jet` and `fn env` form one visible answer to “what does `jet env` do?” A direct `return Environment{...}` keeps the common case literal and short. `jet env info` remains the no-code inspection view.

For an expert, pure helpers, typed builders, package-output paths, declared target input, and target conditionals provide the required control. The expert escape should be explicit when it crosses into effects: a named task, build action, wrapper, or activation hook, not an ambient capability available during environment discovery.

This also improves explanation. `jet env info` can show the final value, each contributing source, selected branches, declared inputs, and lifecycle actions before entry. The current plan already carries source files, graph identity, reads, active environment, and provenance (`crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

### AI-assisted development

A known result type improves completion, refactoring, repair determinism, and context use because tools can navigate one schema and validate the final value. The compiler-known function improves search and source navigation. Bounded evaluation has predictable work, but automated package edits become ambiguous when helpers or control flow own the package list. The structured-anchor rule above contains that cost (`crates/jetpack/src/EnvFile.rs:1-22,114-195`; `crates/jet-env-model/src/ModuleEval/Types.rs:532-610`).

## Migration reality

If the owner chooses environment-as-code, the greenfield rule still requires one canonical form and deletion of the replaced form. The migration should not preserve a declarative `env.jet` beside `fn env`, keep the legacy `pkg.*` reader as a fallback, or add aliases.

Under the recommendation, one coherent migration would:

1. Add the package-wide `fn env(ctx: EnvContext) -> Environment -[]>` contract and optional `@env.jet` home. More than one candidate is an error. No candidate returns the stock empty `Environment`.
2. Define the immutable context and evaluation-input receipt. Preserve finite fuel and reject every non-empty effect row.
3. Define the complete typed `Environment` result in the staged parity order above. Make packages an ordinary computed field, not static sugar (`crates/jet-env-model/src/ModuleEval/mod.rs:6-15`).
4. Keep `package.jet` as the Package root. Require explicit imports into the producer. Do not auto-import every discovered module.
5. Model wrappers, package-output paths, target-conditioned paths, profiles, systems, and lifecycle actions as typed values. Run actions only after trust (`crates/jetpack/src/CLI/run_enter_dev.rs:1484-1529`).
6. Replace `jet add` and `jet remove` environment mutation with the bounded structured edit. Refuse computed or ambiguous package ownership with source provenance.
7. Make `jet bridge flake` emit canonical `@env.jet` producer source and an exact loss report while preserving its stdout-only behavior (`crates/jetpack/src/Bridge.rs:398-450,722-746`).
8. Remove the legacy `EnvFile` parser/renderer and typed `module env.*` authoring path. Migrate every in-repo caller, fixture, example, document, and test in the same change (`crates/jetpack/src/EnvFile.rs:1-22,114-258`).
9. Delete plain `env.jet` and every replaced spelling after all repository consumers move. Do not ship a compatibility parser, alias, or dual discovery path.

If the owner chooses effectful `fn env`, the same deletion rule applies, but the project must first replace the trust-preview order and define cache, sandbox, offline, termination, and source-edit contracts. Those are design requirements, not later implementation details.

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

The owner can make the choice with one question: should normal Jet function syntax be the visible producer of the environment while discovery remains pure?

If yes, choose pure `fn env`, `@env.jet`, explicit imports, and immutable recorded `EnvContext` inputs.

If the function must also probe or execute ambient work, choose the effectful option and accept a trust-before-preview model, weaker caching, and undefined offline behavior until a new protocol exists.

The evidence favors the pure producer. The flake and JetOS gaps need more computation and a much fuller schema, but none requires ambient behavior during discovery. Existing decisions point to one typed `Environment`, one Package graph, pure computed contributions, separate action functions, and optional marked function homes (`docs/spec/syntax-decisions.md:4397-4413,5963-5979,6118-6122,6963-6977`).

## Findings

1. The root `env.jet` covers only 20 of 24 default-shell package expressions and 20 of 50 full-shell expressions; it also omits every admitted hook, wrapper, derived-path, and full-profile behavior (`env.jet:1-17`; `flake.nix:110-283`).
2. The apparent data-versus-code choice is false. Jet already evaluates most environment fields with a pure, deterministic, fuel-bounded interpreter. `fn env` improves visible language unity and discoverability; schema coverage remains the main delivery gap (`crates/jet-env-model/src/ModuleEval/mod.rs:6-19`; `crates/jet-comptime/src/Comptime/Interpreter.rs:15-23,299-328`).
3. A pure producer preserves the pre-trust boundary. An effectful producer creates a trust-preview cycle because the plan is evaluated before `E1255`, while current effects execute afterward (`crates/jetpack/src/CLI/run_enter_dev.rs:1436-1529`; `crates/jetpack/src/Trust.rs:1000-1105`).
4. Current authoring already has two mechanisms: typed modules and the legacy directive reader. The legacy whole-file editor cannot safely evolve into an arbitrary-code editor (`crates/jetpack/src/CLI/realize.rs:661-692`; `crates/jetpack/src/EnvFile.rs:1-22,114-258`).
5. Plain `env.jet` is on a ratified removal path. `@env.jet` is a different role: an optional producer home that must amend the closed `D-ROLEFILE1` set while `package.jet` remains the Package root (`docs/spec/syntax-decisions.md:5963-5969,6118-6122`).
6. Tower card #2155 is a live implementation blocker, not evidence for either source shape. A source redesign does not fix reference collision or package realization.
7. JetOS strengthens the pure producer case and the schema warning. Current `SystemPlan` has only four top-level fields and string options, while Epoch 9 requires typed services, accounts, disks, PAM, activation, hardware, and profiles (`crates/jet-env-model/src/ModuleEval/System.rs:30-89`).

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
|---|---|---|
| F1: Repository environment lacks default and full flake parity | no-action | audit-only: decision input; the owner forbade card creation |
| F2: Pure computed environment values already have a ratified semantic base | decision | D-MODCOMPUTE1=A |
| F3: Arbitrary pre-trust environment execution conflicts with the current authority boundary | decision | D-ENVFN1 option boundary proposed in this report; no board write |
| F4: Typed and legacy authoring/editing surfaces are already split | no-action | audit-only: migration consequence; the owner forbade implementation and cards |
| F5: `env.jet` is not the ratified long-term ecosystem root | decision | D-ECO-FILEROOT1=A |
| F6: Current repository environment cannot be entered for separate parser and realization reasons | card | #2155 |
| F7: JetOS needs function-scale composition and a complete typed system result | decision | D-ENVFN1 draft in this report; no board write |
<!-- /audit-dispositions -->

## Limits and unresolved design work

The final public `Environment` names remain owner-gated. `D-ECO-ENV1=A` explicitly leaves schema and placement open (`docs/spec/syntax-decisions.md:4409-4413`). This revision names the minimum type families and laws needed for parity. Follow-on ballots must settle exact field and constructor names before implementation.

The shipped command-entry mechanics and ratified command-role rulings are temporarily out of sync. Tower card #1866 records the newer direction, but its implementation criteria remain open. This report treats that ratified direction as design authority and current code as implementation state.

The local entry probe stopped at sandbox-sensitive Hangar migration `E2604`, so this audit did not freshly reproduce the two #2155 failures. No other material fact remained undetermined.

## Proposed ballot

### D-ENVFN1 — Environment producer contract

**Group:** syntax

**Gist:** Choose how a package defines the typed environment that `jet env`, dev shells, and JetOS consume.

**Lesson:** An environment producer runs during project discovery and returns data. Jet must inspect that data before it asks the user to trust packages or actions. A pure producer can use functions, conditions, imports, and typed builders over immutable recorded inputs. Its `Environment` result can describe later actions without running them. An effectful producer can inspect the live machine, but then preview, caching, trust, and offline use depend on code that has already run. This choice sets the source shape, function authority, file home, import law, and editing contract.

**Story:** Mara opens the Jet repository. She needs the normal compiler shell today, the full FFI profile tomorrow, and a JetOS workstation image later. She opens `@env.jet` and expects one checked function to show how all three values are composed.

**In the wild:**

```jet
// @env.jet
use "./environment/tools" as tools
use "./environment/workstation" as workstation

fn env(ctx: EnvContext) -> Environment -[]> {
    browser_tools :: if ctx.target.os == .Linux { tools.linux_browsers } else { [] }

    return Environment{
        tools: tools.default + browser_tools,
        variables: [String:EnvValue]{
            "JET_ROOT": .ProjectRoot,
            "TZDIR": tools.tzdata.share.join("zoneinfo"),
        },
        wrappers: [tools.jet, tools.jetpack],
        activation: [
            ActivationAction{ job: tools.clean_nix_tmp, phase: .Enter, once: .PerEnvironment },
        ],
        profiles: [String:EnvironmentProfile]{ "full": tools.full },
        systems: [String:SystemEnvironment]{
            "workstation": workstation.system(ctx.hardware),
        },
    }
}
```

#### Option A — Keep a declarative environment value

Keep environment configuration as a typed Package or `Config` fact. Fold plain `env.jet` into `package.jet` under `D-ECO-FILEROOT1=A`. Expand the schema until it covers dev shells and JetOS. Pure helper calls may compute individual fields, but no compiler-known `fn env` exists.

Technical detail: tools retain a direct literal anchor for edits and can inspect common facts with less control-flow tracing. The source still reads as a special manifest surface. Readers must learn that most fields secretly use the comptime evaluator. Large JetOS composition stays split across discovered Config contributions and their merge law.

```jet
// package.jet
environment: Environment{
    tools: [nixpkgs.rustc, nixpkgs.ripgrep],
    prompt: "jet",
    profiles: [String:EnvironmentProfile]{ "full": full_profile },
}
```

#### Option B — Pure `fn env` returns `Environment`

Add one package-wide `fn env(ctx: EnvContext) -> Environment -[]>`. Its optional home is `@env.jet`. The function uses explicit imports and the fuel-bounded evaluator. It receives immutable recorded inputs and returns one complete typed value. It can describe wrappers, actions, systems, and hardware profiles, but it cannot run or probe them.

Technical detail: this option amends `D-ROLEFILE1=A` from four marked homes to five. Discovery scans the package for one matching function. A duplicate is an error. No function selects the stock empty `Environment`. `package.jet` remains the Package root. Automatic module imports are rejected because pre-trust inspection needs an explicit source closure. Every observed context input enters the evaluation receipt. `jet add` and `jet remove` use one semantic literal anchor or refuse. `jet bridge flake` emits this direct producer form and a loss report. The old directive and `module env.*` authoring paths retire in the same migration.

```jet
// @env.jet
use "./environment/common" as common

fn env(ctx: EnvContext) -> Environment -[]> {
    return common.environment(ctx)
}
```

#### Option C — Effectful `fn env`

Add the same package-wide entry and file home, but let its effect row grant filesystem, network, process, environment, time, random, or hardware authority. This gives the producer immediate access to live probes and arbitrary generators. It also moves trust before preview or permits untrusted effects before `E1255`.

Technical detail: Jet must define a pre-evaluation sandbox, effect receipts, termination rules, and a cache policy for hidden inputs. Offline mode must either reject such producers or define per-effect fallbacks. Content-addressed caching cannot use the normal key unless every effect result becomes a complete recorded input. `BuildContext` proves this model is possible, but it also proves that the context is effectful and policy-gated.

```jet
// @env.jet
fn env(ctx: EnvContext) -> Environment -[FS, Net, Exec]> {
    root :: ctx.exec(["git", "rev-parse", "--show-toplevel"])
    hardware :: ctx.probe_hardware()
    catalog :: ctx.fetch("https://packages.example/catalog")
    return Environment{ project_root: root, hardware, tools: catalog.default_tools }
}
```

**Comparisons:**

- Nix uses functions and local bindings to produce declarative derivations. Reproducibility comes from controlled inputs and outputs, not from manifest-shaped source.
- Starlark looks like a normal programming language but removes ambient filesystem, network, and clock access so evaluation stays deterministic.
- Jet `BuildContext` uses a typed context and typed plan, but it also has recorded I/O and gated ambient effects. Only its entry shape transfers safely here.

**Recommendation:** B.

**Why:** Option B makes “this is Jet” visible and gives JetOS enough composition power. The empty effect row, immutable context, fuel budget, and typed return preserve preview, trust, caching, and offline semantics. `@env.jet` follows the ratified role-file pattern. Explicit imports keep the pre-trust source closure inspectable.

**Why not A:** It preserves the easiest text edit, but it keeps a manifest-shaped teaching surface and hides the ordinary Jet evaluator. JetOS composition remains harder to locate and explain.

**Why not C:** It copies the authority of `BuildContext` into an earlier trust phase. The environment would need to run before Jet can show what the user is trusting.

**Accepted tradeoff:** Tools must evaluate the pure producer to know its selected result. `jet add` and `jet remove` can edit only a unique semantic literal anchor and must refuse computed ownership.
