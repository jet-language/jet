# Package & environment ecosystem — three frameworks

One decision: the shape of Jet's package/env system — the thing that replaces
Nix flakes 1:1 today and every package manager eventually. Pick one framework
by ballot. Everything below is driven by a single worked example: **this repo's
own `flake.nix`**, ported in full.

The three frameworks share one semantic mechanism — typed **role modules**
(`module <role>.<name> { … }`, U3) merged into reserved namespaces. They differ
on exactly one axis the owner named: **the file model.**

- **Deck** — one `project.jet` holds every tier.
- **Roles** — the ratified reserved-file model (`pkg.jet` / `env.jet` /
  `workspace.jet` + discovered role files), sharpened into one story.
- **Fold** — start as one `project.jet`; `jet split` lifts any tier into its
  reserved file when it earns one. Placement is style, not semantics.

Because the mechanism is shared, the *spellings* are defined once (§Mechanism);
each framework chapter is a **file-layout** chapter showing the complete port,
the growth arc, and its own trade-offs. New syntax is tagged `(new — D-ECOn)`;
everything else reuses ratified spellings.

---

## Glossary

| Term | One line |
|---|---|
| **payload** | Publishable identity of a manifest: `name`, `version`, `edition`, toolchain `jet:` pin. |
| **package** | A buildable output; **is** a top-level `module`; declares a `target` (executable/library/…). |
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

Every flake construct gets exactly one spelling. These are framework-independent;
a framework only decides *which file* they live in.

### Package identity & the compiler package (`packages.default` + `apps` + `wrapProgram`)

The flake builds `jet` with `buildRustPackage`, wraps it (`--prefix PATH`,
`--set-default TZDIR`), and exposes a second `jetpack` app pointing at the same
binary.

```jet
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }

sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }

// runtime PATH deps: `jet build`/`run` shell out to these (flake jetRuntimePath).
let runtime = [ pkgs.[rustc, gcc, lld], pkgs.ruby, pkgs.php, pkgs.R.with([cran.jsonlite]) ]
let tzdb    = pkgs.tzdata.zoneinfo

packages: {
    jet: executable {
        build:   Recipe.cargo(lock: "Cargo.lock"),                 // (new — D-ECO2)
        runtime: runtime,                                          // (new — D-ECO11)
        wrap:    Wrap.{ path_prefix: runtime, env: ["TZDIR": tzdb] }, // (new — D-ECO3)
        meta:    Meta.{ description:  "Compiler for the Jet programming language",
                        homepage:     "https://github.com/jet-language/jet",
                        main_program: "jet", platforms: .Unix },      // (new — D-ECO5)
    },
    jetpack: alias(packages.jet, bin: "jetpack"),                  // (new — D-ECO4) apps.jetpack
}
```

`Recipe.cargo` reuses the ratified `Recipe.*` adapter family (D-JPK-ADAPTNAME1),
promoted from ad-hoc adapters to a first-class package build. `Wrap` folds
`wrapProgram` (PATH prefix + env `--set-default`) into one typed field. `alias`
is the whole `apps.*` block: a second binary name for one output.

### The nixpkgs-lib override question (`rWrapper.override { packages = [jsonlite] }`)

Canonical spelling — the beginner surface is a `.with(...)` combinator on a
package; the expert surface is the same thing spelled as a reviewed overlay in
`workspace.jet` (D-JPK-OVERLAY1). One mechanism, two entrypoints (I8-clean):

```jet
pkgs.R.with([cran.jsonlite])                                       // (new — D-ECO8) inline
```
```jet
// workspace.jet overlay — identical effect, reviewed & reusable:
overlay dev { package("R").withPackages += [cran.jsonlite] }
```

The current `env.jet` stopgap listing `R` and `cran.jsonlite` as two loose
entries is **not** parity: it puts jsonlite on PATH but does not wire it into R.
`.with` closes that gap.

### The dev shell, hooks, and wrappers (`devShell` + `shellHook` + `jetDev`)

Owner ruling: typed declarative fields cover the common case; one trust-gated
`hook` is the expert escape. The whole flake `shellHook` and both
`writeShellScriptBin` wrappers map cleanly:

```jet
module env.dev {
    packages: [
        pkgs.[ cargo, rustc, gcc, gnat, fpc, dart, powershell, gfortran, gnucobol,
               go, jdk, dotnet-sdk_8, tcl, lua5_4, lld, ruby, php, qemu, nodejs_22,
               nixfmt, ripgrep, jq, gh, fd, bashInteractive, zsh, fish, util-linux,
               wasm-tools, tree-sitter, emscripten, lldb, pkg-config, raylib ],
        pkgs.R.with([cran.jsonlite]),
        tool.jet, tool.jetpack,                                    // dev wrappers, below
    ]
    platform.linux: [ pkgs.[chromium, gtk4, bubblewrap] ]          // (new — D-ECO10)

    // typed hook fields (the common case) — (new — D-ECO6)
    prompt:         "jet"
    env_vars:       [ "TZDIR": tzdb, "JET_ROOT": git.root ]
    library_path:   [ pkgs.raylib ]                                // LD_LIBRARY_PATH
    git_hooks_path: "scripts/githooks"                             // D-CI3 hook wiring
    banner:         Banner.lines([                                 // stderr, per repo convention
        "Jet dev shell",
        "  build:   cargo build",
        "  run:     jet run examples/features/basics/hello.jet",
        "  package: jetpack help",
        "  release: nix build .#jet",
    ])

    // dev-loop wrappers: exec the cargo-built debug binary with runtime on PATH — (new — D-ECO9)
    tools: {
        jet:     Tool.wrap(bin: "target/debug/jet",     path_prefix: runtime),
        jetpack: Tool.wrap(bin: "target/debug/jetpack", path_prefix: runtime),
    }

    // expert escape: trust-gated Jet code for anything the typed fields can't say — (new — D-ECO7)
    hook: fn(sh: Shell) {
        if !sh.env.has("JET_NIX_TMP_CLEANED") {
            run("scripts/agent/clean-nix-tmp.sh")
            sh.env.set("JET_NIX_TMP_CLEANED", "1")
        }
    }
}
```

`platform.linux:` (beginner sugar) desugars to a ratified comptime `if
target.os == .Linux` guard on the list (computed modules) — experts may write
the `if` directly. `git.root` is a comptime builtin (git top-level), replacing
the flake's `git rev-parse` dance. `Tool.wrap` is the wrapper case of `Wrap`
over a local binary; the flake's root-walk fallback lives in `hook` if wanted.

### Per-system, multi-shell, formatter

- **Per-system** (`eachDefaultSystem`): native, not a construct. Jetpack is
  tier-1 on Linux/macOS/Windows (D-JPK-PLATFORM1); `target.os`/`target.arch`
  gate platform-specific lists. No `eachDefaultSystem` wrapper exists or is
  needed.
- **Multi-shell** (several named devShells): `module env.dev`, `module env.ci`,
  `module env.docs` — reserved namespace, ratified (U3). `jet env ci` enters one.
- **Formatter** (`formatter = nixfmt`): `jet fmt` formats `.jet` natively;
  `.nix` files use `nixfmt` (already in `packages`). Optional passthrough
  `formatter: pkgs.nixfmt` exposes it to `jet fmt --lang nix` (new — D-ECO12).

### Coverage check vs `flake.nix` — zero unmapped

| flake construct | spelling |
|---|---|
| `inputs` + pins | `sources: { pkgs: github@…/nixos-unstable }` |
| `buildRustPackage` | `packages.jet: executable { build: Recipe.cargo }` |
| `cargoLock.lockFile` | `Recipe.cargo(lock: "Cargo.lock")` |
| `wrapProgram --prefix PATH` | `wrap: Wrap.{ path_prefix: runtime }` |
| `--set-default TZDIR` | `wrap: Wrap.{ env: ["TZDIR": tzdb] }` |
| `meta` (desc/home/mainProgram/platforms) | `meta: Meta.{ … }` |
| `apps.default` / `apps.jetpack` | `packages.jet` / `alias(packages.jet, bin:)` |
| `rWrapper.override { packages }` | `R.with([cran.jsonlite])` / overlay |
| `devShell.packages` | `env.dev.packages` |
| `lib.optionals stdenv.isLinux` | `platform.linux:` / comptime `if` |
| `shellHook` env exports | `env_vars:` |
| `LD_LIBRARY_PATH` | `library_path:` |
| `git config core.hooksPath` | `git_hooks_path:` |
| banner echo | `banner: Banner.lines([…])` |
| `clean-nix-tmp.sh` call | `hook: fn(sh)` |
| `writeShellScriptBin` (jetDev/jetpackDev) | `tools: { … Tool.wrap … }` |
| `eachDefaultSystem` | native multi-platform (no construct) |
| `formatter` | `jet fmt` + optional `formatter:` passthrough |

Every field has a spelling. The `L0204` gap is closed on paper in all three
frameworks (they all use this table); `flake.nix` stays the working mechanism
until the runtime lands.

---

## Framework 1 — Deck (one file)

**Thesis:** a project is one `project.jet`. Every tier is a block or module
inside it. Discovery is trivial because there is nothing to discover.

### Beginner's first file

```jet
// project.jet — the whole thing.
packages: { hello: executable }
```
`jet run` still needs **no file at all** for a lone script (U7 preserved). A
`project.jet` appears only when you want a name, deps, or an env.

### The jet repo, fully ported (one file)

```jet
// project.jet
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }

let runtime = [ pkgs.[rustc, gcc, lld], pkgs.ruby, pkgs.php, pkgs.R.with([cran.jsonlite]) ]
let tzdb    = pkgs.tzdata.zoneinfo

packages: {
    jet: executable {
        build:   Recipe.cargo(lock: "Cargo.lock"),
        runtime: runtime,
        wrap:    Wrap.{ path_prefix: runtime, env: ["TZDIR": tzdb] },
        meta:    Meta.{ description: "Compiler for the Jet programming language",
                        homepage: "https://github.com/jet-language/jet",
                        main_program: "jet", platforms: .Unix },
    },
    jetpack: alias(packages.jet, bin: "jetpack"),
}

module env.dev {           // … the full env.dev block from §Mechanism …
    packages: [ pkgs.[cargo, rustc, gcc, /* …all 35… */ raylib], pkgs.R.with([cran.jsonlite]),
                tool.jet, tool.jetpack ]
    platform.linux: [ pkgs.[chromium, gtk4, bubblewrap] ]
    prompt: "jet"
    env_vars: ["TZDIR": tzdb, "JET_ROOT": git.root]
    library_path: [pkgs.raylib]
    git_hooks_path: "scripts/githooks"
    banner: Banner.lines([ "Jet dev shell", "  build: cargo build", /* … */ ])
    tools: { jet: Tool.wrap(bin: "target/debug/jet", path_prefix: runtime),
             jetpack: Tool.wrap(bin: "target/debug/jetpack", path_prefix: runtime) }
    hook: fn(sh: Shell) { if !sh.env.has("JET_NIX_TMP_CLEANED") {
        run("scripts/agent/clean-nix-tmp.sh"); sh.env.set("JET_NIX_TMP_CLEANED", "1") } }
}
```

### Growth arc (still one file, blocks accrete)

```jet
// + a subpackage:
packages: { jet: executable { … }, jetpack: alias(…), lsp: library { deps: { jet: path@. } } }

// + a second shell:
module env.ci { packages: [ pkgs.[cargo, rustc, gcc, lld] ]  git_hooks_path: "scripts/githooks" }

// + a jetos host:
module system.build-box {
    target: linux.x64
    packages: [ pkgs.[git, ripgrep] ]
    services: { openssh: { enable: true, ports: [22] } }
    options:  [ network.hostName: build-box, filesystem.timeZone: "Europe/London" ]
}

// + an OCI image:
module image.server { from: packages.jet, expose: [8080], env_vars: ["RUST_LOG": "info"] }

// + a fleet:
module fleet.prod {                                            // (new — D-ECO15)
    hosts:   [ build-box: system.build-box ]
    rollout: Rollout.{ strategy: .Rolling, batch: 1, proof: .Vm }
}
```

### Framework answers

- **nixpkgs-lib override:** `R.with([…])` inline in the same file; no overlay
  file needed until you want a reviewed, reusable set.
- **Multi-shell:** `module env.<name>` blocks stacked in `project.jet`.
- **CLI:** `jet env`, `jet env ci`, `jet build jet`, `jet image server`,
  `jetpack add …` — all address blocks by their module/package name; one file to
  open.
- **Expert full-control file:** the whole port above — every knob visible in one
  scroll, `let` bindings shared across tiers.
- **Hybrid:** the single file *is* both surfaces; a beginner writes two lines, an
  expert writes the same file with more blocks.

### What this overturns

- `env.jet` / `workspace.jet` / `image.jet` as **reserved filenames** → optional
  (D-ECO1). Reserved *namespaces* stay.
- `find("./packages")` demoted: one file rarely discovers siblings.
- `jet init` writes `project.jet`, not `pkg.jet` (D-JPK-FILENAME2 wording).

### Kill-criteria check

- Hollow defaults? No — beginner file is two lines.
- Dictate file structure? **Yes, softly** — it strongly implies one file. This is
  the main tension with philosophy "flexible structure." Mitigated only if `find`
  imports are still allowed (making it Fold-lite).
- Invariant carve-out? No.

### Adversarial

1. **Growth cliff at scale.** This repo's real `project.jet` would be ~150 lines
   spanning six unrelated concerns; a monorepo with 20 packages is unreadable and
   a merge-conflict magnet. The single-file virtue inverts into a single-file
   bottleneck exactly when a team is largest.
2. **Violates structural flexibility (philosophy §"one mechanical path, flexible
   structure").** The doctrine says code layout is the user's choice; Deck makes
   layout the *framework's* choice. That is the one place the constitution
   explicitly forbids dictation.
3. **U7 boundary blurs.** "A file is a complete program" vs "a project is one
   file" are easy to conflate; beginners will add `project.jet` reflexively and
   lose the zero-ceremony path the philosophy protects.
4. **Diff/review granularity.** CODEOWNERS, per-tier review, and "who touched the
   image config" all collapse when everything is one file with one git blame.

---

## Framework 2 — Roles (reserved files, sharpened)

**Thesis:** each tier has a home. `pkg.jet` is identity + packages, `env.jet` is
the dev shell, `workspace.jet` is the monorepo index; images/systems/fleets are
role modules in any discovered `.jet`. This is the ratified status quo, told as
one crisp story. **Overturns nothing** — it is the baseline the others are
measured against.

### Beginner's first file

Same as Deck: `jet run script.jet` needs nothing (U7). The first manifest is
`pkg.jet` with one line: `packages: { hello: executable }`.

### The jet repo, fully ported (multiple files)

```jet
// pkg.jet — identity + buildable outputs
payload: { name: "jet", version: "1.0.0", edition: "2026", jet: 1.0 }
sources: { pkgs: github@NixOS/nixpkgs/nixos-unstable }
let runtime = [ pkgs.[rustc, gcc, lld], pkgs.ruby, pkgs.php, pkgs.R.with([cran.jsonlite]) ]
packages: {
    jet: executable {
        build: Recipe.cargo(lock: "Cargo.lock"), runtime: runtime,
        wrap: Wrap.{ path_prefix: runtime, env: ["TZDIR": pkgs.tzdata.zoneinfo] },
        meta: Meta.{ description: "Compiler for the Jet programming language",
                     homepage: "https://github.com/jet-language/jet",
                     main_program: "jet", platforms: .Unix },
    },
    jetpack: alias(packages.jet, bin: "jetpack"),
}
```
```jet
// env.jet — the dev shell (the full §Mechanism env.dev block)
module env.dev { /* packages, platform.linux, prompt, env_vars, library_path,
                    git_hooks_path, banner, tools, hook — verbatim from §Mechanism */ }
```
The image/system/fleet tiers live in their own discovered files
(`image.jet`, `system.jet`, `fleet.jet` — names are convention, role comes from
the module declaration).

### Growth arc (add a file per tier)

```jet
// + subpackage: new dir packages/lsp/pkg.jet, then workspace.jet indexes it:
module workspace {
    members: find("./packages")
    catalog: { http: "1.4.0" }                                   // D-JPK-CATALOG1
    overlay dev { package("R").withPackages += [cran.jsonlite] } // D-JPK-OVERLAY1
    policy:  { trust: { default: prompt, ci: { prompt: deny } } }// D-JPK-GRANTSCHEMA1
}
```
```jet
// + system.jet, image.jet, fleet.jet — one file each, same module bodies as Deck's arc.
module system.build-box { … }
module image.server { from: packages.jet, expose: [8080] }
module fleet.prod { hosts: [build-box: system.build-box], rollout: Rollout.{ … } } // D-ECO15
```

### Framework answers

- **nixpkgs-lib override:** the `overlay` in `workspace.jet` is the canonical
  home; `R.with(…)` inline still works in `env.jet` for one-offs.
- **Multi-shell:** `env.dev` in `env.jet`; `env.ci`/`env.docs` as extra `module
  env.<name>` blocks in `env.jet` or any discovered file.
- **CLI:** `jet env`, `jet build`, `jet image server`, `jet os switch build-box`
  — each verb reads its reserved file; `jet info <thing>` says which file owns it.
- **Expert full-control file:** every tier is its own reviewable file with its own
  git blame and CODEOWNERS entry.
- **Hybrid:** beginner opens `pkg.jet` (one line); expert navigates a small tree
  of role files — the same modules, sharded.

### What this overturns

Nothing. This is the ratified model (S52, U10, D-JPK-MODBODY1, D-WORKSPACE1).
The only *additions* are the shared §Mechanism spellings (D-ECO2–D-ECO12,15),
which every framework needs equally.

### Kill-criteria check

- Hollow defaults? No.
- Dictate file structure? **Partially** — reserved filenames mean `env.jet` must
  be named `env.jet`. Softened because non-reserved role modules (image/system/
  fleet) live in *any* discovered file, and `find` lets members go anywhere.
- Invariant carve-out? No.

### Adversarial

1. **Ceremony arrives early.** A three-package repo already wants `pkg.jet` +
   `env.jet` + `workspace.jet` — three files before you write logic. The
   beginner-magic promise (priority #2) frays: "where do I put X" is a real,
   repeated question that Deck never asks.
2. **Reserved-filename rigidity.** `env.jet` must be `env.jet`; a user who wants
   `dev-shell.jet` cannot have it, even though role comes from the module name
   everywhere else. That is an inconsistency — role-by-name for images, but
   role-by-filename for envs/workspaces (I8 friction).
3. **Discovery cost.** Answering "what does this project define" means reading a
   tree and running `find`, not opening one file — worse for onboarding, LSP
   jump-to, and code review of a small project.
4. **Two placement rules to learn.** Some tiers are reserved-file (pkg/env/
   workspace), some are discovered-by-name (image/system/fleet). Newcomers must
   learn which is which; Deck and Fold each have exactly one rule.

---

## Framework 3 — Fold (one file that splits on growth)

**Thesis:** the file model is the *user's* choice, backed by one mechanism.
Start with one `project.jet` (identical to Deck). When a tier earns its own file,
`jet split env` extracts `module env.dev` into `env.jet` — byte-preserving,
reversible with `jet fold env`. The compiler merges every `module <role>.<name>`
it discovers regardless of file (`find` at the root). Reserved filenames become a
recognized *convention* the tooling prefers, never a requirement. This is
philosophy §"one mechanical path, flexible structure" applied literally.

### Beginner's first file

Two lines in `project.jet` (or zero files for a lone script — U7). It never
splits until you ask.

### The jet repo, ported — two equivalent states

**Folded** (early): byte-identical to Deck's single `project.jet`.

**Split** (this repo today): run `jet split` once →

```
project.jet      # payload + sources + packages (== Roles pkg.jet)
env.jet          # module env.dev            (== Roles env.jet)
workspace.jet    # module workspace          (appears when members > 1)
image.jet        # module image.server       (appears when you add it)
```

The two states are the **same program**; `jet fold` / `jet split` move blocks
between files without changing a byte of any module body. `jet fmt` keeps them
canonical. Reserved filenames are where `jet split` *puts* things by default;
`find` means you may put them anywhere and it still resolves.

### Growth arc (mechanical, tool-driven)

```
jet split env        # env.dev leaves project.jet → env.jet
jet new package lsp  # scaffolds packages/lsp/, auto-adds to workspace members
jet split system     # system.build-box → system.jet
jet split image      # image.server → image.jet
jet split fleet      # fleet.prod → fleet.jet
```
Every split is reversible; a solo dev can keep everything in `project.jet`
forever, a large team can shard fully — same semantics either way.

### Framework answers

- **nixpkgs-lib override:** `R.with(…)` inline while folded; `jet split overlay`
  promotes it into `workspace.jet`'s reviewed `overlay` when the team wants it.
- **Multi-shell:** `module env.<name>` anywhere; folded together or split apart.
- **CLI:** identical verbs to Roles, plus `jet split`/`jet fold`; `jet info`
  reports the tier and *current* file, since file is mutable.
- **Expert full-control file:** the expert chooses — one giant file or a full
  tree — and the choice carries zero semantic weight.
- **Hybrid:** this *is* the hybrid pass — one mechanism (role modules + `find`),
  two ergonomic surfaces (folded/split) selected by a reversible command.

### What this overturns

- Reserved filenames become **preferred convention, not requirement** (amends
  S52/U18 "reserved" wording → D-ECO1/D-ECO13).
- Adds `jet split` / `jet fold` commands and whole-tree role discovery at the
  root (extends `find`) — D-ECO13.
- `env.jet` is no longer the *only* legal home for `module env.*`.

### Kill-criteria check

- Hollow defaults? No — two-line start, magic scaffolding.
- Dictate file structure? **No — the opposite.** It is the only framework that
  makes layout the user's choice, satisfying the philosophy directly.
- Invariant carve-out? No. Strengthens I8 (one mechanism, many arrangements).

### Adversarial

1. **`jet split`/`fold` is real tooling surface.** Byte-preserving extraction
   across files, merge-back, `let`-binding scoping when a shared `let runtime`
   spans tiers being split — that is a formatter-grade feature with its own bug
   class. It must round-trip perfectly (formatter-roundtrip law) or it silently
   corrupts manifests.
2. **`let` sharing breaks on split.** Deck's `let runtime = …` is shared by
   `packages` and `env.dev`. Split them into two files and the binding must be
   duplicated, hoisted to a shared file, or resolved cross-file — none free,
   each a new question Deck/Roles never face.
3. **"Where is X" gets *harder*, not easier.** Because a module may live in any
   file, answering "which file defines env.dev" now requires a tool (`jet info`),
   whereas Roles guarantees it by name. Flexibility trades away a locational
   guarantee.
4. **Two mental models coexist.** New users see some repos folded, some split,
   and must learn both plus the command that converts them — arguably *more* to
   learn than either pure model, cutting against priority #2.
5. **Convention drift.** If filenames are only preferred, real repos will scatter
   `module env.*` into oddly named files; the ecosystem loses the "open `env.jet`"
   muscle memory that makes Roles skimmable.

---

## Comparison

| Capability | Deck | Roles | Fold |
|---|---|---|---|
| Pins / sources | `sources:` one file | `sources:` per role file | `sources:` folded or split |
| Packages | `packages:` block | `pkg.jet` | either |
| Named shells | stacked blocks | `env.jet` + blocks | anywhere |
| Hooks (typed + escape) | same fields | same fields | same fields |
| nixpkgs override | `.with` inline | `overlay` in workspace | `.with` → split to overlay |
| Subpackages | blocks in one file | dirs + `workspace.jet` | `jet new` + auto-index |
| Catalog | block | `workspace.jet` | `workspace.jet` |
| System (jetos) | block | `system.jet` | any file / split |
| Image (OCI) | block | `image.jet` | any file / split |
| Fleet | block | `fleet.jet` | any file / split |
| Single-file U7 | preserved | preserved | preserved |
| Teach in one page | **easiest** | hardest | medium (+`split`) |
| Structural flexibility (philosophy) | **worst** | partial | **best** |
| File dictates? | one file | reserved names | **no** |
| Overturns ratified law? | filenames | none | filename "reserved" |

---

## Ballot rows

Owner picks D-ECO1 first; it selects the file model. D-ECO2–D-ECO16 are the
shared spellings needed for flake parity under *any* framework — most are
framework-independent, answered once.

| ID | Decision | Deck | Roles | Fold |
|---|---|---|---|---|
| **D-ECO1** | **File model archetype** | one `project.jet` | reserved role files (status quo) | folded→split, files are convention |
| D-ECO2 | `build: Recipe.cargo(lock:)` as a first-class package build field | yes | yes | yes |
| D-ECO3 | `wrap: Wrap.{ path_prefix, env }` (replaces `wrapProgram`) | yes | yes | yes |
| D-ECO4 | `alias(package, bin:)` (replaces `apps.*` second binary) | yes | yes | yes |
| D-ECO5 | `meta: Meta.{ description, homepage, main_program, platforms }` | yes | yes | yes |
| D-ECO6 | Typed env hook fields: `env_vars`, `path_prepend`, `library_path`, `git_hooks_path`, `banner` | yes | yes | yes |
| D-ECO7 | Expert trust-gated `hook: fn(sh: Shell)` running Jet code | yes | yes | yes |
| D-ECO8 | Package override combinator `R.with([…])` + `overlay` backing | yes | overlay-first | yes |
| D-ECO9 | Dev tool wrappers `tools: { … Tool.wrap(bin:, path_prefix:) }` | yes | yes | yes |
| D-ECO10 | Platform sugar `platform.linux:` desugaring to comptime `if target.os` | yes | yes | yes |
| D-ECO11 | `runtime:` field (runtime PATH deps distinct from build deps) | yes | yes | yes |
| D-ECO12 | `formatter:` passthrough for non-`.jet` formatters (`nix fmt` equivalent) | yes | yes | yes |
| D-ECO13 | `jet split` / `jet fold` commands + root-level role discovery | n/a | n/a | **required** |
| D-ECO14 | Reaffirm U7: a lone script never needs a project file, in every model | reaffirm | reaffirm | reaffirm |
| D-ECO15 | `module fleet.<name>` spelling (`hosts:`, `rollout:`) — un-freezes fleet surface (D-JETOS-FREEZE1) | yes | yes | yes |
| D-ECO16 | `module system.<name>` at equal depth now — un-freezes system surface (D-JETOS-FREEZE1) | yes | yes | yes |

D-ECO15/16 flag that giving fleet and system "equal depth" (owner scope answer
#4) re-opens surface frozen by D-JETOS-FREEZE1 for Epoch 7 — a real ballot, not
a silent inclusion.
