# Package and environment ecosystem research archive

> **Status, 2026-07-14:** the Deck/Roles/Fold proposal and its later atomic
> ballots are archived research. Seven focused live ballots now decide graph
> scope, declaration shape, source control, extension authority, composition,
> realization, and JetOS lifecycle. `project.jet` is an option only in D-ECO-SOURCE1; current
> `pkg.jet` / `env.jet` / `workspace.jet` law remains until that vote. Nothing
> below authorizes syntax.

## Archived three-framework comparison

## Current architecture under decision

The new ballots test one coherent model, not unrelated syntax samples:

1. D-ECO1 chooses whether package through JetOS share one semantic graph.
2. D-ECO-DECL1 chooses one ordinary Jet shape for every project part.
3. D-ECO-SOURCE1 alone may replace the current role-file division.
4. D-ECO-EXTENSION1 chooses how third parties produce normal typed nodes.
5. D-ECO-COMPOSE2 chooses a finite, order-independent composition law.
6. D-ECO-RECEIPT2 chooses one action, output, receipt, and generation record.
7. D-ECO-JETOS2 chooses build, proof, activation, and rollback behavior.

Ratified resolver, variant, BuildContext, and policy laws are inputs to these
ballots. They are not reopened. Moving a role contribution cannot change its
semantic identity; explicit relative paths and generated-module workspace paths
retain their existing path-sensitive laws.

The beginner lens is still small:

```text
jet run
jet test
jet dev
```

The exact lens reveals the same graph rather than a different configuration
language:

```text
jet workspace members
jet package inspect todo
jet explain todo.cli@wasm,release
jet lock why text_parser
```

The shared teaching example is a todo project with one core library, command,
web page, test, and development environment. Each comparison uses that same
example. A ballot may add a concept only when the example needs it.

### Community evidence that shaped the atomic ballots

- Cargo workspaces prove the value of one lock and shared metadata. Cargo's
  feature-unification and mixed minimum-Rust-version discussions show why a
  workspace must not become a hidden second resolver.
- Nix flakes prove the value of explicit inputs, outputs, development shells,
  checks, and locks. Nix issue discussions also document output-schema
  restrictions, repeated system parameters, and duplicated input trees.
- pnpm proves that checked workspace patterns and catalogs reduce repetition.
  Catalog adoption and update issues show the cost of splitting one graph
  across YAML, package JSON, protocols, and publishing rewrites.
- Go workspaces provide simple local source selection. Go issues show how a
  workspace can hide undeclared dependencies or differ from the published
  module graph.
- Bazel proves the value of stable target addresses, platforms, toolchains,
  queries, and remote action identity. Its BUILD and Starlark ceremony argues
  for typed Jet values rather than a second configuration language.

Primary references:

- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Cargo features 2.0 discussion](https://github.com/rust-lang/cargo/issues/8088)
- [Cargo mixed workspace MSRV issue](https://github.com/rust-lang/cargo/issues/14414)
- [Nix flake parameter repetition](https://github.com/NixOS/nix/issues/5663)
- [Nix flake output-shape discussion](https://github.com/NixOS/nix/issues/3966)
- [pnpm catalog update issue](https://github.com/pnpm/pnpm/issues/8641)
- [Go workspace tutorial](https://go.dev/doc/tutorial/workspaces)
- [Go workspace undeclared-dependency issue](https://github.com/golang/go/issues/60430)
- [Go workspace vendoring issue](https://github.com/golang/go/issues/60056)

The archived proposal asked for one decision covering the entire package and
environment system. Everything below used this repo's `flake.nix` as one parity
fixture. The current atomic ballots replace that decision structure.

The three frameworks share one semantic mechanism — typed **role modules**
(`module <role>.<name> { … }`, U3) merged into reserved namespaces. They differ
on exactly one axis the owner named: **the file model.**

- **Deck** — one `project.jet` holds every tier.
- **Roles** — the ratified reserved-file model (`pkg.jet` / `env.jet` /
  `workspace.jet` / `config.jet` + discovered role files), told as one story.
- **Fold** — start as one `project.jet`; `jet split` lifts any tier into its
  conventional file when it earns one. Placement is style, not semantics.

Because the mechanism is shared, spellings are defined once (§Mechanism); each
framework chapter is a **file-layout** chapter: the complete port, the growth
arc, its trade-offs. New syntax is tagged `(new — D-ECOn)`; everything else
reuses ratified spellings.

---

## Glossary

| Term | One line |
|---|---|
| **payload** | Publishable identity of a manifest: `name`, `version`, `edition`, toolchain `jet:` pin. |
| **package** | A buildable output; **is** a top-level `module` (U10); declares a target (executable/library/…). |
| **module** | Named top-level block contributing typed fields to a namespace; many per file; merged. |
| **source** | A named provider ref feeding names into a scope: `pkgs: github@NixOS/nixpkgs/nixos-unstable`. |
| **provider** | Backend that resolves refs (core, nix, cran, npm…); `provider@target` selects one. |
| **hangar** | Content-addressed local store at `/etc/jet/hangar/`; every realized output lives here. |
| **overlay** | Reviewed override set in `workspace.jet`: provider swaps, patches, package customization. |
| **catalog** | Shared dependency versions in `workspace.jet`; packages opt in via visible `deps`. |
| **target** | Build kind of a package (`executable`, `library`, `test`, `benchmark`) and/or platform. |
| **env** | A dev shell: `module env.<name>` — tools on PATH, env vars, prompt, services, hooks. |
| **system** | A whole jetos host: `module system.<name>` — target, packages, services, options. |
| **image** | An OCI container or jetos installer input: `module image.<name>` built `from:` a package/system. |
| **fleet** | A set of hosts + a rollout policy: `module fleet.<name>` — many systems deployed together. |
| **hook** | Trust-gated Jet code run at env activation; the expert escape below typed env fields. |
| **wrap** | A package output wrapped with PATH prefix + env defaults (replaces Nix `wrapProgram`). |

---

## Mechanism (shared by all three frameworks)

Every flake construct gets exactly one spelling. Frameworks decide *which file*
a construct lives in, never *how it is spelled* (I8).

### Package identity, build, check, wrap, alias

The flake builds `jet` with `buildRustPackage` (`doCheck = true`), wraps it
(`--prefix PATH`, `--set-default TZDIR`), and exposes a second `jetpack` app on
the same binary.

```jet
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }

sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }

packages: {
    jet: executable {
        build:   Recipe.cargo(lock: "Cargo.lock"),                  // (new — D-ECO2)
        check:   true,                                              // (new — D-ECO17) doCheck: run
                                                                    // test targets during realize
        // runtime PATH deps: `jet build`/`run` shell out to these (flake jetRuntimePath)
        runtime: [ pkgs.[rustc, stdenv.cc, lld],                    // (new — D-ECO11)
                   pkgs.ruby, pkgs.php, pkgs.R.with([cran.jsonlite]) ],
        wrap:    Wrap.{ path_prefix: self.runtime,                  // (new — D-ECO3)
                        env: ["TZDIR": pkgs.tzdata.zoneinfo] },     // accessor (new — D-ECO19)
        meta:    Meta.{ description:  "Compiler for the Jet programming language",
                        homepage:     "https://github.com/jet-language/jet",
                        main_program: "jet", platforms: .Unix },    // (new — D-ECO5)
    },
    jetpack: alias(packages.jet, bin: "jetpack"),                   // (new — D-ECO4) apps.jetpack
}
```

**Desugaring (U10):** each `packages:` map entry elaborates to a top-level
package *module* — a package IS a module; the map is compact sugar over the
same mechanism, and the compiler treats both identically. `pkgs.stdenv.cc` is
the flake's cc wrapper verbatim — provider refs are nixpkgs attr paths, so no
translation is needed (the dev shell separately lists `gcc`, matching the flake
exactly). `check: true` maps `doCheck`: the package's `test` targets (D-TGT1–4)
run during realize; standalone `jet test` needs no field. Other packages
reference these fields cross-namespace exactly as `from: packages.server`
already does (ratified, U14 fixture) — `packages.jet.runtime` is a first-class
ref. That reference form, not shared `let` bindings, is how tiers share values
across files (load-bearing for Fold; see its split law).

### The nixpkgs-lib override question (`rWrapper.override { packages = [jsonlite] }`)

**One canonical spelling in all frameworks**: the `.with([…])` combinator
(new — D-ECO8). Frameworks differ only in *where* a reviewed, reusable use of
it lives — inline at the use site, or in a `workspace.jet` overlay
(D-JPK-OVERLAY1's `package("…")` selector with the same combinator):

```jet
pkgs.R.with([cran.jsonlite])                        // inline, one-off
```
```jet
overlay dev { package("R").with += [cran.jsonlite] } // reviewed, reusable — same combinator
```

The current `env.jet` stopgap listing `R` and `cran.jsonlite` as two loose
entries is **not** parity: it puts jsonlite on PATH but never wires it into R.
`.with` closes that gap.

### The dev shell — the canonical `env.dev` block

Owner ruling: typed declarative fields cover the common case; one trust-gated
`hook` is the expert escape. This block is written **once**; each framework
places it, never restates it.

```jet
module env.dev {
    packages: [
        pkgs.[ cargo, rustc, gcc, gnat, fpc, dart, powershell, gfortran, gnucobol,
               go, jdk, dotnet-sdk_8, tcl, lua5_4, lld, ruby, php, qemu, nodejs_22,
               nixfmt, ripgrep, jq, gh, fd, bashInteractive, zsh, fish, util-linux,
               wasm-tools, tree-sitter, emscripten, lldb, pkg-config, raylib ],
        pkgs.R.with([cran.jsonlite]),
        tool.jet, tool.jetpack,
    ]
    platform.linux: [ pkgs.[chromium, gtk4, bubblewrap] ]           // (new — D-ECO10)

    // typed hook fields — (new — D-ECO6)
    prompt:         "jet"                                           // ratified (D-FE-PROMPT1)
    env_vars:       [ "TZDIR": pkgs.tzdata.zoneinfo, "JET_ROOT": git.root ]
    library_path:   [ pkgs.raylib ]                                 // LD_LIBRARY_PATH
    git_hooks_path: "scripts/githooks"                              // D-CI3 wiring
    banner: Banner.lines([                                          // typed GUARANTEE: renders to
        "Jet dev shell",                                            // stderr; stdout stays grep-clean
        "  build:    cargo build",
        "  run:      jet run examples/features/basics/hello.jet",
        "  package:  jetpack help",
        "  search:   rg \"pattern\" docs Source tests",
        "  LSP:      jet lsp        (tests: cargo test --test lsp)",
        "  editor:   editors/vscode/install.sh   (Cursor/VS Code)",
        "            editors/zed/install.sh        (Zed dev extension)",
        "  debug:    jet debug <file.jet>  (native lldb backend: tests/debug.rs)",
        "  release:  nix build .#jet",
    ])

    // dev-loop wrappers (flake mkJetDevBin, both wrappers) — (new — D-ECO9)
    // root: resolve bin relative to project root (JET_ROOT env overrides, else
    // this comptime value). missing: the exact error the flake prints when the
    // binary is absent — "jet: no debug binary at <bin>" / "fix: cargo build" —
    // carried as typed data, not hook code.
    tools: {
        jet:     Tool.wrap(bin: "target/debug/jet",     path_prefix: packages.jet.runtime,
                           root: git.root, missing: "cargo build"),
        jetpack: Tool.wrap(bin: "target/debug/jetpack", path_prefix: packages.jet.runtime,
                           root: git.root, missing: "cargo build"),
    }

    // expert escape: trust-gated Jet code for what typed fields can't say — (new — D-ECO7)
    hook: fn(sh: Shell) {
        if !sh.env.has("JET_NIX_TMP_CLEANED") {
            run("scripts/agent/clean-nix-tmp.sh")
            sh.env.set("JET_NIX_TMP_CLEANED", "1")
        }
    }
}
```

`platform.linux:` desugars to a ratified comptime `if target.os == .Linux`
guard (computed modules) — experts write the `if` directly. `git.root` is a
comptime builtin resolving the git top-level, replacing the flake's
`git rev-parse` dance (new — D-ECO18). `Tool.wrap`'s `root:` + `missing:`
fields carry the flake's root-walk fallback and missing-binary error as typed
data — the same spelling in every framework.

### Per-system, multi-shell, formatter

- **Per-system** (`eachDefaultSystem`): native, not a construct. Jetpack is
  tier-1 Linux/macOS/Windows (D-JPK-PLATFORM1); `target.os`/`target.arch` gate
  platform lists. No wrapper exists or is needed.
- **Multi-shell**: `module env.dev`, `module env.ci`, `module env.docs` —
  ratified namespace (U3). `jet env ci` enters one.
- **Formatter** (`formatter = nixfmt`): `jet fmt` is native for `.jet`;
  `formatter: pkgs.nixfmt` passthrough exposes non-Jet formatters to
  `jet fmt --lang nix` (new — D-ECO12).

### Already-ratified coverage & scope-outs

Headline features other package managers ship, answered from existing law —
nothing silent:

- **Scripts / task runner** (npm scripts, cargo xtask): ratified — `#Task fn`
  beside `fn run()` (D-JPK-TASKRUN1), invoked `jetpack run <name>`, scheduled
  with `#Every(…)` (D-SCHEDULE1). No new surface needed.
- **Feature flags / conditional compilation** (cargo features): ratified —
  `Build.{ features }` in build profiles (D-BUILDPROFILE1) plus comptime
  `if target.*` for platform conditionals.
- **Workspace-internal version protocol** (pnpm `workspace:*`): covered —
  members address each other by ratified member refs (D-MONOREF1); shared
  versions live in `catalog:` (D-JPK-CATALOG1).
- **Install lifecycle hooks** (npm postinstall) — **scoped out, by design.**
  Arbitrary code on install is the supply-chain hole the trust law closes
  (D-WD1, D-JPK-GRANTSCHEMA1). Package build code is `fn build(b)` — Tier-1
  pure by default, Tier-2 only via `#Impure` + explicit grant (D-BUILDENTRY1).
  The env-activation `hook` (D-ECO7) is the only user hook, and it is
  trust-gated. A framework wanting npm-style install hooks must re-ballot the
  trust law; none of the three does.

### Coverage check vs `flake.nix` — zero unmapped

| flake construct | spelling |
|---|---|
| `inputs` + pins | `sources: { pkgs: github@…/nixos-unstable }` |
| `buildRustPackage` | `packages.jet: executable { build: Recipe.cargo }` |
| `cargoLock.lockFile` | `Recipe.cargo(lock: "Cargo.lock")` |
| `doCheck = true` | `check: true` |
| `wrapProgram --prefix PATH` | `wrap: Wrap.{ path_prefix: self.runtime }` |
| `--set-default TZDIR` | `wrap: Wrap.{ env: ["TZDIR": pkgs.tzdata.zoneinfo] }` |
| `meta` (desc/home/mainProgram/platforms) | `meta: Meta.{ … }` |
| `apps.default` / `apps.jetpack` | `packages.jet` / `alias(packages.jet, bin:)` |
| `jetRuntimePath` (rustc, stdenv.cc, lld, ruby, php, R) | `runtime: [ pkgs.[rustc, stdenv.cc, lld], … ]` |
| `rWrapper.override { packages }` | `pkgs.R.with([cran.jsonlite])` |
| `devShell.packages` (incl. `gcc`) | `env.dev.packages` |
| `lib.optionals stdenv.isLinux` | `platform.linux:` / comptime `if` |
| `shellHook` env exports (JET_ROOT, TZDIR) | `env_vars:` + `git.root` |
| `LD_LIBRARY_PATH` (raylib) | `library_path:` |
| `git config core.hooksPath` | `git_hooks_path:` |
| banner (all 10 lines, stderr) | `banner: Banner.lines([…])`, typed stderr guarantee |
| `clean-nix-tmp.sh` guard | `hook: fn(sh)` |
| `mkJetDevBin` root-walk + JET_ROOT override | `Tool.wrap(root: git.root)` + JET_ROOT env override |
| `mkJetDevBin` missing-binary error + fix line | `Tool.wrap(missing: "cargo build")` |
| `writeShellScriptBin` jetDev / jetpackDev | `tools: { jet: …, jetpack: … }` |
| `eachDefaultSystem` | native multi-platform (no construct) |
| `formatter` | `jet fmt` + `formatter:` passthrough |

Every field has a spelling; L0204 is closed on paper in all three frameworks.
`flake.nix` stays the working mechanism until the runtime lands.

---

## Framework 1 — Deck (one file)

**Thesis:** a project is one `project.jet`. Every tier is a block or module
inside it. Discovery is trivial because there is nothing to discover.

### Beginner's first file

```jet
// project.jet — the whole thing.
packages: { hello: executable }
```
`jet run` still needs **no file at all** for a lone script (U7). A
`project.jet` appears only when you want a name, deps, or an env.

### The jet repo, fully ported

```jet
// project.jet
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }

packages: { /* the §Mechanism packages block, verbatim */ }

module env.dev { /* the §Mechanism env.dev block, verbatim */ }
```

That is the entire file: identity plus the two §Mechanism blocks. ~90 real
lines.

**U10 note:** U10 says `env.jet` is never a package index. Under Deck,
`env.jet` doesn't exist; the rule's substance — the *env module* never indexes
packages — holds (`env.dev` contains no package index). U10's wording still
needs amendment from filename to module; part of D-ECO1's Deck outcome.

**Trust granularity:** grants stay per-role-module, not per-file — the grant
graph (D-WD1) keys on `env.dev` vs `system.build-box`, and `policy.trust`
(D-JPK-GRANTSCHEMA1) names roles the same way in one file or many. What Deck
loses is file-level *review* granularity: an env-hook change and a
system-activation change share one blame, one CODEOWNERS entry, one diff.
Admitted in the adversarial section.

### Growth arc (blocks accrete, one file)

```jet
// + a subpackage + shared versions:
catalog:  { textkit: "1.4.0" }
packages: { jet: executable { … }, jetpack: alias(…),
            lsp: library { deps: { textkit: catalog.textkit } } }

// + a second shell:
module env.ci { packages: [ pkgs.[cargo, rustc, gcc, lld] ]  git_hooks_path: "scripts/githooks" }

// + a jetos host. Option keys use the ratified D-JPK-OSNS1 namespaces; the
//   ordered dotted `options:` list belongs to the jetos system schema (a
//   different construct from map literals like `env_vars:` — both spellings
//   are shipped law):
module system.build-box {
    target: linux.x64
    packages: [ pkgs.[git, ripgrep, ccache] ]
    services: { openssh: { enable: true, ports: [22] } }
    options: [
        network.hostName:     build-box,
        filesystem.timeZone:  "Europe/London",
        packages.shell:       pkgs.fish,
        boot.loader:          limine,
        users.nate:           { groups: [wheel], shell: pkgs.fish },
    ]
}

// + an OCI image (all ratified U14 fields):
module image.server {
    kind: .Oci
    from: packages.jet
    expose:   [8080, 8443]
    env_vars: ["RUST_LOG": "info", "TZDIR": pkgs.tzdata.zoneinfo]
    files:    ["config/app.toml"]
    // base: oci("…") — captured; realization gated on TLS (ratified U14 state)
}

// + a fleet (new — D-ECO15). No flake anchor exists; the acceptance fixture is
//   the owner's ~/nixos halcyon host (card #337 parity matrix):
module fleet.prod {
    hosts:   [ build-box: system.build-box, halcyon: system.halcyon ]
    rollout: Rollout.{ strategy: .Rolling, batch: 1 }
    health:  "curl -sf http://127.0.0.1:8080/health"   // gate before next batch
    proof:   .Vm                                        // D-WD8 risk-class proof
}
```

### Framework answers

- **Override:** `.with([…])` inline; the reviewed `overlay` form lives in the
  same file's workspace-role block when wanted.
- **Multi-shell:** stacked `module env.<name>` blocks.
- **CLI:** `jet env ci`, `jet build jet`, `jet image server`,
  `jet os switch build-box` — verbs address blocks by module/package name; one
  file to open.
- **Expert file:** the whole port — every knob in one scroll.
- **Hybrid:** the single file is both surfaces; two lines for a beginner, more
  blocks for an expert.

### What this overturns

- `env.jet` / `workspace.jet` / `config.jet` as **reserved filenames** — gone
  (D-ECO1). Reserved *namespaces* stay. U10's filename wording amended.
- D-JPK-OSHOST1's `./config.jet` host discovery → `project.jet`.
- `find("./packages")` demoted: one file rarely discovers siblings.
- `jet init` writes `project.jet` (amends D-JPK-FILENAME2 wording).

### Kill-criteria check

- Hollow defaults? No — beginner file is two lines.
- Dictate file structure? **Yes** — one file is the structure. Direct tension
  with philosophy "flexible structure."
- Invariant carve-out? No.

### Adversarial

1. **Growth cliff at scale.** A 20-package monorepo in one file is unreadable
   and a merge-conflict magnet. The single-file virtue inverts into a
   bottleneck exactly when the team is largest.
2. **Violates structural flexibility.** Philosophy §"one mechanical path,
   flexible structure" says layout is the user's choice; Deck makes it the
   framework's. The one place the constitution explicitly forbids dictation.
3. **Review and trust blast radius.** Per-module grants survive, but review
   does not: a banner tweak and a `system.*` activation change land in the
   same file, same blame, same CODEOWNERS line. High-authority tiers (system,
   fleet) deserve isolated diffs; Deck structurally denies that.
4. **U7 boundary blurs.** "A file is a complete program" vs "a project is one
   file" are easy to conflate; beginners add `project.jet` reflexively and
   erode the zero-ceremony path.

---

## Framework 2 — Roles (reserved files, sharpened)

**Thesis:** each tier has a home. `pkg.jet` is identity + packages, `env.jet`
the dev shell, `workspace.jet` the monorepo index, `config.jet` the jetos host
root (D-JPK-OSHOST1=C); images and fleets are role modules in any discovered
`.jet`. This is the ratified status quo told as one story. **Overturns
nothing** — the baseline the others are measured against.

### Beginner's first file

Same as Deck: `jet run script.jet` needs nothing (U7). First manifest is
`pkg.jet`, one line: `packages: { hello: executable }`.

### The jet repo, fully ported

```jet
// pkg.jet
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }
packages: { /* the §Mechanism packages block, verbatim */ }
```
```jet
// env.jet
module env.dev { /* the §Mechanism env.dev block, verbatim */ }
```

`env.dev`'s `tools:` reach `packages.jet.runtime` across files through the
ratified cross-namespace ref form (`from: packages.server` precedent) — no
binding is ever shared between files (U3 holds).

### Growth arc (a file per tier)

```jet
// workspace.jet — appears with the second package:
module workspace {
    members: find("./packages")
    catalog: { textkit: "1.4.0" }                                 // D-JPK-CATALOG1
    overlay dev { package("R").with += [cran.jsonlite] }          // D-JPK-OVERLAY1 + D-ECO8
    policy:  { trust: { default: prompt, ci: { prompt: deny } } } // D-JPK-GRANTSCHEMA1
}
// packages/lsp/pkg.jet: payload + packages: { lsp: library { deps: { textkit: catalog.textkit } } }
```
```jet
// config.jet — jetos host root (D-JPK-OSHOST1=C: `jet os switch build-box`
// discovers system.build-box here):
module system.build-box { /* the Deck arc's system block, verbatim */ }
```
```jet
// image.jet, fleet.jet — conventional names; role comes from the declaration:
module image.server { /* the Deck arc's image block, verbatim */ }
module fleet.prod   { /* the Deck arc's fleet block, verbatim */ }   // D-ECO15
```

### Framework answers

- **Override:** the same `.with([…])` combinator; its reviewed home is the
  `workspace.jet` overlay.
- **Multi-shell:** `env.dev` in `env.jet`; more `module env.<name>` blocks
  there or in any discovered file.
- **CLI:** `jet env`, `jet build`, `jet image server`,
  `jet os switch build-box` — each verb knows its file; `jet info <thing>`
  names the owner.
- **Expert file:** every tier its own reviewable file — own blame, own
  CODEOWNERS entry, isolated trust diffs.
- **Hybrid:** beginner opens one-line `pkg.jet`; expert navigates a small tree
  of the same modules, sharded.

### What this overturns

Nothing (S52, U10, D-JPK-MODBODY1, D-WORKSPACE1, D-JPK-OSHOST1). The
§Mechanism spellings (D-ECO2–12, 15–19) are additions every framework needs
equally.

### Kill-criteria check

- Hollow defaults? No.
- Dictate file structure? **Partially** — reserved filenames are mandatory
  homes. Softened: image/fleet modules live in *any* discovered file, and
  `find` lets members go anywhere.
- Invariant carve-out? No.

### Adversarial

1. **Ceremony arrives early; teach cost is highest.** A three-package repo
   already wants `pkg.jet` + `env.jet` + `workspace.jet`; a jetos host adds
   `config.jet`. Four filenames and two placement rules before page one of the
   tutorial ends — the steepest teach-in-one-page cost of the three, against
   priority #2.
2. **Placement rules are inconsistent (I8 friction).** Envs, workspaces, and
   hosts are role-by-*filename* (`env.jet`, `workspace.jet`, `config.jet`);
   images and fleets are role-by-*declaration* in any file. Two rules for one
   job; newcomers must learn which tier follows which.
3. **Reserved-filename rigidity.** `env.jet` must be `env.jet`; a user wanting
   `dev-shell.jet` can't have it, even though the module name already carries
   the role everywhere else.
4. **Discovery cost.** "What does this project define" means reading a tree,
   not opening one file — worse onboarding and review for small projects.

---

## Framework 3 — Fold (one file that splits on growth)

**Thesis:** the file model is the *user's* choice, backed by one mechanism.
Start with one `project.jet` (identical to Deck). When a tier earns its own
file, `jet split env` extracts `module env.dev` into `env.jet`; `jet fold env`
reverses it. The compiler merges every discovered role module regardless of
file. Reserved filenames become the *convention the tooling prefers* — where
`jet split` puts things — never a requirement. Philosophy §"one mechanical
path, flexible structure" says code layout and file structure are the user's
choice; Fold is that sentence applied literally.

### The split law (D-ECO13) — precise, or it doesn't ship

- `jet split <tier>` moves **whole modules** (plus each module's attached
  leading comments) into the tier's conventional file. Module bodies move
  byte-identically; computed fields (`platform.linux:` sugar, comptime `if`)
  move verbatim, never desugared. The formatter-roundtrip law applies: a
  STABILITY test pins `split` → `fold` → byte-identical original, comments
  included.
- **Cross-module top-level bindings block split.** If a top-level `let` is
  referenced both by a module being moved and by anything staying behind,
  `split` **refuses** with a dedicated diagnostic (E13xx: names the binding,
  both referents, and the fix — route the value through its namespace field,
  e.g. `packages.jet.runtime`, or inline it). U3 stands: modules never import
  each other, and no file ever references another file's bindings.
- Stated plainly: **reversibility is guaranteed for whole modules only.** A
  file whose modules share top-level bindings must be restructured before it
  can split. The §Mechanism port needs no restructuring — shared values
  already flow through `packages.jet.runtime` refs, so this repo splits clean
  — but the guarantee is conditional, not absolute.

### Beginner's first file

Two lines in `project.jet` (or zero files — U7). It never splits until asked.

### The jet repo, ported — two equivalent states

**Folded:** byte-identical to Deck's `project.jet`.
**Split** (`jet split`, once):

```
project.jet      # payload + sources + packages   (== Roles pkg.jet content)
env.jet          # module env.dev                 (== Roles env.jet)
workspace.jet    # module workspace               (appears when members > 1)
config.jet       # module system.*                (matches D-JPK-OSHOST1)
image.jet        # module image.server
fleet.jet        # module fleet.prod
```

Same program in both states; `jet fmt` keeps both canonical; `find` resolves
modules wherever they live.

### Growth arc (tool-driven, reversible)

```
jet split env        # env.dev → env.jet
jet new package lsp  # scaffolds packages/lsp/, auto-indexes in workspace members
jet split system     # system.build-box → config.jet
jet split image      # image.server → image.jet
jet split fleet      # fleet.prod → fleet.jet
```
The module bodies are the Deck arc's, verbatim — placement is the only delta.
A solo dev folds forever; a team shards fully; same semantics.

### Framework answers

- **Override:** `.with([…])` inline while folded; `jet split overlay` promotes
  the same combinator into `workspace.jet`'s reviewed `overlay`.
- **Multi-shell:** `module env.<name>` anywhere, folded or split.
- **CLI:** identical verbs to Roles plus `jet split`/`jet fold`; `jet info`
  reports tier + current file, since file is mutable.
- **Expert file:** the expert chooses — one file or a full tree — and the
  choice carries zero semantic weight.
- **Hybrid:** this *is* the hybrid pass — one mechanism (role modules +
  `find`), two ergonomic surfaces (folded/split), converted by one reversible
  command.

### What this overturns

- Reserved filenames become **preferred convention, not requirement** (amends
  S52/D-JPK-TWONAMES1 "reserved" wording; D-JPK-OSHOST1 discovery generalizes from
  `./config.jet` to "the file holding `system.<host>`, conventionally
  `config.jet`") — D-ECO1.
- Adds `jet split`/`jet fold`, whole-tree role discovery, and the
  split-refusal diagnostic — D-ECO13.

### Kill-criteria check

- Hollow defaults? No — two-line start, magic scaffolding.
- Dictate file structure? **No — the opposite.** The only framework that makes
  layout the user's choice, satisfying the philosophy text directly.
- Invariant carve-out? No. U3 is preserved by the split-refusal law.

### Adversarial

1. **`jet split`/`fold` is formatter-grade tooling with its own bug class.**
   Byte-preserving extraction, comment attachment, merge-back — it must
   round-trip perfectly or it silently corrupts manifests (`jet fmt` silently
   corrupted serde markers for months; same law, same risk). The STABILITY pin
   is mandatory, not optional.
2. **The reversibility guarantee is conditional.** Whole modules only; shared
   top-level bindings refuse with a fix. Honest, but it means `split` can say
   "no" — a beginner hitting E13xx on their first split meets the framework's
   sharpest edge at the worst moment. The diagnostic's fix line carries the
   whole UX weight.
3. **"Where is X" needs a tool.** A module may live in any file, so "which
   file defines env.dev" requires `jet info`, where Roles guarantees it by
   name. Flexibility trades away a locational guarantee.
4. **Two visible states in the wild.** New users see folded repos and split
   repos and must learn both plus the converting command — arguably more to
   learn than either pure model, against priority #2.
5. **Convention drift.** If filenames are only preferred, real repos scatter
   `module env.*` into oddly named files; the ecosystem loses the "open
   `env.jet`" muscle memory that makes Roles skimmable.

---

## Comparison

| Capability | Deck | Roles | Fold |
|---|---|---|---|
| Pins / sources | `sources:`, one file | `sources:` in `pkg.jet` | either |
| Packages (+build/check/wrap/alias) | block | `pkg.jet` | either |
| Named shells | stacked blocks | `env.jet` | anywhere |
| Hooks (typed fields + escape) | same spelling | same spelling | same spelling |
| Override (`.with`) | inline (+ in-file overlay) | overlay in `workspace.jet` | inline → split to overlay |
| Subpackages | blocks in one file | dirs + `workspace.jet` | `jet new` + auto-index |
| Catalog | `catalog:` block | `workspace.jet` | `workspace.jet` (convention) |
| System (jetos) | block | `config.jet` (OSHOST1) | any file / split → `config.jet` |
| Image (OCI) | block | discovered role file | any file / split |
| Fleet | block | discovered role file | any file / split |
| Tasks / scripts | `#Task fn` (ratified) | same | same |
| Single-file U7 | preserved | preserved | preserved |
| Teach in one page | **easiest** | hardest | medium (+ `split`) |
| Structural flexibility (philosophy) | **worst** | partial | **best** |
| File dictates? | one file | reserved names | **no** |
| Overturns ratified law? | filenames, OSHOST1, U10 wording | none | "reserved" → convention |

---

## Archived reviewer recommendation — superseded

This recommendation compared file layouts only. D-ECO-SOURCE1 now owns that
choice; Fold is not a live option or recommendation.

Adversarial review reframed the axis: the real choice is **fixed file model vs
user-chosen file model**. Fold subsumes the other two as states — folded *is*
Deck, split *is* Roles — so picking Deck or Roles is picking one Fold state
and forbidding the other. Deck independently trips the dictate-file-structure
kill criterion, and the philosophy's own text (I8: "code layout, file
structure … are the user's choice") reads as Fold's thesis verbatim.

**Recommendation: Fold, with the split law of D-ECO13 ratified as written —
whole-module moves, byte-identical round-trip pinned by a STABILITY test,
split-refusal diagnostic for shared bindings.** It is the only framework the
philosophy's file-structure sentence permits without amendment, and its worked
port splits clean because cross-file value flow uses ratified namespace refs,
never shared bindings. If the owner rejects the conditional reversibility
guarantee, Roles is the fallback — it overturns nothing and needs no new
tooling.

---

## Archived parity inventory

This table records what the earlier comparison attempted to cover. It is not a
ballot. Rows already ratified in Tower remain ratified records. Conflicting or
missing spec entries require explicit reconciliation; this archive cannot
silently settle them.

| ID | Decision | Deck | Roles | Fold |
|---|---|---|---|---|
| **D-ECO1** | **Archived file-model comparison; source layout now lives in D-ECO-SOURCE1** | one `project.jet` | reserved role files | folded→split |
| D-ECO2 | `build: Recipe.cargo(lock:)` as first-class package build field | yes | yes | yes |
| D-ECO3 | `wrap: Wrap.{ path_prefix, env }` (replaces `wrapProgram`) | yes | yes | yes |
| D-ECO4 | `alias(package, bin:)` (replaces `apps.*` second binary) | yes | yes | yes |
| D-ECO5 | `meta: Meta.{ description, homepage, main_program, platforms }` | yes | yes | yes |
| D-ECO6 | Typed env fields: `env_vars`, `library_path`, `git_hooks_path`, `banner` (typed stderr guarantee) | yes | yes | yes |
| D-ECO7 | Expert trust-gated `hook: fn(sh: Shell)` running Jet code | yes | yes | yes |
| D-ECO8 | `.with([…])` combinator — ONE spelling for package customization; overlay is its reviewed placement | yes | yes | yes |
| D-ECO9 | `Tool.wrap(bin:, path_prefix:, root:, missing:)` dev wrappers | yes | yes | yes |
| D-ECO10 | `platform.linux:` sugar desugaring to comptime `if target.os` | yes | yes | yes |
| D-ECO11 | `runtime:` package field (runtime PATH deps; referencable as `packages.<n>.runtime`) | yes | yes | yes |
| D-ECO12 | `formatter:` passthrough for non-`.jet` formatters | yes | yes | yes |
| D-ECO13 | `jet split`/`jet fold`, root-level role discovery, split-refusal diagnostic, STABILITY round-trip pin | n/a | n/a | **required** |
| D-ECO14 | Reaffirm U7: a lone script never needs a project file, in every model | reaffirm | reaffirm | reaffirm |
| D-ECO15 | `module fleet.<name>` spelling (`hosts:`, `rollout:`, `health:`, `proof:`) — un-freezes fleet surface (D-JETOS-FREEZE1) | yes | yes | yes |
| D-ECO16 | `module system.<name>` at equal depth now — un-freezes system surface (D-JETOS-FREEZE1) | yes | yes | yes |
| D-ECO17 | `check: true` package field (`doCheck` parity: run test targets during realize) | yes | yes | yes |
| D-ECO18 | `git.root` comptime builtin (git top-level path) | yes | yes | yes |
| D-ECO19 | Package output accessors (`pkgs.tzdata.zoneinfo`) | yes | yes | yes |

D-ECO15/16 flag that giving fleet and system equal depth (owner scope ruling)
re-opens surface frozen by D-JETOS-FREEZE1 — a real ballot, not a silent
inclusion. Fleet has no flake anchor; its acceptance fixture is the owner's
~/nixos halcyon host (card #337 parity matrix).
