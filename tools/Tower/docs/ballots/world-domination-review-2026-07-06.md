# Jet world-domination review

Status: proposal artifact, not ratified. No Tower cards were created. Use this
as the approval menu for converting ideas into cards or owner ballots.

Goal: make Jet the language people reach for when they want Python's reach,
Rust's safety/performance, Go's deployment, TypeScript's app ergonomics, Nix's
reproducibility, Zig's control, Swift/Kotlin's app polish, Julia/R's data
fluency, and still want one coherent toolchain.

## Research signals

- Stack Overflow 2025: Python grew sharply and is framed as the go-to language
  for AI, data, and backend. Rust remains the most admired language at 72.4%.
  Docker, npm, pip, Cargo, pnpm, Bun, Terraform, and Kubernetes are all in the
  same developer workflow bucket. Jet should not think in one ecosystem silo.
- Cargo's lockfile story is loved because the manifest stays broad and the
  lock records exact versions. Cargo's resolver also explains duplicate-version
  hazards. Jet should keep this, but make the lock inspectable and merge-safe.
- pnpm is admired for disk efficiency, strict dependency visibility,
  workspaces, and catalogs. Jetpack already has the hangar and monorepo law;
  it should add workspace catalogs and strict package visibility as product
  UX, not hidden policy.
- npm has huge reach, audit/provenance machinery, and supply-chain pain. Jet
  should import npm reach but never inherit install-script trust by default.
- Deno's permission model is a clear beginner story: run code with no ambient
  access, grant scoped powers. Jet already has effects; Jetpack should make
  those effects visible at package, build, env, and OS activation boundaries.
- Python wins by being useful everywhere, but its packaging guide still has to
  teach shell vs REPL, pip bootstrapping, virtualenv activation, system Python
  conflicts, and PATH fixes. Jet's default must delete that whole class.
- Nix wins on immutable stores, complete dependency declarations, atomic
  upgrades, rollbacks, binary caches, and declarative OS config. Nix loses
  mindshare on approachability, errors, docs drift, and GUI absence. jetos
  should keep the math, replace the human interface.

Sources used: Stack Overflow 2025 Developer Survey; Cargo Book; pnpm docs;
npm docs; Deno docs; Python Packaging User Guide; Nix "How Nix Works" and Nix
manual.

## Current Jet/Jetpack reading

Strong existing assets:

- Single-file `jet run` stays ceremony-free forever.
- Front end owns semantics and diagnostics. rustc never talks to users.
- One canonical spelling per semantic operation, with structural flexibility.
- Core language already has ownership, effects, contracts, typestate,
  protocols, migrations, taskgroups, reactive UI groundwork, web/native UI,
  C/Rust interop, package manifests, and a content-addressed package store.
- Jetpack canon already targets the right package-manager endgame: separate
  engine binary, `pkg.jet`, role modules, hangar, signed output cache envelope,
  no daemon/root, offline guarantee, adapters, toolchain pin, discovery, build
  logs, services, secrets, images, fleets, and jetos.

Hardening gaps:

- Epoch-4 docs and syntax law still have drift around reserved role filenames,
  `jetpack os` vs `jetos`, and older `jet switch` examples. Reconcile before
  adding more surface.
- Current implementation baseline admits code does not yet match canon. Phase A
  reconciliation remains the first Jetpack move.
- Jetpack's near-term dogfood bar should stay concrete: replace this repo's
  `nix develop` first, including wrappers, `JET_ROOT`, toolchains, services,
  cache, offline behavior, and failure UX.
- jetos syntax is research/frozen until post-Epoch-3. Product design can
  advance now; surface ratification should wait.

## Product doctrine

World takeover succeeds only if Jet is boring at the bottom and magical at the
top.

- Beginner: a script runs, a web app opens, a package installs, a laptop config
  toggles, and errors say exactly what to do. No manifest until needed. No
  permissions until a program asks for power. No package-manager vocabulary
  before the first win.
- Expert: every artifact has a graph, hash, provenance, toolchain, permission
  set, cache key, scheduler trace, generated source, and rollback point.
  Nothing is hidden; it is just behind explicit commands and grants.
- Hybrid: one semantic graph covers code, build, package, env, image, machine,
  and fleet. Different entrypoints are views over the same graph.

## Top approval candidates

### D-WD1: capability grants as the universal trust surface

Gist: choose whether Jet effects become the visible permission model for code,
builds, packages, envs, services, and jetos activation.

Story: Mara runs a new repo. It wants network, two secrets, a post-fetch build
recipe, and a local Postgres service. She should see one human trust screen,
not five unrelated prompts.

In wild:

```jet
module env.dev {
    packages: [default.postgres, npm.vite]
    services: { postgres: { enable: true, port: 5432 } }
    secrets: { STRIPE_KEY: secret("stripe-dev") }
}
```

Options:

- A: Effects are code-only. Simple, but package/env/OS trust becomes separate
  policy and users learn multiple models.
- B: Effects plus Jetpack grants. Recommended. One grant vocabulary spans
  `#(Net, Secret)`, build recipes, install hooks, services, and activation.
- C: Deno-style CLI flags only. Familiar for scripts, weak for packages and OS.

Comparison: Deno permissions are praised for clarity; npm install scripts are
widely distrusted; Nix sandboxing is strong but not beginner-visible.

Rec: B. One trust model is the clean conquest weapon.

Group: ecosystem.

### D-WD2: semantic dossier as first-class tooling

Gist: choose whether every Jet artifact can explain itself as a dossier.

Story: Sol gets a PR that changes a backend service. They need to know public
API drift, effects, dependency graph, generated code, perf budget, migrations,
and OS/package impact without spelunking.

In wild:

```text
jet dossier src/server.jet
jet dossier package textkit
jet dossier system.laptop --diff current
```

Options:

- A: Keep separate commands (`graph`, `audit`, `expand`, `semindex`,
  `explain-build`). Good pieces, scattered story.
- B: Add `jet dossier` as an umbrella view over existing facts. Recommended.
  No new semantics; one lens product.
- C: IDE-only dossier. Pretty, but CLI/CI loses parity.

Comparison: Cargo has `tree`; pnpm has workspace views; Nix has derivation
graphs; Jet can unify graph, diagnostics, effects, generated code, and policy.

Rec: B.

Group: tooling.

### D-WD3: package graph strictness and catalogs

Gist: choose how Jetpack prevents hidden dependencies and workspace drift.

Story: Priya maintains a monorepo with 40 packages. A test passes because a
tool is accidentally available from the root env. A release later fails.

In wild:

```jet
module workspace {
    catalog: {
        http: "2.4.1"
        ui: "1.8.0"
    }
    members: find("./packages")
}
```

Options:

- A: Shared env packages are visible to all workspace members. Convenient,
  recreates npm/Python ambient-dep bugs.
- B: Strict member visibility plus workspace catalogs. Recommended. Packages
  see only declared deps; catalogs centralize versions.
- C: Per-member lockfiles only. Smaller files, worse monorepo coordination.

Comparison: pnpm's strictness and catalogs reduce duplicate versions and
workspace drift. Cargo workspaces unify resolution but feature unification has
surprises. Jet should be strict by default and explain every duplicate.

Rec: B.

Group: packages.

### D-WD4: lockfile explain and merge strategy

Gist: choose whether `.jet/lock` becomes a user-explainable artifact.

Story: Amara rebases a dependency update and gets a lock conflict. She should
not manually edit hashes or guess whether the conflict is safe.

In wild:

```text
jet lock explain textkit
jet lock merge --ours-feature-policy --theirs-version-bump
jet lock why-dupe regex
```

Options:

- A: Treat lock as generated, rerun resolver. Fast, but hides intent and can
  make review noisy.
- B: Explainable lock with semantic merge. Recommended. Every lock entry has
  source, reason, owner package, policy, hash, and update command.
- C: Human-authored lock. Breaks reproducibility discipline.

Comparison: lockfile research shows developers struggle to maintain and
interpret locks. Cargo and npm commit exact trees; Jet can add rationale and
safe merge.

Rec: B.

Group: packages.

### D-WD5: native migration bridges from other ecosystems

Gist: choose whether migration importers are first-class product surfaces.

Story: Lee has a TypeScript app, Python notebooks, a Rust CLI, a Nix flake, and
a Dockerfile. Jet adoption should be "consume, coexist, replace" in one tool.

In wild:

```text
jet import package.json
jet import pyproject.toml
jet import Cargo.toml
jet import flake.nix
jet import Dockerfile
```

Options:

- A: Only document manual migration. Too slow for adoption.
- B: Importers create role modules, adapters, FFI stubs, and TODO diagnostics.
  Recommended. Output is Jet source users can edit.
- C: Runtime compatibility shims only. Easy to start, hard to finish native
  migration.

Comparison: TypeScript won by coexistence with JS. Jet should go broader:
  npm/PyPI/Cargo/SwiftPM/Nix/Docker become on-ramps, not forever crutches.

Rec: B.

Group: interop.

### D-WD6: Jetpack provider federation

Gist: choose how much foreign package ecosystem Jetpack should ingest.

Story: Noor wants `numpy`, `plotly`, `ripgrep`, `raylib`, and a GitHub binary
tool in one reproducible env.

In wild:

```jet
deps: {
    numpy: py@"==2.1"
    plotly: js@"plotly.js@2"
    rg: nixpkgs@ripgrep
    weirdctl: Pkg.adapt(name: "weirdctl", source: github@acme/weirdctl#abc,
        recipe: Recipe.prebuilt(bin: "weirdctl"))
}
```

Options:

- A: Jet registry first; foreign ecosystems only via manual adapters.
- B: Federated providers with strict sandbox, provenance, and replacement
  overlays. Recommended.
- C: Shell out to native package managers. Broad, but loses lock/provenance.

Comparison: npm/PyPI win reach; Nix wins reproducibility; Cargo wins integrated
build; Jetpack should ingest reach through one locked graph.

Rec: B.

Group: packages.

### D-WD7: jetos Studio GUI

Gist: choose whether declarative OS config gets a first-party GUI that writes
Jet role modules.

Story: Eli wants rollback-safe declarative Linux but does not want to learn a
configuration language before enabling Steam, Flatpak, fonts, backups, and SSH.

In wild:

```text
jetos studio
```

Options:

- A: CLI/text only, like NixOS. Powerful but narrows audience.
- B: GUI writes normal Jet modules with round-trip comments and diff preview.
  Recommended. The GUI is an editor over the same typed graph.
- C: GUI stores separate JSON state. Easier to model visually, violates one
  source of truth.

Comparison: NixOS has unmatched rollback power but lacks a mainstream friendly
control panel. Homebrew/Windows/macOS win beginner UX but lose declarative
truth. Jetos can have both.

Rec: B.

Group: jetos.

### D-WD8: OS dry-run, VM proof, and power-cut simulation

Gist: choose whether jetos changes must be testable before activation.

Story: Ren updates GPU drivers. Before touching the boot default, they want a
VM boot, service health checks, and a rollback proof.

In wild:

```text
jetos switch laptop --plan
jetos test laptop --vm
jetos switch laptop --name pre-gpu
```

Options:

- A: Build/switch/rollback only. Nix-like baseline.
- B: Plan plus VM proof plus rollback proof. Recommended.
- C: GUI-only preview. Friendly, but CI cannot enforce it.

Comparison: NixOS `test`, `build-vm`, and rollbacks are loved. Jetos should
make them the default path with clearer output and CI artifacts.

Rec: B.

Group: jetos.

### D-WD9: typed data stack as a first-party domain

Gist: choose whether data analysis gets Core-level treatment.

Story: June reads CSV, joins telemetry, runs stats, trains a model through
Python interop, and ships a native dashboard. Python should not be mandatory.

In wild:

```jet
use core.data
use core.stats
use py.numpy as np

fn run() -> Unit ? {
    rows :: data.csv<Row>("events.csv")?
    daily :: rows.group_by(.day).summarize(count: count(), p95: p95(.latency))
    print(daily)
}
```

Options:

- A: Leave data to Python interop. Good bridge, weak native story.
- B: First-party `core.data` table/series/stats/plot stack, with Python/R
  interop as accelerators. Recommended.
- C: SQL-only data story. Strong for DBs, weak for files/notebooks.

Comparison: Python wins data through batteries and libraries; Julia wins
syntax/perf for numeric work. Jet can make typed data native and still call
Python during transition.

Rec: B.

Group: core.

### D-WD10: game-dev standard lane

Gist: choose whether game dev gets a blessed engine substrate.

Story: Imani builds a 2D game, then an editor, then a 3D prototype. She needs
assets, hot reload, ECS, fixed-step simulation, input, audio, GPU, and
deterministic replay.

In wild:

```jet
use core.game

fn tick(world: &World, dt: Seconds) {
    world.query(Position, Velocity).each((e) => {
        e.Position += e.Velocity * dt
    })
}
```

Options:

- A: Only wrap raylib. Good start, not world-class.
- B: First-party game lane: assets, ECS, deterministic scheduler, replay,
  editor hooks, shader DSL gate, raylib/wgpu backends. Recommended.
- C: Third-party engines only. Faster ecosystem, weaker Jet identity.

Comparison: Unity/Godot win workflow; Rust game dev has power but fragmented
ergonomics; Jai/Odin/Zig attract game devs through control. Jet should offer
control plus batteries.

Rec: B.

Group: core.

### D-WD11: embedded and freestanding profiles

Gist: choose how Jet scales down to microcontrollers and kernels.

Story: Tessa writes firmware. She wants no allocator, no OS, fixed memory,
volatile/MMIO gates, linker script control, and the same diagnostics.

In wild:

```jet
#Target(Embedded.CortexM4)
module system.board {
    memory: { flash: 1024KiB, ram: 256KiB }
}
```

Options:

- A: Rust-backend freestanding flags only. Minimal surface, weak product.
- B: Typed target profiles plus explicit low-level gates and memory budgets.
  Recommended.
- C: Separate Jet dialect. Violates one language.

Comparison: Zig shines at target/control; Rust embedded has safety but many
crate/tooling concepts; C wins ubiquity while losing safety. Jet should keep
one language and make target constraints typed.

Rec: B.

Group: low-level.

### D-WD12: proof and replay mode

Gist: choose whether correctness facts are a first-class development mode.

Story: Omar ships payment code and a distributed job runner. He wants contracts,
budget checks, deterministic replay, and a proof report before deploy.

In wild:

```text
jet prove src/payments.jet
jet replay --from trace.jtr --until panic
```

Options:

- A: Runtime tests and contracts only.
- B: `jet prove` over contracts/refinements/effects/budgets plus replay
  artifacts. Recommended.
- C: Full formal verification first. Powerful, but less usable as default UX.

Comparison: Rust gives compile-time memory proof; Dafny/SPARK give stronger
proof with heavier ceremony; property testing gives practical confidence. Jet
should make proof progressive.

Rec: B.

Group: static-guarantees.

### D-WD13: AI-assisted codebase workbench, local and auditable

Gist: choose whether Jet tooling includes a first-party codebase assistant over
the semantic index.

Story: Dana asks "what breaks if I change this type?" The answer should cite
real symbols, tests, effects, and generated code, not hallucinate.

In wild:

```text
jet ask "can UserId and AccountId mix anywhere?"
jet ask --fix "replace this Python adapter with native Jet"
```

Options:

- A: No AI surface. Safe, but leaves best DX on the table.
- B: Local-first assistant over semindex/dossier/test facts, with patch
  provenance. Recommended.
- C: Cloud-only assistant. Powerful, weaker trust story.

Comparison: Developers increasingly use AI, but raw chat lacks project truth.
Jet can make AI a compiler-backed tool, not a guessing box.

Rec: B.

Group: tooling.

### D-WD14: performance budget profiles

Gist: choose whether performance expectations are declared and checked.

Story: Mei writes an HTTP server and wants latency, allocation, binary size,
and startup budgets enforced in CI.

In wild:

```jet
module perf.server {
    budgets: {
        startup: 20ms
        alloc_per_request: 0
        p99_latency: 5ms
        binary_size: 3MiB
    }
}
```

Options:

- A: Benchmarks only. Flexible, easy to ignore.
- B: Typed budgets tied to `jet bench`, `jet build`, `jet dev`, and CI.
  Recommended.
- C: Compiler auto-optimizes without user budgets. Nice story, weak contract.

Comparison: Go wins simple deploy/perf predictability; Rust wins peak
performance but compile/perf tuning can sprawl. Jet should make performance
visible and enforceable.

Rec: B.

Group: tooling.

### D-WD15: native package replacement overlays

Gist: choose how Jet rewrites foreign dependencies into native Jet over time.

Story: A team starts with `js.plotly` and `py.numpy`, then Jet-native
`core.plot` and `core.linalg` mature. They need call sites to survive.

In wild:

```jet
deps: {
    plotly: js@"plotly.js@2"
    plotly.native: core.plot replaces js.plotly
}
```

Options:

- A: Manual migration, edit imports/calls.
- B: Replacement overlays: a Jet package can declare it exports the same
  surface as a foreign package; `jet migrate` proves compatibility and rewrites
  deps. Recommended.
- C: Permanent foreign interop. Useful bridge, not native conquest.

Comparison: TypeScript preserved JS call sites; Jet should preserve package
surfaces while replacing implementations.

Rec: B.

Group: interop.

## Wider feature backlog

Language/Core:

- `core.asset`: typed assets with hashes, hot reload, packing, compression,
  license/provenance, and target-specific transforms.
- `core.query`: typed query values over SQL, in-memory tables, files, and
  streams, reusing whitelisted `#Sql<Row>` blocks where needed.
- `core.ui` completion push: a11y by default, keyboard navigation, typed theme
  tokens, native hot reload, component ownership via `jetpack add`.
- `core.net` product hardening: structured concurrency, backpressure,
  protocol blocks, TLS profiles, load-test budgets, service templates.
- `core.cloud`: typed deploy targets as role modules, but with provider
  lock/provenance and no cloud-specific language syntax.
- `core.robotics`: real-time-ish loops, sensor streams, kinematics, safety
  budgets, simulator hooks.
- `core.ml`: tensor/data bridge, ONNX import, Python accelerator bridge, typed
  model artifacts, reproducible training envs.
- `core.docs`: doctests, literate examples, API snippets from real tests.
- `core.compat`: migration helpers for JS/Python/Rust/C/Swift idioms with
  teaching diagnostics once D-S14 teaching pause lifts.

Jetpack:

- First dogfood target: replace this repo's `nix develop`.
- `jet import` family for package.json, pyproject, Cargo.toml, flake.nix,
  Dockerfile, Compose, systemd, Homebrew Brewfile.
- `jetpack search` trust score: maintenance, license, platform, signatures,
  CVEs, last release, transitive effects, install hooks, binary cache status.
- Offline-first onboarding: `jetpack prefetch --bundle` creates a shareable
  cache capsule for classrooms, planes, conferences, and air-gapped teams.
- Branch lockfiles or lock intents: reduce merge pain without multiplying
  truth files.
- Package health gates before publish: docs, examples, API diff, effects,
  supply chain, benchmark regression, semver proof.
- Build crime-scene UX: log, preserved scratch, env diff, command replay,
  exact artifact provenance.
- Store/hangar GUI: disk usage, GC roots, generations, packages, source graph,
  cache hits, and "why is this still here?"

jetos:

- GUI first-class, text source canonical.
- One-character module disable stays; GUI should expose it as a reversible
  toggle and preserve source layout.
- Option search with final-value preview and conflict explanation.
- Migration wizard from NixOS/Home Manager/flake repos.
- Hardware scan creates a proposed role module with every uncertain choice
  marked as a question, not guessed.
- VM test before switch, power-cut sim, rollback proof, boot-entry preview.
- Generation naming, notes, diff, pin, export, and restore.
- App store view over declarative packages: install toggles edit Jet modules.
- Fleet rollout UI: canary, health gate, automatic rollback, per-host diff.

## Persona pass

Beginner challenge:

- Does a single file still run with no project? Yes, protect R9.
- Can a GUI user manage a declarative OS without writing code? Only if jetos
  Studio lands and source remains canonical.
- Can errors teach without jargon? Every proposed surface must have what/why/fix
  and avoid raw foreign tool errors.
- Can packages be used without learning trust theory? Yes if the grant screen is
  concrete: "this repo wants network to npm, build command, STRIPE_KEY, and a
  Postgres service."

Expert challenge:

- Can I inspect every generated byte? Dossier and provenance must include
  generated source, cache keys, toolchains, scheduler, and unsafe/effect gates.
- Can I pin toolchains, targets, allocators, schedulers, caches, and policies?
  Existing canon says yes; ballots above make it visible.
- Can I go low-level without fighting the language? Embedded profiles and
  expert gates need first-class docs and examples, not hidden footnotes.
- Can I replace foreign packages with native Jet without rewrites? Replacement
  overlays are the missing bridge.

Adversarial pass:

- Feature sprawl risk: every idea above must attach to the same semantic graph.
  Reject standalone DSLs, separate config files, and second package concepts.
- Trust theater risk: audits and signatures are useless if install scripts run
  ambient. Default-deny build/install effects.
- GUI split-brain risk: jetos Studio must edit Jet modules, never own separate
  state.
- Lockfile opacity risk: exact locks without explanations become magic. Add
  `jet lock explain`.
- Interop forever risk: foreign providers must be framed as migration and reach,
  not an excuse to leave Core thin.
- Nix clone risk: keeping store/rollback math is good; copying error style,
  docs drift, and code-only OS UX is failure.
- Python clone risk: data/AI reach matters; virtualenv/PATH/system-package
  friction must not leak into Jet.
- npm clone risk: reach matters; ambient install scripts and dependency
  confusion must not leak into Jet.
- Rust clone risk: safety/performance matter; compile-time pain, lifetime
  exposure, and scattered async model must not leak into Jet.

## Suggested card conversion order

1. Phase-A Jetpack reconciliation and doc drift cleanup.
2. Dogfood card: replace this repo's `nix develop` with Jetpack.
3. D-WD1 universal grants.
4. D-WD3 package strictness/catalogs and D-WD4 lock explain/merge.
5. D-WD5 migration importers and D-WD6 provider federation.
6. D-WD7 jetos Studio product design, without ratifying jetos syntax yet.
7. D-WD9 data stack, D-WD10 game lane, D-WD11 embedded profiles.
8. D-WD2 dossier and D-WD13 AI workbench as one tooling push.

