# Project, package and config files today: a map and what confuses people

Status: research, for card #6368. Written 2026-10-30 against dev `e784bb1ffe`.
Companion proposal: `Docs/proposals/project-package-config-2026-10-30.md`.

## Summary

Jet has no single answer to "where does this setting go?" today. The same fact can be written in two to six places, the same word means three or four different things, and several ratified rulings disagree with each other and with the code.

- **Files.** Twelve file names carry configuration or command code: `package.jet`, `pkg.jet` (retired but still read), `workspace.jet`, `env.jet`, `config.jet`, `build.jet`, `test.jet`, `dev.jet`, `bench.jet`, `run.jet`, `.jet/lock` and the ratified-but-unbuilt `jet.lock`. On top of those come an inline `package { }` block and freely named `configs:` files. `fn build` can sit in any file of a package, including `tools/build_entry.jet` and `workspace.jet`.
- **Fields.** `package.jet` accepts 25 top-level fields. They mix four kinds of fact: what the package *is* (name, version), what it *needs* (deps, effects), what it *makes* (outputs), and how *this checkout* works (profiles, members, environments, toolchain, policy ceilings). Nothing in the file tells you which kind a field is.
- **Words.** "Workspace" means the monorepo root (`workspace.jet`), the editor window (Canvas), and, in Jai, one compilation with its own options. The owner has ruled that it moves to "project". "Profile", "target", "build" and "package" are each overloaded too.
- **Rulings.** Three rulings named the root differently (Package, Jetspace, workspace). Two rulings place `fn build` differently. Two rulings order settings layers differently. Today's D-MANIFEST-ONE1 folds `workspace.jet` and `env.jet` into `package.jet`, which makes one file mean both "a package" and "a group of packages".
- **A real bug from that overlap.** A root `package.jet` with a `name:`, its own `run.jet` and `members:` silently skips its own code: `jet check` passes with a broken root `run.jet`, and `jet run` runs a member's program instead (probe P3 below).
- **Peers.** Every peer that scales well separates "what a package is" from "how this checkout builds" (Cargo by root-only profiles, Go by `go.work`, pnpm by `pnpm-workspace.yaml`, Gradle by `settings.gradle`, Bazel by `MODULE.bazel` + `BUILD` + `.bazelrc`). Zig, SwiftPM, Gradle and Nix all let real code drive the build, but keep one fixed file for it. Bazel retired its `WORKSPACE` file in Bazel 8.

Section 8 turns these findings into eight requirements that the proposal answers.

## 1. Every file that carries configuration

Counts are `git ls-files` at `e784bb1ffe`. "Read by" names the code that gives the file meaning.

| File | What it holds today | Count | Read by | Ruling state |
|---|---|---|---|---|
| `package.jet` | identity, deps, outputs, profiles, settings, policy, members, and more (section 2) | 2310 | `Jetpack/PackageModel/Source/Manifest/PackageParse.jet` (one parser, 521 lines) | D-CONF-PLANE1, D-MANIFEST-ONE1 |
| inline `package { }` block | the same fields, inside a source file (all of `Core/` uses it, e.g. `Core/Build/Build.jet:1-10`) | 604 files | same parser | D-ECO-INLINEPACKAGE1; may not coexist with `package.jet` |
| `pkg.jet` | retired name for `package.jet`, still a project marker | 5 | `JetCli/Source/Cli/Request.jet:232`, `Entry.jet:74` | retired, still recognised |
| `workspace.jet` | `module workspace { members: … }`, overlays, sometimes `fn build` or even `fn run` | 22 | `JetFoundation/Data/Syntax/JetpackConfig.jet:484`, `Entry.jet:70` | D-WORKSPACE1=B said "no workspace file"; D-MANIFEST-ONE1=A folds it into `package.jet` (not built) |
| `env.jet` | `module env.<name> { sources: … packages: … prompt: … }`: dev tools and source aliases | 159 | `JetpackConfig.jet:469` | D-MANIFEST-ONE1=A folds it into `package.jet environments:` (not built) |
| `config.jet` and `configs:` files | `NAME :: Config{ … }` records: deps, environments, even `outputs:` (`Examples/features/tooling/compiler_api_package/config/release.jet`) | 30 + 2 users of `configs:` | `PackageParse.jet` (E: ConfigMembers forbids members, build, settings) | D-MANIFEST-ONE1 keeps `configs:` for env-only files |
| `build.jet` | `fn build(b: Build)`; `jet new` writes a commented stub | 27 | `JetCli/Source/Cli/Entry.jet:72`, `Loader.jet:2613` | D-CONF-ENTRY1: one per package, any file |
| `test.jet` | `fn test(t: Test)` | 8 | same | D-TEST-PROFILE1: only for code suites |
| `dev.jet` | `fn dev(d: Dev)` | 6 | same | D-ENTRY-SHAPE1 |
| `bench.jet` | old bench entry | 3 | none (Bench handle retired, D-ENTRY-NAMES1) | stale |
| `run.jet` | the stock program entry, `fn run()` | 1695 | `Entry.jet:77` | D-ECO14 |
| `.jet/lock` | toolchain pin, build stamp, dependency graph | 22 committed; `jet new` writes and git-ignores it | `Host.jet:1607,1618`, `InspectProvenance.jet:308` | D-LOCKFILE1=A moved it to a committed root `jet.lock` (not built) |
| `jet.lock` | the ratified lock | 0 | none yet | D-LOCKFILE1=A |
| `.jet/` | build output, cache, generated code, last-run records | 6 committed `.jet/last-run/*` files in `Examples/features/tooling/build_entry_discovery` | everything | D-WORKSPACE-CMD1: one `.jet/` at the root, disposable |
| `project.jet` | one stray fixture | 1 | none | none |

The command code that `fn build` holds can live in any file of a package. A census of top-level `fn build|test|dev(` declarations finds them in `main.jet` (55 files), `run.jet` (36), `build.jet` (25), `workspace.jet` (3), `package.jet` (3) and others, such as `tools/build_entry.jet`.

`JetCli/Source/Cli/Entry.jet:72-74` reserves nine names that user code cannot use as module files: `run.jet`, `build.jet`, `dev.jet`, `test.jet`, `package.jet`, `pkg.jet`, `env.jet`, `config.jet` and `workspace.jet`.

## 2. Every field of `package.jet`

The parser's switch (`PackageParse.jet:160-215`) accepts these fields. The census column counts `package.jet` files (not inline blocks) that use the field. The last column sorts each field by the question it answers. That sorting is this document's own, and it drives the proposal.

| Field | Meaning | Files | Kind of fact |
|---|---|---|---|
| `name`, `version` | identity | 2305, 2265 | what the package **is** |
| `edition`, `jet`, `source` | language edition, toolchain requirement, source root | 389, 92, few | is |
| `description`, `license`, `repository` | publishing metadata | 114, 109, 95 | is |
| `deps` | dependencies | 177 | what it **needs** |
| `authority` | effects the package may use, grants to its deps, trust | 1494 | needs |
| `boundaries` | which modules may import which | 7 | needs (code rules) |
| `policy` | lints and code rules (`lints: { deny: [auto_derive] }`, `check_removal: .Forbid`, `sentries`) | 47 | needs (code rules) |
| `allocator`, `runtime` | program allocator and runtime flavour | 5, 12 | what it **makes** |
| `outputs` | the things the package makes (nine kinds, D-TARGET-ONE1) | 94 | makes |
| `packages` | an older map of output kinds (`packages: { backend: library }`) | 80 | makes (second home) |
| `targets` | platform profiles (`targets: { machine: { profile: "hosted" } }`) | 6 | makes (and a third meaning of "target") |
| `settings` | typed knobs the package declares | 8 | makes |
| `build` | named build profiles, plus `allow:` for build-time effects | 35 | how **we work** |
| `defaults` | which output `jet run` uses; which profile bare commands use | 18 | how we work |
| `dev`, `services` | dev-loop policy, local services | 0, 0 | how we work |
| `environments` | dev tools and shells | 1 | how we work |
| `members` | the packages this root groups | 5 | how we work |
| `configs` | extra files holding `Config{}` records | 2 | how we work |
| `test`, `suites` | ratified by D-TEST-PROFILE1, not accepted yet (E1206) | 0 | how we work |
| `dev_deps`, `patch`, `workspace` | reserved; must be empty | 0 | — |
| `grants` | retired, moved under `authority:` | 0 | — |

One more spelling sits beside these: a `package.jet` may also declare the package's single `fn build`, and an inline `NAME :: Config{ … }` record.

The fourth kind ("how we work") is the trouble. None of these fields describe the package to someone who depends on it. They describe one checkout, one team or one machine. Cargo reached the same split and enforces it by ignoring a dependency's profiles (section 7).

## 3. Every concept and word

| Word | Meanings in use today | Where |
|---|---|---|
| package | (1) the unit with a `package.jet`; (2) the root noun for a group of packages (D-ECO-ROOTNAME1=I); (3) the old `packages:` output map; (4) env.jet `packages:`, which means tools | ratified rulings, `PackageParse.jet:186`, `env.jet` files |
| workspace | (1) the monorepo root and `workspace.jet` (D-WORKSPACE-CMD1, D-BUILD-REUSESET1, spec "Workspace overlays" §2360); (2) the Canvas editor window (`Docs/spec/reference/canvas-workspace-architecture.md`); (3) in Jai, one compilation with its own build options (`compiler_create_workspace`) | owner ruling on #6368: rename (1) to project |
| Jetspace | the whole checked graph (D-ECO-ROOTNAME2=D) | ratified, never used in code |
| project | the folder the CLI and Canvas call "the project" (`project-navigation.js`, `Request.jet:232 JET_CLI_PROJECT_MARKERS`) | used informally everywhere, defined nowhere |
| member | a package listed by a root | `members:` in `package.jet` or in `module workspace` |
| output | a thing a package makes (D-TARGET-ONE1) | `outputs:`, `b.add_*`, `b.output(name)` |
| target | (1) the platform (D-TARGET-ONE1); (2) the `targets:` platform-profile field; (3) old meaning "a thing to build", still in `Core/Build/Build.jet` `BuildGraphTarget`; (4) the field name `target`, which is an error | parser, Core, flags |
| profile | (1) a named bundle of build options (D-CONF-WORD1); (2) a test profile (D-TEST-PROFILE1, same namespace); (3) a target platform profile (`targets: { … profile: "hosted" }`); (4) env presets (renamed by D-CONF-WORD1) | `build:`, `test:`, `targets:` |
| build | (1) the `build:` profiles field; (2) `Build{ … }`, the profile constructor (until `BuildOptions` lands, D-ENTRY-NAMES1); (3) `Build`, the handle in `fn build(b: Build)`; (4) `build.jet`; (5) `jet build`; (6) the `build.*` hook stages | `Examples/features/tooling/build_link_inputs` uses (1), (2), (3) and (4) together |
| config | (1) `Config{}` records; (2) `config.jet`; (3) `configs:` | parser |
| settings / options | (1) `settings:`, knobs a package declares; (2) build options (`BuildOptions`, D-SETTING-ORDER1's one option table) | spec, rulings |
| policy / authority / grants / boundaries | four blocks about what is allowed: code rules, effects, effect grants, import rules | `package.jet` |
| environment | dev tools and shells: `env.jet`, `environments:`, `Config{ environments: }` | three homes |
| lock | `.jet/lock` (code, git-ignored by `jet new`) and `jet.lock` (ratified, committed) | two homes |

## 4. The rulings in force, and where they disagree

| Ruling | Says | Conflicts with |
|---|---|---|
| D-WORKSPACE1=B (06-25) | Reuse the package file; no workspace manifest | 22 `workspace.jet` files, spec §3905 ("`workspace.jet` supplies workspace membership") |
| D-ECO-ROOTNAME1=I | The root noun is "Package" | D-ECO-ROOTNAME2=D ("Jetspace"), D-WORKSPACE-CMD1 and D-BUILD-REUSESET1 ("workspace"), the owner's "project" |
| D-BUILDSCOPE1=A (07-19) | `fn build` lives in the unit's existing definition file at every rung (single file, `pkg.jet`, `workspace.jet`) | D-CONF-ENTRY1=A: any file of the package; `jet new` writes `build.jet` |
| D-CONF-SPLIT1=A | Facts in text, actions in `fn build` | D-ENTRY-SHAPE1=A: `fn dev` sets facts in code (`d.port(8080)`); D-TARGET-ONE1=A: outputs from data and from code |
| D-CONF-MERGE1=A | declaration < profile < workspace < environment < system < fleet < CLI | D-SETTING-ORDER1=A: stock < source marker < `package.jet` < entry code < flag; D-BUILD-REUSESET1=A adds a root layer between stock and each member |
| D-SETTING-ORDER1=A, owner directive #4913 | every option row has a flag, a package field and a handle method | D-OPTFLAG1=A removed `--optimize`, `--opt-level` and `--inline` in favour of profiles |
| D-TEST-PROFILE1=A | named test runs are data in `package.jet` (`test:`), code suites in `test.jet` | the parser rejects `test:` (probe P1); `jet new`'s `test.jet` stub still says "replace the stock default" |
| D-LOCKFILE1=A | `<root>/jet.lock`, committed | `jet new` writes `.jet/lock` and git-ignores it (probe P4) |
| D-MANIFEST-ONE1=A (10-30) | `package.jet` is the one project file; a root without `name:` is a virtual root | makes a `package.jet` mean "a package" or "a group" depending on one missing field; keeps `configs:` files |
| D-ENTRY-NAMES1=A | `Build` is the handle; the profile record is `BuildOptions` | 35 `package.jet` files still use `Build{ … }` as the profile record |

## 5. The confusions, with examples

Each confusion names a real file or probe. They are ordered by how often a newcomer meets them.

**C1. One project, many homes.** `Examples/features/tooling/build_entry_discovery` is one small project spread over seven configuration places: `package.jet` (identity, authority), `workspace.jet` (members, and also `fn run`), `tools/build_entry.jet` (the root's `fn build`, found "package-wide" per its `tools/README.md`), `packages/foundation/tools/build.jet` (a member's `fn build`), two member `package.jet` files, and six committed `.jet/last-run/*` files. Nothing in `package.jet` points at any of the others. A reader must know the discovery rules to find the build.

**C2. A root is both a package and a group.** `Compiler/package.jet` has `name: "compiler"`, a version and 13 `members:`. Its comment says `jet build Compiler/` runs `build.jet`, which builds an executable from the JetCli member. So is `compiler` a package with code, a group, or both? Probe P3 shows the cost: a named root with `members:` and its own `run.jet` is silently skipped by `jet check` and `jet run` (bug B1).

**C3. One fact, several homes.**
- Members: `members:` in `package.jet`, or `module workspace { members: }` in `workspace.jet`. Probe P3 accepts both at once, listing the same member twice, with no warning.
- Environments: `env.jet` `module env.<name>`, `Config{ environments: }` in a `configs:` file (`Examples/jetpack/epoch5/config/dev.jet`), or `environments:` in `package.jet`.
- Outputs: `outputs:`, the older `packages:` map, `Config{ outputs: }` in a configs file, or `b.add_*` in `fn build`.
- The entry function: any file of the package. The census found `fn build`, `fn test` or `fn dev` in `main.jet`, `run.jet`, `build.jet`, `workspace.jet`, `package.jet` and `tools/build_entry.jet` (section 1).
- The lock: `.jet/lock` or `jet.lock`.

**C4. Three syntaxes for one kind of data.** Top-level `field: value` (`package.jet`), `module workspace { field: value }` (`workspace.jet`, `env.jet`), and `NAME :: Config{ field: value }` (`config.jet`). A fourth, the inline `package { }` block, holds the first syntax inside a source file. The output constructor is spelled `.Executable{ … }` in `package.jet` and `Executable{ … }` in a configs file.

**C5. "Workspace" collides with Jai.** In Jai a workspace is one compilation with its own build options, created by `compiler_create_workspace` ([The Way to Jai, ch. 30](https://github.com/Ivo-Balbaert/The_Way_to_Jai); `Docs/audits/jai-parity-2026-10-16.md` §D). Jet's equivalent of that is one output with its options (D-JAI-CONFIG1). Jet uses the same word for the monorepo root, and Canvas uses it for the editor window. A user reading Jai material and Jet material at once meets two meanings for the word.

**C6. "Build" names six things.** `Examples/features/tooling/build_link_inputs/package.jet` writes `build: { vendored: Build{ optimize: none, link_args: ["-Lvendor"], os: linux } }`. Its `build.jet` writes `fn build(b: Build)`. Here `build:` is a profile map, `Build{}` is a profile record, `Build` is a handle and `build.jet` is a file, all in one example.

**C7. "Target" and "profile" are overloaded.** `targets: { machine: { profile: "hosted" } }` uses both words in a third and a fourth sense. A field named `target` is an unknown-field error. Test profiles and build profiles share one namespace (D-TEST-PROFILE1) but live under different fields.

**C8. Facts and code overlap without a rule.** D-CONF-SPLIT1 says "facts in text, actions in code". Yet the default output is `defaults: { run: cli }` or `b.default(x)`, a profile is `build: { … }` or `b.profile(…)`, an option is a field or a handle call, and a test suite is `suites:` data or `t.suite(…)`. Each pair is ratified. No rule says which to pick, so two teams write the same project two ways.

**C9. The settings order has two versions and a hidden layer.** D-CONF-MERGE1 lists seven layers, D-SETTING-ORDER1 lists five, and D-BUILD-REUSESET1 adds the root's `package.jet` as a layer under each member. A member's effective `optimize` depends on which ruling the implementation follows.

**C10. Inheritance has three different rules.** Members inherit `version`, `edition` and `license` from the root (D-CORE-HEADER1), build settings from the root (D-BUILD-REUSESET1), and policy ceilings through the grant chain (D-BUILDSCOPE1). They do not inherit deps, lints or outputs. A member cannot tell what it inherits without reading three rulings.

**C11. What publishes and what does not is unstated.** A library's `build:` profiles, `environments:`, `members:` and `defaults:` mean nothing to a consumer, but they sit beside `name:` and `deps:` in the file that gets published. Nothing says whether a consumer's build honours them. Cargo answers this explicitly ("Profile settings defined in dependencies will be ignored", [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)).

**C12. Command files are named by convention only, and their stubs are stale.** `jet new --template overrides` writes `build.jet`, `dev.jet` and `test.jet` stubs that say "Uncomment one `fn build` to replace the stock default" (`Host.jet:1514-1516`). D-ENTRY-SHAPE1 says entries set values and never replace the stock work. Nine file names are reserved (section 1), so a module cannot be called `test.jet` or `config.jet`.

**C13. Four blocks say what is allowed.** `policy:` (lints, code rules), `authority:` (effects, grants, trust), `boundaries:` (imports) and `build: { allow: }` (build-time effects, parsed inside the profiles map by `BuildProfiles.jet:310`). The project's ceiling ("this team never allows `unchecked`") has no home of its own; it sits in the root's `package.jet` beside the package's own needs.

**C14. Errors point the wrong way.** Probe P1's E1206 for `test:` says "Identity is bare `name:` and `version:` at the top level — there is no `identity:` or `payload:` wrapper", which has nothing to do with `test:` (bug B2). Probe P5's duplicate `fn build` is E0105 "Imported names share the module namespace" from `jet check` but E3520 "Two build entries" from `jet build` (bug B3).

## 6. Probes

Run with `JET_COMPILER_IDENTITY="jet@0.1.0#probe" JET_TOOLCHAIN_ROOT=<export of e784bb1ffe> jet`, where `jet` is the prebuilt base compiler for that commit.

| Probe | Input | Result |
|---|---|---|
| P1 | `package.jet` with `test: { quick: TestProfile{ suites: [tests], time_limit: 60s } }` | `E1206 Unknown field test:`; the "Why" line talks about identity wrappers |
| P2 | `build: { small: .{ optimize: size } }` | `E1206 … remove the dot before {` |
| P3 | root `package.jet` with `name: "p3"`, `members: ["a"]`, an `outputs:` app and a `run.jet` containing an undefined name; `workspace.jet` also listing `a`; member `a` prints "hi" | `jet check .` and bare `jet check`: "ok: …/a/run.jet has no problems". `jet run`: prints `hi` (the member's program). `jet check run.jet`: E0107 for the root file. Removing `workspace.jet` changes nothing. |
| P4 | `jet new demo` | writes `demo/.jet/lock` and a `.gitignore` containing `.jet/lock` |
| P5 | `build.jet` and `tools/build.jet` both declaring `fn build` | `jet check .`: E0105 at `build.jet:1:4`; `jet build`: E3520 |

## 7. How peers draw the lines

| System | Package data | Build code | Multi-package root | Profiles and flags | Lock |
|---|---|---|---|---|---|
| Cargo | `Cargo.toml` `[package]`, `[dependencies]` | `build.rs`, one fixed file per package | `[workspace]` in the root `Cargo.toml`; a root without `[package]` is a "virtual manifest" | `[profile.*]` honoured only at the workspace root; `.cargo/config.toml` per machine or repo | `Cargo.lock` at the root |
| Go | `go.mod` | none; `go generate` runs by hand and its output is committed | `go.work`, a separate file, "generally inadvisable to commit" | build tags and flags only | `go.sum` |
| Zig | `build.zig.zon`, "metadata specifically pertaining to build.zig" | `build.zig`, real Zig code that builds a step graph | none; a package is the unit | `b.standardOptimizeOption` and `-D` flags declared in `build.zig` | hashes inside `build.zig.zon` |
| SwiftPM | `Package.swift`: Swift code that builds one declarative `Package` value | plugins (build-tool and command plugins), separate packages | none (Xcode workspaces are an IDE concept) | `swift build -c release`; `swiftSettings` per target | `Package.resolved` |
| Bazel | `MODULE.bazel` (identity and deps) | `BUILD` files (Starlark, restricted) and `.bzl` rules | the repository root marked by `MODULE.bazel`; the `WORKSPACE` file was disabled by default in Bazel 8 | `.bazelrc` named configs (`--config=release`) | `MODULE.bazel.lock` |
| Buck2 | `BUCK` files | Starlark rules | cells in `.buckconfig` | `.buckconfig`, modifiers | — |
| Nix flakes | `flake.nix` `inputs` | `flake.nix` `outputs`, a function of the inputs | none; one flake per repo is the norm | outputs per system | `flake.lock` |
| npm | `package.json` | `scripts` strings | `"workspaces"` globs inside the root `package.json` | none | `package-lock.json` |
| pnpm | `package.json` | `scripts` | `pnpm-workspace.yaml`, a separate file that also holds shared version "catalogs" | `.npmrc` | `pnpm-lock.yaml` |
| Gradle | `build.gradle(.kts)` per project | the same file, real Kotlin or Groovy; shared logic in convention plugins (`build-logic`) | `settings.gradle(.kts)`, a separate file that lists included projects | `gradle.properties`; version catalog `libs.versions.toml` | dependency locking, opt-in |
| Jai | none | the metaprogram (`first.jai` / `build.jai`), real Jai code run by the compiler | none | `Build_Options` per workspace | none |

What each teaches Jet:

- **Cargo** proves the split between package facts and checkout facts is real. It enforces it silently ("Cargo only looks at the profile settings in the `Cargo.toml` manifest at the root of the workspace. Profile settings defined in dependencies will be ignored", [profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)). It also shows the cost of one file for two roles: a root `Cargo.toml` can be a package, a workspace or both, and its docs need a "root package" vs "virtual manifest" section to explain the difference ([workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)). `[workspace.package]` and `[workspace.dependencies]` add opt-in inheritance (`version.workspace = true`): explicit, per field. `build.rs` has one fixed name ([build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)).
- **Go** keeps the module file free of checkout concerns and puts multi-module work in `go.work` ([Go modules reference, Workspaces](https://go.dev/ref/mod#workspaces)). It is the cleanest split, but it gives up build code entirely, which Jet will not do.
- **Zig** uses a data file for identity and dependencies and a code file for the build graph, and names the data file after the code file to show they belong together ([build.zig.zon](https://github.com/ziglang/zig/blob/master/doc/build.zig.zon.md), [Zig build system](https://ziglang.org/learn/build-system/)). Options are declared in code (`b.option`), then set by `-D` flags: one mechanism.
- **SwiftPM** writes the manifest as code that builds one declarative value ([PackageDescription](https://docs.swift.org/swiftpm/documentation/packagedescription)). This is close to Jet's "data file is Jet values" design. Plugins extend the build without making the manifest imperative ([Plugins](https://github.com/swiftlang/swift-package-manager/blob/main/Documentation/Plugins.md)).
- **Bazel** splits three concerns into three file kinds: identity and deps (`MODULE.bazel`, [modules](https://bazel.build/external/module)), targets (`BUILD`, [BUILD files](https://bazel.build/concepts/build-files)) and invocation flags (`.bazelrc`, [bazelrc](https://bazel.build/run/bazelrc)). It replaced its `WORKSPACE` file because of its "shortcomings"; "the WORKSPACE file is already disabled in Bazel 8" ([Bzlmod migration](https://bazel.build/versions/8.1.1/external/migration)). Bazel's lesson for Jet is the cost of a word and file that grew several jobs.
- **Buck2** names the multi-repo root a "cell" in `.buckconfig` and keeps targets in `BUCK` files ([Buck2 docs](https://buck2.build/docs/)).
- **Nix flakes** put inputs and outputs in one file, outputs as a function of inputs, with a committed lock ([nix flake](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake)). One file is clear when it holds one project; it does not attempt multi-package grouping.
- **npm** puts `"workspaces"` inside the root `package.json` ([npm workspaces](https://docs.npmjs.com/cli/using-npm/workspaces)); **pnpm** moved it to its own `pnpm-workspace.yaml` ([pnpm workspaces](https://pnpm.io/workspaces)) and added "catalogs", "a workspace feature for defining dependency version ranges as reusable constants" ([catalogs](https://pnpm.io/catalogs)).
- **Gradle** has a separate settings file for the build as a whole and a build file per project ([multi-project builds](https://docs.gradle.org/current/userguide/multi_project_builds_intermediate.html), [organizing projects](https://docs.gradle.org/current/userguide/organizing_gradle_projects.html)). Shared logic moves into convention plugins, and shared versions into a catalog ([version catalogs](https://docs.gradle.org/current/userguide/version_catalogs.html)). Note the naming trap: Gradle calls each sub-unit a "project", which is Jet's package. Jet's docs must define the word once and keep it.
- **Jai** has no package or project file. One metaprogram creates workspaces, sets each one's options and drives the compiler ([The Way to Jai](https://github.com/Ivo-Balbaert/The_Way_to_Jai) ch. 30; `focus` [`first.jai`](https://github.com/focus-editor/focus/blob/main/first.jai)). It is the strongest primitive layer of any peer and has no wrappers at all, so every project re-writes the same message loop (`jai-parity-2026-10-16.md` line 91).

The pattern across peers:

1. The ones that scale keep "what this package is and needs" apart from "how this checkout builds it": Cargo by a root-only rule, Go, pnpm and Gradle by a separate root file, Bazel by three file kinds.
2. Where build code exists, it has one fixed home: `build.rs`, `build.zig`, `build.gradle.kts`, `BUILD`, `first.jai`.
3. The data file and the code file meet through shared value types, not by repeating facts: Zig's `b.dependency("x")` reads `.zon`; SwiftPM's manifest is a value.
4. Shared versions and options across many packages are an explicit, opt-in feature of the root (Cargo `workspace = true`, pnpm catalogs, Gradle catalogs), not silent inheritance.

## 8. What a cleaner system must do

These requirements come from sections 4–7 and from `Docs/spec/philosophy.md` (ladders, not cliffs; one mechanism; enterprise auditability; the language is a library). The proposal answers each one.

- **R1. Two nouns, two files, no overlap.** One noun and one file for the unit others depend on (package). One noun and one file for the folder people work in (project). No file means both.
- **R2. One home per fact.** Every field has exactly one file it may appear in. A beginner can sort any fact with one question.
- **R3. One home per entry function.** `fn build`, `fn test` and `fn dev` each have one fixed file name, found without discovery rules.
- **R4. One settings order.** One list of layers, with pins and policy forbids stated on the same list.
- **R5. Explicit inheritance.** A package states what it takes from its project, or the project states what it gives. No hidden layer.
- **R6. Composition.** Data files and entry code build the same Core values. The primitive (`b.add(output)`, a typed action) can express everything; the wrappers (`outputs:` entries, `b.add_executable`) cover the common cases and expand to the primitive.
- **R7. One meaning per word.** "Workspace" leaves the package system; "build", "target" and "profile" each keep one meaning.
- **R8. A mechanical migration.** `jet fix` moves every file and field, and a leftover old spelling is an error that names the fix.
