# Jet ecosystem shape

**Status:** vocabulary and shape ratified 2026-07-15. Examples describe the intended final product, not shipped behavior. A source spelling marked `NEW: D-*` is ratified but not yet implemented; implementation is tracked on Tower cards. A marker in a code-block caption or first line governs every new spelling in that block. Unmarked Jet syntax and command names are already ratified.

## Glossary

| Term | Meaning |
|---|---|
| Package | The complete checked meaning of one endeavor: dependencies, environments, checks, images, systems, fleets, policy, and outputs. One package owns the directory tree rooted at its nearest `package.jet`. A monorepo root lists member packages by reference; members cannot have members. |
| Config | One typed slice of fields that merges into a package. It holds no code or package identity but may contribute dependencies, outputs, services, and other Package facts. |
| Output | A named, typed result users can build, run, enter, publish, activate, or deploy. Outputs are thin views over the package graph. |
| Graph | The resolved facts and relationships shared by the compiler, package manager, environment manager, JetOS, editors, and audit tools. |
| Lock | `.jet/lock`, the source-adjacent index of exact graph identity, selection reasons, policy, platform facts, and complete merge provenance. |
| Receipt | An immutable connected record from locked inputs through an action and output digest to verification, activation, and parent generation. |
| Hangar | The content-addressed store for downloaded bytes, build outputs, toolchains, closures, receipts, and generation artifacts. |
| Generation | An immutable, named profile or system closure with a parent, proof, activation record, and rollback point. |
| Plan | A checked prediction made before mutation. It says what Jet intends to fetch, build, replace, start, stop, activate, or remove. |
| Proof | Recorded evidence about what Jet built and observed, bound to exact inputs, outputs, baseline, readiness checks, and provenance. |
| Provider | A source of package metadata and bytes. Provider facts enter one graph and never create a second package model. |
| Adapter | A reviewed recipe that turns fetched bytes without canonical Jet metadata into a Package. It is not a provider. |
| Profile | A package set and user configuration activated as an atomic generation. `profile.<name>` and `user.<name>` use the same engine. |
| System | An Output that closes over packages, users, services, files, options, boot facts, and proofs for one machine. |
| Fleet | An Output containing named hosts, their System outputs, deployment targets, and one staged rollout policy. |

## The one idea

One typed value spans the whole endeavor. Everything users make is an Output of that value. Files organize source; declarations carry meaning. Growth moves the same values into more files without changing their identity or wiring.

The same graph answers every rung:

```text
hello.jet
    -> Package
        -> Package outputs
        -> Environment outputs
        -> System outputs
        -> Fleet outputs

one graph -> one lock -> one Hangar -> one plan/proof model -> one explain vocabulary
```

A beginner can stop at `jet run hello.jet`. An expert can inspect target triples, effects, toolchains, cache candidates, scheduler choices, remote builders, merge provenance, activation deltas, and every authority grant. Those are two views of one mechanism, not separate modes of authorship.

## Ratified foundation

This proposal completes, and does not reopen, these decisions:

| Ratified law | Consequence here |
|---|---|
| D-ECO-DECL1=A | Packages, environments, checks, services, images, systems, and fleets are ordinary named typed values under one root. |
| D-SHAPE5a/5b=A | Roles use ordinary `T.{}` or `.{}` construction; Output is a closed sum with checked payloads. |
| D-ECO-EXTENSION1 / D-ECO-COMPOSE2=A | Extensions are typed functions returning closed graph values. Safe additions merge by field law; scalar disagreements stop with full provenance. |
| D-ECO-ENV1=A | Environment is an Output, not a parallel setup language. |
| D-ECO-RECEIPT2=A | One record connects input, action, output digest, activation proof, and parent generation. |
| D-JPK-OSVERB1=A | JetOS uses `jet os check|init|plan|proof|build|switch|rollback|generations|lift|import|image`. |
| D-JPK-PROFILE1=D | Package profiles and user profiles share one generation engine. |
| D-SHAPE-MERGEPROVENANCE1=A | `.jet/lock` remains the sole primary merge-history authority. |
| D-JPK-DISPATCH1=B | Users type `jet`; `jetpack` and `jetos` remain versioned engine processes behind it. |
| U7 / R9 | A single `.jet` file remains a complete program without package state. |
| U28 / U29 | Per-user operation needs no root or resident daemon; a satisfied lock realizes offline. |

## Final shape

### Package and Config

`Package` is independent of checkout boundaries but never recursive. Each directory tree has one Package, defined by its nearest `package.jet`. A monorepo root may list member packages by reference; a member cannot list members, so package depth is capped at one. A repository may contain several unrelated packages, and one package may import signed Configs from several repositories. A source file may hold one or many Configs. File order and path never decide meaning.

The reserved filename supplies the `Package` type: top-level fields construct it, with no wrapper value and no repeated noun.

```jet
configs: [app, operations]                     // NEW: D-ECO-SLICENAME1

app :: Config.{                        // NEW: D-ECO-SLICENAME1
    source: "Source/api"
    deps: .{ http: "4.2" }
    outputs: .{ api: .Service.{ name: "api", entry: run_api } }
}

operations :: Config.{                 // NEW: D-ECO-SLICENAME1
    services: .{
        api: .{ enable: true, from: api, ports: [8080] }
    }
}
```

Configs contribute Package fields such as `deps:`, `outputs:`, and `services:`. They never declare member packages. Only the monorepo root's own `package.jet` may contribute its `members:` field.

Composition is deterministic:

- equal scalar facts coalesce;
- unequal scalar facts conflict;
- named collections combine by key;
- sets union;
- ordered values use the order law declared by their type;
- ecosystem scalar disagreements stop with provenance; experts use ordinary functions to construct one final value (D-ECO-COMPOSE2); `OptionValue.{ value, priority }` exists only inside option contributions (D-JOS-PRIORITY-SURFACE2);
- every successful contributor remains in `.jet/lock`.

```text
$ jet explain services.api.ports
services.api.ports = [8080]
  app.jet:18        default  [8080]
  operations.jet:7 ordinary  [8080]
Both contributions agree. No value was discarded.
```

### One reserved file

`package.jet` is the only reserved ecosystem source filename (`NEW: D-ECO-FILEROOT1`). It replaces `pkg.jet`, `env.jet`, `workspace.jet`, and JetOS `config.jet`. A script needs no reserved file. At first, all Package facts may remain inline. Any Config may later move to any discovered `.jet` file.

Discovery starts at the nearest `package.jet`, follows explicit imports and root-listed member references, and discovers `.jet` Configs under declared roots. A leading `_` disables a discovered file without changing its contents (`NEW: D-ECO-FILEROOT1`). Generated state remains under `.jet/`.

For one epoch, old role files are read together and produce one proposed teaching diagnostic. `L1320` is minted only if D-ECO-FILEROOT1 is ratified:

```text
$ jet check
Warning [L1320]: this Package uses four retired ecosystem filenames
 What: `pkg.jet`, `env.jet`, `workspace.jet`, and `config.jet` describe one Package
 Why: `package.jet` is now the one reserved ecosystem source file
 Fix: run `jet init --check`, review the fold, then run `jet init`
```

`jet init` folds old files or lifts a script. The fold records old paths and spans, so `jet init --restore-role-files` can restore byte-identical originals during the migration epoch. The final model has no permanent compatibility branch.

### Outputs

Outputs are thin projections over facts stored once on the graph. The proposed closed v1 kind set is `Library`, `Executable`, `Service`, `Check`, `Environment`, `Image`, `Bundle`, `System`, and `Fleet` (`NEW: D-ECO-OUTPUT-KINDS1`). Adding another kind requires a decision; arbitrary text kinds are rejected.

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
cli: Output :: .Executable.{ name: "todo", entry: run }
lib: Output :: .Library.{ name: "todo_core", modules: [Todo] }
api: Output :: .Service.{ name: "todo_api", entry: serve }
release: Output :: .Check.{ name: "release", entry: verify_release }       // NEW: D-ECO-OUTPUT-KINDS1
dev: Output :: .Environment.{ name: "dev", tools: [ripgrep] }              // NEW: D-ECO-OUTPUT-KINDS1
image: Output :: .Image.{ name: "todo", from: cli, kind: .Oci }
all: Output :: .Bundle.{ name: "todo-release", members: [cli, image] }
host: Output :: .System.{ name: "halcyon", packages: [cli] }               // NEW: D-ECO-OUTPUT-KINDS1
prod: Output :: .Fleet.{ name: "prod", hosts: .{ halcyon: host } }         // NEW: D-ECO-OUTPUT-KINDS1
```

The payload holds only name and kind-specific facts. Sources, dependencies, actions, effects, policy, target facts, and provenance live once. `jet inspect output` (`NEW: D-ECO-OUTPUT-PAYLOAD1`) reconstructs the complete path:

```text
$ jet inspect output todo.cli                  # NEW: D-ECO-OUTPUT-PAYLOAD1
todo.cli: Executable "todo"
entry       Source/main.jet::run
sources     source-set app (14 files, sha256:21c8…)
deps        textkit#1.4.2 -> unicode-data#15.1.0
action      compile.todo [action sha256:8d2a…]
output      sha256:746a…
toolchain   jet#3.2.0, x86_64-linux-gnu -> aarch64-linux-gnu
effects     read(source-set app), write(output todo.cli)
policy      release
provenance  complete
```

Runnable outputs link to ordinary functions by checked reference. Renames, visibility, editor navigation, provenance, and role validation use the normal symbol model. A lone `fn run` needs no Output declaration. Explicit `entry:` appears when more than one runnable Output exists.

Commands accept typed parameters, which become checked CLI flags. Services and checks take no parameters; their settings live in checked graph values so laptops, CI, images, and JetOS cannot invoke different configurations. Every callable returns `Void` or `Void ?`. A service lives while its function call lives. A check passes on normal return and fails on error.

```jet
fn export(path: Path, pretty: Bool = false) -> Void ? {
    write_export(path, pretty)?
}

fn serve() -> Void ? {
    api.serve()?
}

fn verify_release() -> Void ? {
    require(licenses_ok(), "unapproved license")
}
```

Plural intents run every matching Output: `jet test` runs every Check. Singular intents (`run`, `enter`, `publish`, and activation) use one ordered rule: explicit address; unchanged function-only `fn run`; sole capable Output; checked `defaults:` entry for that capability; otherwise a sorted candidate error. Adding a second singular-capability Output never silently changes automation because a formerly unique command becomes an error until `defaults:` is set.

### Lock, receipts, Hangar, and roots

`.jet/lock` is a small, reviewable index, not a dump of artifact logs. It owns exact package graph identity, solver rationale, provider facts, targets, toolchains, policy, and complete merge edges. Each realization or activation appends an immutable receipt object to the Hangar and records its digest in the lock or generation (`NEW: D-ECO-RECEIPTSTORE1`). The receipt points back to locked inputs; it does not copy merge history.

```text
.jet/lock
  package graph sha256:20f1…
  exact package and toolchain selections
  policy and platform matrix
  successful merge edges with source spans
  receipt refs:
    build todo.cli -> receipt sha256:4ae2…
    system halcyon/gen-42 -> receipt sha256:91b8…

Hangar receipt sha256:91b8…
  locked inputs sha256:20f1…
  planned action sha256:8140…
  output digest sha256:b71e…
  parent generation gen-41
  predicted delta sha256:8ca0…
  readiness proof sha256:7d31…
  activation observation sha256:2f18…
```

Per-user Hangar is the default (`NEW: D-ECO-HANGARPATH1`):

| Platform | Default |
|---|---|
| Linux | `$XDG_DATA_HOME/jet/hangar`, falling back to `~/.local/share/jet/hangar` |
| macOS | `~/Library/Application Support/Jet/Hangar` |
| Windows | `%LOCALAPPDATA%\Jet\Hangar` |

`/etc/jet/hangar` is retired as a default. An administrator may install the ratified socket-activated shared-store broker. It starts per request, exits when idle, never evaluates user source, rebuilds only under ephemeral sandbox identities, and re-verifies bytes, signatures, provenance, and writer authority before promotion (`NEW: D-ECO-BROKERBOUNDARY1`). This is a transient verifier, not a resident daemon or default privileged helper.

Packages, profiles, running processes, builds, toolchains, Systems, and Generations create automatic closure roots. Manual roots cover only external consumers:

```text
$ jet hangar register-external-root backup-sdk ripgrep#2.0.17@nixpkgs --expires-in 12w --yes # NEW: D-JPK-MANUALROOT1
Plan external root
  + backup-sdk
    closure objects: 3
    expires: 12 weeks
    etag: 1.1
Created external root `backup-sdk` at etag 1.1.

$ jet hangar list-external-roots
backup-sdk  ripgrep#2.0.17@nixpkgs  expires in 12 weeks  etag 1.1

$ jet hangar unregister-external-root backup-sdk --etag 1.1 --yes
Removed external root `backup-sdk`.
```

### JetOS is the same graph

A System is an Output closing over packages, services, users, files, typed options, boot facts, images, variants, and proofs. A Fleet is an Output mapping host names to Systems plus shared Configs, host deltas, targets, and rollout policy (`NEW: D-ECO-JETOS2`). JetOS consumes Jetpack's resolver, native Nix compatibility, sandbox, Hangar, cache, toolchain, closure, and receipt substrate. It owns system assembly and activation; it does not fork a provider or build engine.

The lifecycle is the same at every scale:

```text
plan -> build -> verify -> canary -> activate -> observe -> rollback
```

`plan` predicts from checked source and a captured baseline. `proof` records the exact baseline-relative built delta, output digests, readiness results, provenance, activation observations, and rollback artifact. Historical proof never recomputes against the current machine.

## Named resolutions to current tensions

| Tension | Named resolution |
|---|---|
| Three ecosystem shapes | **Typed-root convergence.** Retire role-module and wrapper shapes after migration; ordinary named typed values under Package are final. |
| `pkg.jet` permanence versus `package.jet` | **One-root amendment.** D-ECO-FILEROOT1 explicitly supersedes S52, D-JPK-FILENAME2, and D-JPK-TWONAMES1. |
| Environment identity split | **Environment projection.** One Environment Output feeds `jet env`, `jet dev`, editors, tasks, and CI. `env.jet` is migration input only. |
| U10 package-as-module versus typed values | **Value identity.** D-ECO-DECL1 retires U10's package-is-module rule. Modules remain code namespaces; Package is a value. |
| `/etc/jet/hangar` versus no-root law | **User Hangar.** Platform user-data paths are default; shared storage is explicit administrator policy. |
| U28 no daemon versus MULTIUSER1 broker | **Transient verifier boundary.** Socket activation, idle exit, no source evaluation, and independent verification distinguish broker from a resident daemon. |
| `config.jet` versus unified root | **Root discovery.** `package.jet` supplies Systems and Fleets; `host@root` loads the named Package, not a second OS file model. |
| Native JetOS versus hidden NixOS backend | **Migration-only oracle.** Native JetOS never builds through NixOS. The hidden NixOS realizer is relabeled explicit migration/A-B proof tooling and cannot close native acceptance. |
| Freeze wording | **Epoch-scoped freeze.** D-JETOS-FREEZE1 constrained Epoch 4 implementation. Epoch 7 ratifications reopened System and Fleet work; it is not a global syntax ban. |
| Studio status conflict | **Capability labels.** Compatibility projection is shipped; full source transaction, provenance, proof dashboard, and activation handoff remain separate capabilities until each live path passes. |
| Reality labels vary by document | **Proof-class status.** Every package/JetOS claim is labeled model, fixture, compatibility, live, or hostile-proofed; only the last applicable class closes replacement claims. |
| Lock name overload | **Index plus receipts.** `.jet/lock` owns graph and merge authority; immutable receipt objects hold per-action and per-generation evidence by digest. |
| One graph versus L1/L2/L3 seams | **Semantic unity, layered execution.** L1 owns canonical facts, L2 derives pure plans, and L3 realizes them. All share one versioned graph/receipt contract. |
| No-installed-Nix work duplicated by Jetpack and JetOS | **Single substrate owner.** Jetpack owns native Nix compatibility and package realization; JetOS only consumes its outputs. |
| Vision outruns frozen scope | **Target versus shipped labels.** This proposal is final-shape target law. Roadmaps and help expose only capabilities with matching live evidence. |

## Ladder

Each rung shows the complete authored files for that rung. Generated `.jet/lock`, Hangar objects, and receipts are omitted unless they are the subject of the example.

### S0 — one script

Beginner path: write one file and run it. No manifest, lock, daemon, package directory, or root access.

`hello.jet`:

```jet
fn run() {
    print("Hello, Jet!")
}
```

```text
$ jet run hello.jet
Hello, Jet!
```

Expert controls stay available without changing source:

```text
$ jet build hello.jet --target aarch64-linux-gnu --profile release
Plan build `hello`
  target: aarch64-linux-gnu
  toolchain: jet#3.2.0
  cache: local, then `team-read`
  effects: read hello.jet; write one output
Built sha256:9b72…

$ jet inspect dossier hello.jet
Graph: one source, one executable action, one output
Sandbox: required, passed
Provenance: complete
```

### S1 — script with dependencies

Beginner path: one `use` line adds a pinned package. Jet resolves once, locks by file-content hash, and reuses Hangar bytes.

`report.jet`:

```jet
use textkit#1.4

fn run() {
    print(textkit.wrap("A long weekly report", width: 12))
}
```

```text
$ jet run report.jet
Resolving textkit#1.4
Locked textkit#1.4.2 for report.jet sha256:12ad…
A long
weekly
report
```

Expert path: make the ephemeral lock reviewable and prove offline behavior.

```text
$ jet fetch --lock report.jet
Wrote script lock for report.jet sha256:12ad…
  textkit#1.4.2 sha256:7c01…
  unicode-data#15.1.0 sha256:b119…

$ jet run report.jet --offline
Using satisfied script lock sha256:12ad…
A long
weekly
report

$ jet inspect dossier report.jet
Target: x86_64-linux-gnu
Toolchain: jet#3.2.0
Cache: 2 verified local hits
Schedule: resolve complete; 1 compile action in pool `cpu`
Effects: read report.jet and locked package sources; write one output
Audit: sandbox passed; no network during realization; provenance complete
```

### Transition S1 -> S2 — lift without redesign

`jet init` is the ratified lift. Preview shows the exact source edit and graph fingerprint. The script remains unchanged.

```text
$ jet init --check
Would create: package.jet                         # NEW: D-ECO-FILEROOT1
Would edit: report.jet `use textkit#1.4` -> `use textkit`
Would move: dependency version ownership into Package
package graph before: sha256:65aa…
package graph after:  sha256:65aa…
No files changed.

$ jet init
Created package.jet.
package graph unchanged: sha256:65aa…
```

Generated `package.jet` (`Package` is `NEW: D-ECO-ROOTNAME1`):

```jet
name: "report"
deps: .{ textkit: "1.4" }
```

The reserved filename supplies the `Package` type; top-level fields construct it with no wrapper value or repeated noun. The sole source file is discovered automatically. A single package needs no `members:` field.

Resulting `report.jet`:

```jet
use textkit

fn run() {
    print(textkit.wrap("A long weekly report", width: 12))
}
```

### S2 — first Package

Beginner path is unchanged:

```text
$ jet run
A long
weekly
report
```

`jet new` creates the same shape for a fresh program:

```text
$ jet new weather
Created weather/package.jet.
Created weather/weather.jet.
$ cd weather && jet run
Hello from weather!
```

Fresh `weather/package.jet` names the Package; the sole source file is discovered automatically.

```jet
name: "weather"
```

Fresh `weather/weather.jet`:

```jet
fn run() {
    print("Hello from weather!")
}
```

Expert path exposes the same selected Output and facts:

```text
$ jet inspect output weather                   # NEW: D-ECO-OUTPUT-PAYLOAD1
Selected by: unchanged `fn run` path
Target: x86_64-linux-gnu
Toolchain: jet#3.2.0
Cache key: sha256:118a…
Schedule: 1 compile action, local pool `cpu`
Effects: read weather.jet; write one executable output
Audit: no network, secrets, unsafe code, or remote execution
```

### S3 — hobby application

One file owns application, development environment, service, secret binding, and defaults. The secret value is not in source, lock, or Hangar.

`package.jet` (`Package` and filename are `NEW: D-ECO-ROOTNAME1` and `NEW: D-ECO-FILEROOT1`; `Environment` and `Check` kinds are `NEW: D-ECO-OUTPUT-KINDS1`):

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1; NEW: D-ECO-OUTPUT-DEFAULT1
name: "pulse"
source: "Source"
deps: .{ http: "2.3", postgres: "1.9" }
outputs: .{
    app: .Executable.{ name: "pulse", entry: run }
    unit: .Check.{ name: "unit", entry: test_unit }
}
environments: .{
    dev: .Environment.{
        name: "dev"
        tools: [ripgrep, jet_language_server]
        services: .{
            postgres: .{ enable: true, ports: [5432], ready: postgres_ready }
        }
        secrets: .{ DB_PASS: secret("db-dev") }
    }
}
defaults: .{ run: app, check: unit, enter: dev }
```

`Source/main.jet`:

```jet
fn run() -> Void ? {
    print("pulse ready")
}

fn test_unit() -> Void ? {
    require(2 + 2 == 4, "arithmetic changed")
}

fn postgres_ready() -> Void ? {
    require(true, "postgres did not answer")
}
```

After the environment starts, `jet dev` runs `fn dev()` when present and falls back to `fn run()` (D-JPK-DEVCOMPOSE1).

Beginner path:

```text
$ jet dev
Plan development environment `dev`
  + postgres on 127.0.0.1:5432
  + secret binding DB_PASS (memory only)
postgres healthy
pulse ready
```

Expert path:

```text
$ jet dev --target x86_64-linux-gnu --profile dev --offline
Lock: satisfied; network disabled
Toolchain: jet#3.2.0
Cache: 7/7 verified local hits
Schedule: postgres -> health gate -> pulse
Effects: postgres write(.jet/services/postgres); pulse Secret(DB_PASS), Net(loopback)
Audit: sandbox passed; secret plaintext absent from lock and Hangar; provenance complete

$ jet explain environments.dev.services.postgres.ports
environments.dev.services.postgres.ports = [5432]
  package.jet:16 ordinary [5432]
Source policy: package
```

Representative equivalent split uses three configuration files before application code:

| Shape | Authored configuration lines | Files | Ergonomic difference |
|---|---:|---:|---|
| Jet | 19 | 1 | Package, shell, service, secret binding, and defaults are one checked graph. |
| `flake.nix` + `devenv.nix` + `compose.yaml` | 48 | 3 | Flake projection, shell/service module, and container service repeat package/environment facts; secret handling still needs another policy. |

The comparison counts the shown responsibilities, not comments, lockfiles, or application code. Nix can be shorter with a framework template; Compose can be shorter by using mutable tags. Those change guarantees, so they are not counted as equivalents.

### Transition S3 -> multiple files — extract environment

`jet split env` uses the proposed transition surface (`NEW: D-ECO-TRANSITION1`) and extraction policy (`D-ECO-SPLITPOLICY1`).

```text
$ jet split env --check
Would extract: package.environments
Would create: package/env.jet::development
Would add: Config `development`                # NEW: D-ECO-SLICENAME1
package graph before: sha256:39df…
package graph after:  sha256:39df…
No files changed.

$ jet split env
Created package/env.jet.
package graph unchanged: sha256:39df…
Reverse with: jet fold package/env.jet # NEW: D-ECO-TRANSITION1
```

Resulting `package.jet`:

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1; NEW: D-ECO-OUTPUT-DEFAULT1
configs: [application, development]            // NEW: D-ECO-SLICENAME1
name: "pulse"
defaults: .{ run: app, check: unit, enter: dev }

application :: Config.{                // NEW: D-ECO-SLICENAME1
    source: "Source"
    deps: .{ http: "2.3", postgres: "1.9" }
    outputs: .{
        app: .Executable.{ name: "pulse", entry: run }
        unit: .Check.{ name: "unit", entry: test_unit }
    }
}
```

Resulting `package/env.jet`:

```jet
pub development :: Config.{            // NEW: D-ECO-SLICENAME1
    environments: .{
        dev: .Environment.{             // NEW: D-ECO-OUTPUT-KINDS1
            name: "dev"
            tools: [ripgrep, jet_language_server]
            services: .{ postgres: .{ enable: true, ports: [5432], ready: postgres_ready } }
            secrets: .{ DB_PASS: secret("db-dev") }
        }
    }
}
```

### S4 — expert systems package

The common path remains `jet build`. The same Package records build, host, and target roles; pinned toolchains; remote execution authority; cache roles; resource pools; and audit policy.

`package.jet` (`NEW: D-ECO-ROOTNAME1`, `NEW: D-ECO-FILEROOT1`):

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
name: "edge_agent"
source: "Source"
deps: .{
    wire: "3.1"
    weirdctl: Pkg.adapt(
        name: "weirdctl"
        source: "acme/weirdctl#8a31c9d@github"
        recipe: Recipe.cmake
    )
}
outputs: .{ agent: .Executable.{ name: "edge-agent", entry: run } }
targets: .{
    appliance: .{ build: linux.x64, host: linux.x64, target: linux.arm64, libc: .Musl }
}
profiles: .{
    release: .{ optimize: .Speed, debug_info: .Lines, small: false, panic: .Abort }
}
toolchains: .{ appliance: .{ jet: "3.2.0", sdk: "aarch64-linux-musl#1.2.5" } }
build: .{
    remote: .{ role: "trusted-arm", fallback: .Local }
    cache: .{ read: [team_read], write: team_ci, require_provenance: true }
    scheduler: .{ pools: .{ cpu: 12, memory: 24GB, linker: 1 } }
}
policy: .{
    trust: .{ ci: .DenyPrompt }
    audit: .{ sandbox: .Require, reproducibility: .TwoCleanBuilds }
}
```

`Source/device.jet`:

```jet
use core.mem

#Unsafe("device register is defined by ACME-42") fn reset_device(register: *U32) {
    register.* = 1
}

fn run() -> Void ? {
    print("edge agent ready")
}
```

Beginner path:

```text
$ jet build
Built edge-agent for this machine.
```

Expert path:

```text
$ jet build agent --target appliance --profile release --builder trusted-arm
Plan build `agent`
  build:  x86_64-linux-gnu
  host:   x86_64-linux-gnu
  target: aarch64-linux-musl
  toolchain: jet#3.2.0 + aarch64-linux-musl#1.2.5
  cache: team-read; writer team-ci
  schedule: 9 actions across cpu/memory/linker pools
  authority: remote `trusted-arm`; no network during build
  unsafe: Source/device.jet:3, reason recorded
Verified remote output sha256:0d9f…

$ jet explain package:weirdctl
edge_agent -> build dependency weirdctl
Introduced at package.jet:8
No canonical metadata found; adapter `Recipe.cmake` owns conversion.
Removal: delete `deps.weirdctl` and its only call site.

$ jet explain cache:agent
Hit team-read object sha256:0d9f…
Accepted: target, toolchain, action, output, signature, provenance, and sandbox policy match.
```

### S5 — enterprise monorepo

One root Package lists member Packages by reference and owns shared versions, policy, source authority, cache roles, and CI matrices. Members still declare every direct dependency. A catalog centralizes versions but never grants hidden visibility.

`package.jet` (`NEW: D-ECO-ROOTNAME1`, `NEW: D-ECO-FILEROOT1`):

```jet
name: "acme"
members: find("./packages")
catalog: .{
    http: "4.2.1"
    postgres: "1.9.4"
    tracing: "3.0.2"
}
sources: .{
    public: registry@registry.jet.dev
    company: registry@packages.example.test
}
policy: .{
    providers: .{ company: .{ trust_root: "company-registry-root#7" } }
    licenses: .{ allow: [.Apache2, .MIT, .BSD3], deny: [.AGPL3] }
    advisories: .{ severity: .High, action: .Deny }
    maturity: .{ third_party: 24h, company: 0h }
    cache: .{ read: [company_read], write: company_ci }
    resolution: .Conservative
}
checks: .{
    minimum: .{ resolution: .Lowest, targets: [linux.x64, linux.arm64, macos.arm64, windows.x64] }
    current: .{ resolution: .Latest, targets: [linux.x64, linux.arm64, macos.arm64, windows.x64] }
}
```

`packages/api/package.jet`:

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
name: "api"
source: "Source"
deps: .{ http: catalog.http, postgres: catalog.postgres, tracing: catalog.tracing }
outputs: .{
    server: .Executable.{ name: "acme-api", entry: run }
    unit: .Check.{ name: "api-unit", entry: unit }
}
```

`packages/api/Source/main.jet`:

```jet
fn run() -> Void ? {
    print("api ready")
}

fn unit() -> Void ? {
    require(true, "api unit failed")
}
```

`packages/billing/package.jet`:

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
name: "billing"
source: "Source"
deps: .{ http: catalog.http, api_contract: api }
outputs: .{
    library: .Library.{ name: "billing", modules: [Billing] }
    unit: .Check.{ name: "billing-unit", entry: unit }
}
```

`packages/billing/Source/main.jet`:

```jet
fn unit() -> Void ? {
    require(true, "billing unit failed")
}
```

`packages/web/package.jet`:

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
name: "web"
source: "Source"
deps: .{ http: catalog.http, api_contract: api }
outputs: .{ app: .Executable.{ name: "acme-web", entry: run } }
```

`packages/web/Source/main.jet`:

```jet
fn run() -> Void ? {
    print("web ready")
}
```

Beginner path from the monorepo root:

```text
$ jet test
PASS api.unit
PASS billing.unit
2 passed
```

Dependency changes use the nearest `package.jet`:

```text
$ cd packages/api
$ jet add tracing
Updated packages/api/package.jet
Locked tracing#3.0.2 from catalog.
```

Expert path:

```text
$ jet inspect query build --affected-since origin/main # D-BUILDQUERY1
Changed source: packages/api/Source/contract.jet
Affected packages: api, billing, web
Required checks: api.unit, billing.unit
Required builds: api.server, web.app

$ jet test --affected --resolution lowest --target linux.arm64
Resolution: lowest
Target: aarch64-linux-gnu
Toolchain: jet#3.2.0 + aarch64-linux-gnu SDK#2026.07
Remote cache: 18 verified hits, 2 misses
Remote execution: company-arm (2 actions)
Schedule: 20 actions, peak 8 workers, linker pool 1
Effects: source reads only; test outputs and logs written to build scratch
Audit: license/advisory/maturity policy passed; provenance complete
PASS api.unit
PASS billing.unit

$ jet explain package:tracing
Selected tracing#3.0.2 from registry.jet.dev
Requested by api at packages/api/package.jet:5 through catalog.tracing
Rejected 3.1.0: maturity window has 11 hours remaining
Policy source: package.jet:15
```

Representative equivalent responsibility split:

| Shape | Root/member configuration lines | Files | Missing or duplicated concern |
|---|---:|---:|---|
| Jet root Package + three member Packages | 45 | 4 | One graph carries flat member references, versions, policy, cache roles, CI matrices, and affected queries. |
| Cargo workspace + three Cargo manifests + Bazel workspace/BUILD + Artifactory policy | 86 | 8 | Cargo owns language deps; Bazel repeats target edges; repository/cache policy lives outside both. |

The Cargo-only equivalent is shorter when Cargo builds everything locally, but it does not cover remote action identity, affected queries, source authority, binary-cache writer policy, or lowest/latest platform matrices. The Bazel-only equivalent can own those actions, but then Jet package metadata is modeled twice.

### Transition inside S5 — extract a member

The same `split` operation extracts a member Package. It moves billing's top-level facts into the member's `package.jet` and adds that file to the root's `members:` references.

```text
$ jet split package billing --check             # NEW: D-ECO-TRANSITION1
Would move: billing top-level package facts
Would create: packages/billing/package.jet
Would add member: packages/billing/package.jet
Would preserve canonical address: billing.*
package graph before: sha256:af31…
package graph after:  sha256:af31…
No files changed.

$ jet split package billing                     # NEW: D-ECO-TRANSITION1
Created packages/billing/package.jet.
package graph unchanged: sha256:af31…
```

The split ledger stores stable identity, origin span, destination span, ordinal, tier, and full graph fingerprint. A shared local binding, unresolved collision, or open field refuses before writes. `jet fold packages/billing/package.jet` moves the member's facts back to the root, removes its member reference, restores the exact pre-split `package.jet` bytes, and leaves authored sibling files alone (`NEW: D-ECO-TRANSITION1`).

### S6 — environment manager without an application

A Package may define only an Environment Output; no dummy value or shell hook state exists.

`package.jet` (`NEW: D-ECO-ROOTNAME1`, `NEW: D-ECO-FILEROOT1`, and `NEW: D-ECO-OUTPUT-KINDS1`):

```jet
// NEW: D-ECO-OUTPUT-DEFAULT1
name: "research"
sources: .{ upstream: nixos-unstable@nixpkgs }
environments: .{
    data: .Environment.{
        name: "data"
        tools: [upstream.[python3, duckdb, ripgrep]]
        variables: .{ DATA_ROOT: path("./data") }
    }
}
defaults: .{ enter: data }
```

Beginner path:

```text
$ jet env
[data] ~/research
$ python --version
Python 3.13.5
$ exit
Environment `data` closed.
```

Jet acquires the pinned tools, projects their executables, and tells editors and subprocesses the same resolved paths. It does not run Package code.

Expert path:

```text
$ jet env --target macos.arm64 --offline
Environment: data
Lock: satisfied; network disabled
Tools:
  python3 -> Hangar sha256:13c4…/bin/python3
  duckdb  -> Hangar sha256:a5b2…/bin/duckdb
  ripgrep -> Hangar sha256:50f0…/bin/rg
Cache: 3 verified hits from local Hangar
Effects: read ./data; no package function executed
Toolchain: environment tools pinned independently; Jet runner jet#3.2.0
Schedule: 3 tool projections in parallel; shell starts after all verify
Audit: provider digests verified; no hooks, secrets, network, or package code

$ jet inspect dossier environment:data --json
{"environment":"data","target":"aarch64-darwin","lock":"sha256:…","tools":[…],"effects":[…]}
```

Foreign flake consumption uses the ratified bridge before conversion:

`flake.nix`:

```nix
{
  outputs = { self, nixpkgs }:
    let pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in { devShells.x86_64-linux.default = pkgs.mkShell { packages = [ pkgs.ripgrep pkgs.duckdb ]; }; };
}
```

```text
$ jet env
No Package Environment found. Using flake.nix `devShells.x86_64-linux.default`.
[flake:default] ~/legacy-tools

$ jet init --check
Would create package.jet with Environment `default`.
Would preserve pinned flake input and output address in `.jet/lock`.
Untranslated facts: none.
No files changed.
```

The generated Package contains direct locked provider facts. It does not retain a hidden shell selection or require installed Nix once Jetpack's native compatibility proof is satisfied.

### S7 — one machine

This System covers a graphical laptop, one-line desktop selection, users, themes, file emission, typed options, package overlays, an installer image, and a VM check. The ordinary file is readable top to bottom. Expert precedence appears only where used.

`package.jet` (`NEW: D-ECO-ROOTNAME1`, `NEW: D-ECO-FILEROOT1`, `NEW: D-ECO-JETOS2`, and System/Image/Check kinds under `NEW: D-ECO-OUTPUT-KINDS1`):

Under D-ECO-JETOS2, `profiles:`, `themes:`, the VM `checks:` entry, and `filesystem:` re-home the ratified D-JOS-USERENV1, D-JOS-THEME1, D-JOS-VMTEST1, and D-JOS-DISK1 surfaces as typed Package values.

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
name: "halcyon"
imports: find("./system")
options: laptop_options
themes: .{ halcyon: halcyon_theme }
profiles: .{ nate: .{ packages: [fish, helix, git] } }
systems: .{
    halcyon: .System.{
        name: "halcyon"
        target: linux.x64
        packages: [firefox, ghostty, helix, git, ripgrep, btop]
        users: .{
            nate: .{ shell: fish, groups: [wheel], profile: profiles.nate }
        }
        filesystem: .{
            root: .{ device: "/dev/disk/by-label/jetos", type: .Ext4 }
        }
        hardware: hardware.halcyon
        variants: .{
            rescue: .{ services: .{ desktop: .{ enable: false } }, boot: .{ target: .Rescue } }
        }
        network: .{ hostName: "halcyon", firewall: .{ allowedTcpPorts: [22] } }
        services: .{
            desktop: .{ environment: .Kde }
            audio: .{ pipewire: true }
            openssh: .{ enable: false }
        }
        apps: .{ flatpak: [.{ ref: "org.mozilla.firefox" }] } // NEW: D-ECO-JETOS2
        theme: themes.halcyon
        files: .{
            issue: .{ path: "/etc/issue", text: "jetos 26.10 (Apex)\n", mode: 0o644 }
            sysctl: generated_sysctl
        }
        health: [booted, desktop_ready, user_login]
    }
}
images: .{
    installer: .Image.{
        name: "halcyon-installer"
        from: systems.halcyon
        kind: .Iso
        installer: .{ mode: .Guided, desktop: .Kde, storage: .FromSystem }
    }
}
checks: .{
    vm: .Check.{ name: "halcyon-vm", entry: verify_halcyon_vm }
}
```

`system/options.jet`:

```jet
pub laptop_options :: OptionSet.{   // NEW: D-ECO-JETOS2
    declarations: .{
        desktop: .{
            path: "services.desktop.environment"
            default: .Kde
            docs: "Desktop session installed for graphical login."
            allowed: [.Gnome, .Kde, .Hyprland, .Niri]
        }
        ssh: .{
            path: "services.openssh.enable"
            default: false
            docs: "Accept remote shell connections."
        }
    }
}
```

`system/theme.jet`:

```jet
pub halcyon_theme :: Theme.{            // D-JOS-THEME1; unified placement: D-ECO-JETOS2
    polarity: .Dark
    wallpaper: "wallpapers/forest.png"
    fonts: .{ ui: "Inter", monospace: "JetBrains Mono" }
    palette: .{ accent: "#7aa2f7" }
}
```

`system/hardware.jet`:

```jet
pub halcyon_hardware :: Config.{        // NEW: D-ECO-SLICENAME1; schema: D-ECO-JETOS2
    hardware: .{
        halcyon: .{
            cpu: .Amd64
            graphics: .Amd
            storage: [nvme]
            firmware: [iwlwifi]
        }
    }
}
```

`system/packages.jet`:

```jet
fn workstation() -> Config {                   // NEW: D-ECO-SLICENAME1
    return .{
        overlays: .{
            browsers: .{ firefox: .{ channel: "stable" } }
        }
    }
}

pub workstation_packages: Config :: workstation() // NEW: D-ECO-SLICENAME1
```

`system/files.jet`:

```jet
pub generated_sysctl :: File.{          // NEW: D-ECO-JETOS2
    path: "/etc/sysctl.d/90-jetos.conf"
    text: "vm.swappiness=10\n"
    mode: 0o644
    replace: .Force
}

pub ghostty_config :: File.{            // NEW: D-ECO-JETOS2
    path: "/home/nate/.config/ghostty/config"
    source: path("home/ghostty/config")
    mode: 0o644
    replace: .Backup
}
```

`system/laptop.jet` contributes one closed feature across system and user scope:

```jet
pub laptop :: Config.{                  // NEW: D-ECO-SLICENAME1; schema: D-ECO-JETOS2
    systems: .{
        halcyon: .{ services: .{ power: .{ profile: .Balanced } } }
    }
    profiles: .{
        nate: .{ packages: [brightnessctl], files: [ghostty_config] }
    }
}
```

`system/vm.jet`:

```jet
fn verify_halcyon_vm() -> Void ? {
    host :: vm.host(systems.halcyon)
    host.install(images.installer)?
    host.reboot()?
    host.wait_for_boot(90s)?
    host.assert_unit_active("display-manager.service")?
    host.assert_desktop(.Kde)?
    host.assert_user_login("nate")?
    host.assert_generation_switch()?
    host.assert_rollback()?
}
```

`system/_nvidia.jet` is discovered but disabled by its one-character `_` prefix (`NEW: D-ECO-FILEROOT1`):

```jet
pub nvidia :: Config.{                  // NEW: D-ECO-SLICENAME1; NEW: D-ECO-FILEROOT1
    systems: .{ halcyon: .{ kernel: .{ drivers: [nvidia] } } }
}
```

Beginner path uses only ratified JetOS verbs:

```text
$ jet os plan halcyon
Plan halcyon from generation gen-41
  ~ desktop Gnome -> Kde
  + 14 packages
  + display-manager.service
  ~ /etc/issue
  restart: display-manager.service
  disk: +612 MiB
Rollback point: gen-41
No files or services changed.

$ jet os build halcyon
Built generation gen-42.
Readiness: 12/12 checks passed.

$ jet os image halcyon
Built hybrid installer image `halcyon-installer.iso`.
Guided install: KDE; storage facts from System `halcyon`.
VM install/reboot proof: passed.

$ jet os switch halcyon
Switch halcyon: gen-41 -> gen-42? [y/N] y
Activated gen-42.
Observed healthy for 30s.

$ jet os generations halcyon
* gen-42  26.10 Apex  healthy  parent gen-41
  gen-41  26.10 Apex  healthy  parent gen-40

$ jet os rollback halcyon
Activated gen-41.
```

Expert path shows option origin, generated file, precedence, target, cache, schedule, and proof:

```text
$ jet explain services.desktop.environment
services.desktop.environment = Kde
  system/options.jet:5   Default  Kde
  package.jet:16         ordinary Kde   winner
Derived display manager: Sddm
Rejected contenders: none

$ jet inspect system halcyon --files /etc/sysctl.d/90-jetos.conf # NEW: D-ECO-JETOS2
Source: system/files.jet:1
Mode: 0644
Write: atomic generation projection
Content sha256:660d…

$ jet os proof halcyon --name gen-42
Proof: halcyon generation gen-42
baseline: gen-41
  ~ services.desktop.environment Gnome -> Kde
  + package plasma-desktop#6.3.5
  + service display-manager.service
target: x86_64-linux-gnu
toolchain: jet#3.2.0
cache: 217 verified hits, 4 local builds
schedule: 31 actions, peak 9 workers, linker pool 1
readiness: passed (12/12 checks)
activation: atomic; observed healthy 30s
provenance: complete
receipt: sha256:91b8…
rollback: gen-41 ready
```

The type/default/docs declarations feed source checks, completion, `jet inspect search`, `jet inspect info`, Studio, and generated reference pages. Final-value reads use the checked resolved graph; a dependency cycle reports the shortest option cycle and every source span. Lists and maps merge by their declared type law; unequal ordinary scalars conflict. Experts use ordinary functions to construct one final ecosystem value (D-ECO-COMPOSE2). Only option contributions may use `OptionValue.{ value, priority: .Force }` (D-JOS-PRIORITY-SURFACE2), never a parallel `force` grammar.

Representative NixOS comparison:

| Shape | Authored lines for shown laptop | Files | Error/override model |
|---|---:|---:|---|
| Jet | 117 | 9 | Typed source points at both contributors; ordinary conflicts stop; one explicit priority wrapper; proof records built delta. |
| `configuration.nix` + Home Manager + Stylix/overlay/VM test modules | 116 | 6 | Equivalent concerns use module imports plus `mkDefault`/`mkForce`, overlay functions, and a separate NixOS test expression. |

NixOS may need fewer lines when a module already packages the exact laptop policy. Jet permits the same reuse through an ordinary typed function returning a Config. Jet's comparison advantage is local meaning and proof, not lack of abstraction.

### Transition S7 -> S8 — one host becomes a fleet

`jet split hosts halcyon` extracts the existing System into a host map and creates a sole-host Fleet without changing the System or building anything (`NEW: D-ECO-TRANSITION1`).

```text
$ jet split hosts halcyon --check
Would keep: systems.halcyon
Would add: fleets.home.hosts.halcyon -> systems.halcyon
Would create: package/fleet.jet::home
System graph before: sha256:7780…
System graph after:  sha256:7780…
No files changed.

$ jet split hosts halcyon
Created package/fleet.jet.
System graph unchanged: sha256:7780…
```

Generated `package/fleet.jet`:

```jet
pub home :: Config.{                    // NEW: D-ECO-SLICENAME1
    fleets: .{
        home: .Fleet.{                  // NEW: D-ECO-OUTPUT-KINDS1
            name: "home"
            hosts: .{ halcyon: systems.halcyon }
            rollout: .{ canary: 1, max_parallel: 1, on_failure: .RollbackAndStop }
        }
    }
}
```

### S8 — multi-host fleet

The fleet adds two web hosts. Shared Configs carry common packages and health rules. Per-host deltas contain only facts that differ.

`package/fleet.jet` (`NEW: D-ECO-SLICENAME1`, `NEW: D-ECO-OUTPUT-KINDS1`, and `NEW: D-ECO-JETOS2`):

```jet
// NEW: D-SHAPE-OUTPUT-CALLABLE1
use core.http as http
use core.net as net

fn run_api() -> Void ? {
    print("api ready")
}

fn api_ready() -> Void ? {
    stream :: net.tcp_connect("127.0.0.1:8080")?
    stream.close()?
}

fn api_can_serve() -> Void ? {
    response :: http.get("http://127.0.0.1:8080/health")?
    require(response.status() == 200, "api could not serve a request")
}

pub web_base :: Config.{
    source: "Source/api"
    outputs: .{ api: .Service.{ name: "api", entry: run_api } }
    systems: .{
        web: .{
            target: linux.x64
            packages: [api, curl, btop]
            services: .{ api: .{ enable: true, ports: [8080], ready: api_ready } }
            health: [booted, api_ready, api_can_serve]
        }
    }
}

pub production :: Config.{
    systems: .{
        web1: web_base.systems.web.with(.{ network: .{ hostName: "web1" }, region: "us-east" })
        web2: web_base.systems.web.with(.{ network: .{ hostName: "web2" }, region: "eu-west" })
    }
    fleets: .{
        prod: .Fleet.{
            name: "prod"
            hosts: .{
                web1: .{ system: systems.web1, target: .{ binding: "web1" }, authority: .{ identity: "deploy", privilege: .Root } }
                web2: .{ system: systems.web2, target: .{ binding: "web2" }, authority: .{ identity: "deploy", privilege: .Root } }
            }
            rollout: .{
                cohorts: [[web1], [web2]]
                canary: 1
                max_parallel: 1
                health_timeout: 90s
                on_failure: .RollbackAndStop
            }
        }
    }
}
```

Host endpoints and credentials are local bindings, not repository authority. These commands are explicit one-time host setup; the beginner path starts at `jet deploy prod`:

```text
$ jet remote bind web1 ssh://deploy@web1.example.test --credential company-ssh
Bound `web1`; repository source unchanged.
$ jet remote bind web2 ssh://deploy@web2.example.test --credential company-ssh
Bound `web2`; repository source unchanged.
```

Beginner path uses the recommended top-level fleet intent (`NEW: D-ECO-FLEETVERB1`):

```text
$ jet deploy prod                                # NEW: D-ECO-FLEETVERB1
Plan fleet `prod`
  hosts: web1, web2
  canary: web1
  then: web2
  health gates: booted, api_ready, api_can_serve
  failure: rollback changed host and stop
Push this plan? [y/N] y
web1  built gen-88
web1  verified 3/3
web1  activated gen-88
web1  healthy 90s
web2  built gen-91
web2  verified 3/3
web2  activated gen-91
web2  healthy 90s
Fleet `prod` healthy: 2/2.
```

Expert path separates stage, activation, concurrency, audit, and per-host rollback without changing declaration semantics:

```text
$ jet deploy prod --stage-only --cohort web --max-parallel 2 --json
{"fleet":"prod","stage":"built","hosts":{"web1":"gen-88","web2":"gen-91"},"activated":[]}

$ jet inspect fleet prod                       # NEW: D-ECO-JETOS2
Targets: web1/web2 x86_64-linux-gnu over bound SSH identities
Toolchain: jet#3.2.0; system closure schema 1
Cache: 431 verified hits, 6 builds
Schedule: 2 hosts, max parallel 2, linker pool 1 per builder
Effects: SSH to bound hosts; generation writes only; no undeclared host paths
Audit: host keys, privilege boundary, plans, receipts, and rollback roots complete

$ jet deploy prod --from-stage --canary web1 --observe 10m
web1 activated gen-88; healthy 10m
web2 activated gen-91; health failed: api_can_serve
web2 rolled back gen-91 -> gen-90
Rollout stopped. web1 remains healthy on gen-88.

$ jet os proof web2 --name gen-91
baseline: gen-90
activation: rolled back
failed gate: api_can_serve at 2026-07-15T14:31:08Z
rollback proof: gen-90 healthy
receipt: sha256:6d22…
```

Representative fleet comparison:

| Shape | Authored fleet/deploy lines | Tools | Lifecycle split |
|---|---:|---:|---|
| Jet | 29 | 1 front door, separate Jetpack/JetOS engines | Same graph and receipt: plan, build, verify, canary, activate, observe, rollback. |
| Colmena/deploy-rs + Terraform + Ansible | 91 | 3-4 | Nix host evaluation, remote infrastructure state, and imperative repair each carry separate addresses, state, and failure reports. |

Colmena or deploy-rs alone can be shorter for an SSH-only NixOS switch. Terraform or Ansible alone can be shorter for one narrow resource. The comparison covers the shown locked system build, staged health gate, observed activation, and per-host rollback.

## Growth invariants

Every transition follows one contract:

| Transition | Command | Semantic test | Reverse |
|---|---|---|---|
| Script dependency to Package | `jet init` | Same script dependency and output graph fingerprint | `jet init --restore-script` during migration epoch |
| Inline environment to file | `jet split env` (`NEW: D-ECO-TRANSITION1`) | Same Environment identity and package graph fingerprint | `jet fold package/env.jet` (`NEW: D-ECO-TRANSITION1`) |
| Root package facts to flat member file | `jet split package <name>` (`NEW: D-ECO-TRANSITION1`) | Same canonical `<member>.*` address, affected query result, and graph fingerprint; root gains one member reference | `jet fold <member-file>` (`NEW: D-ECO-TRANSITION1`) |
| System to Fleet host map | `jet split hosts <name>` (`NEW: D-ECO-TRANSITION1`) | Same System identity, generation roots, and build plan | `jet fold package/fleet.jet` (`NEW: D-ECO-TRANSITION1`) |

Every command previews by default when writes could discard or relocate authored text. It refuses on shared bindings, ambiguous ownership, collisions, or fields that cannot close over their dependencies. A successful split records enough source provenance to prove graph equality and restore byte-identical source. Files added by the user after a split are never consumed by fold.

## JetOS completeness check

The S7/S8 shape covers every requirement in the Epoch 4 JetOS research appendix without adding another configuration mechanism:

| Required reach | Where it appears |
|---|---|
| Multi-host, ISO host, hardware, variants | Fleet host map; Image `.Iso`; hardware Config; named System deltas and boot variants. |
| Typed options with type/default/docs/enums | `OptionSet` declarations feed checks, `jet inspect search`, `jet inspect info`, Studio, and docs (`NEW: D-ECO-JETOS2`). |
| Final-value reads and cycle diagnostics | Resolved graph view; shortest cycle with all source spans. |
| Deterministic merge and provenance | Field-law composition; `.jet/lock`; `jet explain` shows contributors, priority, winner, and reason. |
| One-character disable | S7's `system/_nvidia.jet` uses D-ECO-FILEROOT1's proposed discovered-file rule. |
| File emission | Typed `.{ path, text, mode, replace }` entries projected inside generation (`NEW: D-ECO-JETOS2`). |
| Stable/unstable sets, overlays, custom derivations | S6 shows `nixos-unstable@nixpkgs`; S7 shows a stable browser Overlay. Custom derivation acceptance uses ordinary Package recipes and is not shown here. |
| One feature spanning system and user scope | A Config may contribute System facts and referenced user Profile facts in one closed value. |
| KDE, GNOME, Hyprland, Niri and display managers | S7 shows representative KDE selection. GNOME/Hyprland/Niri and display-manager swap acceptance over the same typed field are not shown here. |
| Theming | S7 shows a representative Theme value. Owner-stack parity across Home Manager/Stylix/NUR-class breadth is acceptance work over the same mechanism, not shown here. |
| Flatpak, AppImage, native packages | S7 shows one Flatpak app fact and native packages. AppImage acceptance over the same typed app facts is not shown here. |
| Installer | Image Output `.Iso`; guided install drafts editable storage facts; scripted path consumes the same graph. |
| VM tests | S7's Check calls `install`, `reboot`, boot, service, desktop, login, generation-switch, and rollback assertions on one typed host. |
| Fleet rollout | Typed targets and authority, staged canary, bounded concurrency, health gates, rollback-and-stop, per-host receipts. |

Raw escape hatches remain explicit and audited. A compatibility file or service action declares effects, checkpoints, compensation, and proof. It cannot mutate outside the generation and still claim declarative activation.

## Appendix A — fifteen required transplants

| Feature | Design home | Worked Jet moment |
|---|---|---|
| Zero-ceremony common path | S0-S3 | `jet run`, `jet add`, `jet env`, and `jet dev` select the sole capable Output without graph vocabulary. |
| Complete immutable identity | Graph, lock, receipt | `jet inspect output todo.cli` separates action digest from output digest and shows source, toolchain, target, policy, and effects. |
| Atomic profiles, generations, rollback | Profile/System/Fleet Outputs | `jet os switch` moves one pointer after proof; `jet os rollback` activates the recorded parent. |
| Whole-workflow ownership | One `jet` front door | Resolve, lock, build, run, test, publish, environment, image, system, and fleet intents dispatch to versioned engines. |
| What/why/fix explanations | One graph and diagnostic registry | `jet explain package:tracing` shows introducer, selected and rejected versions, policy source, and removal path. |
| Shared content store | Hangar | Verified bytes live once; roots and leases retain closures; `jet clean` alone collects and optimizes. |
| Hermetic effect-typed builds | Action graph and grants | S4 reports declared reads/writes, remote authority, sandbox result, secret/network use, and unsafe audit reason. |
| Readable exact lock | `.jet/lock` index | Targeted `jet update <pkg>` changes one rationale subtree while unrelated records stay byte-identical. |
| Monorepo and single-file parity | Package ladder | S1 inline deps and S5 flat member packages lower to the same package graph and cache identities. |
| Toolchain and target ownership | Target/Toolchain facts | S4 distinguishes build, host, and target and keys artifacts by SDK, ABI, libc, and toolchain. |
| Secure substitution and remote execution | Cache roles and RemoteBuild | A cache hit must match digest, signature, provenance, platform, sandbox, and policy; miss falls back to a source build. |
| Typed composition with provenance | Config field laws | `jet explain services.desktop.environment` shows defaults, ordinary values, explicit priority, winner, and source spans. |
| Plan before mutation | Plan/proof lifecycle | S7 predicts disk/service changes before build; S8 previews cohorts and rollback behavior before push. |
| Open provider and recipe ecosystem | Provider roots and Pkg.adapt | Direct roots preserve provider facts; an adapter converts fetched bytes under the same lock, sandbox, and audit rules. |
| Laptop-to-fleet desired state | System and Fleet Outputs | The same System identity becomes one Fleet host; staged rollout adds execution policy, not another declaration language. |

## Appendix B — ten failures made structurally impossible

| Failure to avoid | Design feature that prevents it |
|---|---|
| Users debug evaluator or backend internals | Eager typed graph checks point at user source. rustc and provider internals remain optional debug context under Jet-owned what/why/fix diagnostics. |
| Global files mutate without a generation | Profiles, users, Systems, and Fleets activate immutable closures by pointer; file projection occurs inside a generation. |
| Resolution or install executes undeclared code | Metadata probes never execute code. Hooks and adapters are digest-bound, reviewed, effect-declared, and sandboxed. |
| Merge and override algebra is folklore | One field law, ordinary functions for final ecosystem values, option-only `OptionValue` contributions, and complete locked provenance replace last-file-wins and constructor families. |
| A distant consumer silently changes package behavior | Direct dependency visibility and closed variant domains prevent additive feature or peer-dependency leakage across unrelated graph members. |
| An under-specified build is called reproducible | Lock identity includes source, toolchain, target, effects, environment, and policy; clean rebuild divergence creates failed proof. |
| Lockfiles tell partial truth | `.jet/lock` owns exact graph facts and reasons; receipt digests connect those facts to observed bytes and activation. |
| Physical store layout becomes API | Packages import declared logical identities. Hangar paths, mounts, links, and Nix path projections remain realization details. |
| One lifecycle splits across rival schemas | Package, Output, lock, receipt, plan, proof, and explain vocabulary remain unchanged from script through fleet. |
| Declarative plans imply certainty | Plans predict; receipts record observed readiness, failure, compensation, activation, and rollback against a captured baseline. |

## Appendix C — Nix exit moments replaced

| Nix exit moment | Jet replacement |
|---|---|
| First install asks for daemon and root choices | Per-user Hangar works first. Shared broker is an administrator opt-in and absent without consequence. |
| Old and experimental command families disagree | One versioned `jet` command registry dispatches to engines and rejects version skew. |
| Package attribute differs from executable name | `jet inspect search ripgrep` returns Package and Output names; `jet inspect info` shows the exact runnable address. |
| Dev shell requires choosing `shell.nix`, `flake.nix`, or framework | `jet env` selects the Package's Environment Output or consumes a foreign flake when no Environment exists. |
| Flake schema and `${system}` precede first build | `jet build` derives host target; `--target` reveals explicit control only when asked. |
| Untracked source disappears from a flake snapshot | Plan names every included and excluded source with the owning source-set rule before build. |
| Lazy evaluation fails through library frames | Eager graph checks report the user declaration, failed contract, shortest dependency or option cycle, and fix. |
| Overrides require choosing among several layers | Ordinary contributions merge; disagreement stops; the sole expert precedence spelling is `OptionValue.{ value, priority }`. |
| Cache miss triggers an unexplained rebuild | `jet explain cache:<output>` names first mismatching identity or trust fact and the exact fallback. |
| Update fanout is unclear | `jet update <pkg> --check` previews selected/rejected versions, rebuild set, policy, and untouched lock subtrees. |
| Deployment needs NixOS plus another fleet tool | System and Fleet are Outputs in one graph with plan, proof, staged push, health gates, and per-host rollback. |
| Cleanup requires learning roots, profiles, and result links | `jet clean --check` lists reclaimable bytes and why retained closures remain live; automatic owners need no manual roots. |
| Documentation spans language, package set, flakes, modules, and wiki eras | Command, schema, option, and diagnostic registries generate one versioned manual with runnable examples. |
| Team adoption exposes daemon trust, substituters, builders, and secrets at once | Safe local defaults remain implicit; source requests roles while administrators bind endpoints and credentials separately. |

## Decision stack

All 19 rows are decided. Later rows depend on earlier vocabulary. The stack is now the ratified record; every outcome preserves one beginner path and exposes expert controls through the same mechanism. **19 decided · 0 open.**

### 1. D-ECO-ROOTNAME1 — name the semantic whole

**Answers/replaces:** D-ECO-ROOTNAME1 and umbrella gate D-ECO1.

**Gist:** Record the noun used in source, diagnostics, docs, and inspection for the repository-neutral semantic whole.

**Ratified (2026-07-15): I — Package.** One noun spans script through fleet; `members:` contains references only, with depth capped at one.

Source example: `members: find("./packages")`. The reserved file supplies the root `Package` type; ordinary single packages need no `members:` field.

Rejected: Hub, Manifest, Project.

### 2. D-ECO-SLICENAME1 — name one typed contribution

**Answers/replaces:** D-ECO-SLICENAME1.

**Gist:** Record the noun for one layout-neutral typed value that contributes facts to a Package.

**Ratified (2026-07-15): G — Config.** Plain English distinguishes code modules, shipped Packages, and merged settings while matching NixOS migration vocabulary.

Rejected: Shard, Spoke, Part.

### 3. D-ECO-FILEROOT1 — choose the final source-file law

**Answers/replaces:** D-ECO-SOURCE1; overturns S52, D-JPK-FILENAME2, D-JPK-TWONAMES1, and D-JPK-OSHOST1's `config.jet` path while preserving bare-host resolution. It extends U3's module-level leading-underscore disable rule to discovered files.

**Gist:** Decide whether one reserved Package file replaces permanent package, environment, workspace, and OS role files.

**Ratified (2026-07-15): A — One `package.jet`.** Bare top-level fields construct the Package; one teaching diagnostic folds `pkg.jet`, `env.jet`, `workspace.jet`, and `config.jet` during one migration epoch while scripts remain file-free.

Rejected: keep role files; no reserved file.

### 4. D-ECO-SPLITPOLICY1 — define what split does

**Answers/replaces:** D-ECO-SPLITPOLICY1.

**Gist:** Decide whether `jet split` extracts inline facts or only moves an already-authored Config.

**Ratified (2026-07-15): A — Extract and move.** `jet split` extracts closed inline facts into the same Config an expert would write, previews the generated binding, and records reversible provenance.

Rejected: move Configs only.

### 5. D-ECO-TRANSITION1 — name growth and reversal commands

**Answers/replaces:** new gate surfaced by S3, S5, and S7 transitions; uses D-ECO-SPLITPOLICY1's semantics.

**Gist:** Record the commands for extracting and folding package tiers.

**Ratified (2026-07-15): A — `jet split` / `jet fold`.** Separate growth and reversal verbs share one provenance ledger and make reversal explicit.

Rejected: flag-based reversal, intent-specific transition verbs.

### 6. D-ECO-OUTPUT-PAYLOAD1 — decide where output facts live

**Answers/replaces:** D-ECO-OUTPUT-PAYLOAD1.

**Gist:** Decide whether Output values are thin typed projections or repeat graph slices.

**Ratified (2026-07-15): A — Thin projections.** Output payloads keep only name and kind-specific facts; `jet inspect output` reconstructs shared graph facts and provenance without duplication.

Rejected: explicit slice per Output; verb sections.

### 7. D-ECO-OUTPUT-KINDS1 — close the v1 Output kind set

**Answers/replaces:** new closure gate left open by D-SHAPE5b and D-ECO-OUTPUT-PAYLOAD1.

**Gist:** Decide whether v1 has one closed kind set spanning package and JetOS results.

**Ratified (2026-07-15): A — Nine closed kinds.** `Library`, `Executable`, `Service`, `Check`, `Environment`, `Image`, `Bundle`, `System`, and `Fleet` make tooling exhaustive; new kinds require ratification.

Rejected: package-only closure; extensible text kinds.

### 8. D-SHAPE-OUTPUT-CALLABLE1 — link runnable Outputs to code

**Answers/replaces:** D-SHAPE-OUTPUT-CALLABLE1.

**Gist:** Choose the one checked relationship between a runnable Output and ordinary Jet code.

**Ratified (2026-07-15): A — Function reference.** Checked entry references reuse normal resolution, visibility, rename, provenance, and role validation while preserving zero-config `fn run`.

Rejected: module with `run`; text name.

### 9. D-ECO-OUTPUT-CALLCONTRACT1 — define callable role contracts

**Answers/replaces:** D-ECO-OUTPUT-CALLCONTRACT1.

**Gist:** Choose how commands, services, and checks use ordinary functions.

**Ratified (2026-07-15): A — Role-specific ordinary functions.** Executables derive typed flags from parameters; Services and Checks take no ad hoc invocation flags and report success through ordinary return.

Rejected: one permissive shape; lifecycle result types.

### 10. D-ECO-OUTPUT-DEFAULT1 — select an omitted Output address

**Answers/replaces:** D-ECO-OUTPUT-DEFAULT1.

**Gist:** Choose the deterministic selection rule for singular `run`, `enter`, `publish`, and activation intents; plural `test` runs every Check Output value.

**Ratified (2026-07-15): A — Plural all; singular explicit, legacy, sole, defaults, error.** `jet test` runs every Check; singular intents follow that five-step rule so small Packages stay automatic and growth cannot silently retarget automation.

Rejected: conventional keys; always explicit after Outputs.

### 11. D-SHAPE-INTERNAL1 — define `pub _name`

**Answers/replaces:** D-SHAPE-INTERNAL1.

**Gist:** Decide whether a public underscore name is callable without becoming a compatibility promise.

**Ratified (2026-07-15): A — Soft-public.** `pub _name` permits outside use with one unsuppressible warning while excluding the name from supported API and semver promises.

Rejected: all public is supported.

### 12. D-ECO-JETOS2 — connect Package to JetOS

**Answers/replaces:** D-ECO-JETOS2 and the JetOS half of D-ECO1. It re-homes ratified D-JOS-USERENV1 (`user.<name>`), D-JOS-THEME1 (`theme.<name>`), D-JOS-VMTEST1, and D-JOS-DISK1 spellings as typed Package values — semantics preserved, re-spelling gated by this row.

**Gist:** Decide whether Systems and Fleets are Outputs of the same package graph and use the same realization substrate.

**Ratified (2026-07-15): A — Same graph, typed Outputs.** Systems and Fleets share Package identity, policy, cache, explanation, and receipts while JetOS retains its activation engine.

Rejected: same source with separate OS graph; separate OS root.

### 13. D-ECO-JETOS-PREVIEW1 — define plan and proof

**Answers/replaces:** D-ECO-JETOS-PREVIEW1.

**Gist:** Decide whether proof preserves the exact built delta from a captured parent generation.

**Ratified (2026-07-15): A — Plan predicts, proof confirms.** Plan previews change; proof preserves exact built delta from its captured baseline plus outputs, readiness, activation, provenance, and rollback.

Rejected: plan as the only delta.

### 14. D-ECO-RECEIPTSTORE1 — place connected receipts

**Answers/replaces:** the schema/location choice intentionally left open by D-ECO-RECEIPT2 and the lock-overload tension.

**Gist:** Choose where action and activation evidence lives without creating a second merge authority.

**Ratified (2026-07-15): A — Hangar objects referenced by lock/generation.** Immutable receipt objects use Hangar identity, deduplication, signing, retention, and export while `.jet/lock` remains the sole graph and merge index.

Rejected: inline every receipt in `.jet/lock`; package-local receipt directory.

### 15. D-ECO-FLEETVERB1 — choose fleet lifecycle verbs

**Answers/replaces:** new fleet command gate left open by D-JPK-FLEET1 and D-JOS-FLEETROLLOUT1; amends D-CLI-SURFACE3's grouping of `jet os push`.

**Gist:** Record the command users type for plan, staged rollout, observation, and rollback across hosts.

**Ratified (2026-07-15): A — `jet deploy <fleet>`.** `deploy` names fleet rollout directly and leaves `push` free for another job.

Rejected: `jet fleet push`, `jet os push`, bare push.

### 16. D-JPK-MANUALROOT1 — name external retention operations

**Answers/replaces:** D-JPK-MANUALROOT1.

**Gist:** Record the rare expert operations that retain a closure with no Package, profile, process, build, toolchain, System, or Generation owner.

**Ratified (2026-07-15): B — `register-external-root` / `unregister-external-root` / `list-external-roots`.** Precise verbs distinguish retention metadata from realization in scripts, CAS errors, and audit records.

Rejected: add/remove, keep/release, pin/unpin.

### 17. D-ECO-HANGARPATH1 — replace the root-owned default path

**Answers/replaces:** S52's `/etc/jet/hangar` location and the no-root tension; preserves D-JPK-MULTIUSER1.

**Gist:** Choose the default physical Hangar ownership model across Linux, macOS, and Windows.

**Ratified (2026-07-15): A — Native per-user data path.** Linux uses `$XDG_DATA_HOME/jet/hangar` or `~/.local/share/jet/hangar`, macOS uses `~/Library/Application Support/Jet/Hangar`, and Windows uses `%LOCALAPPDATA%\Jet\Hangar`; shared storage stays optional.

Rejected: `/etc/jet/hangar` default; package-local `.jet/hangar`.

### 18. D-ECO-BROKERBOUNDARY1 — reconcile U28 with shared storage

**Answers/replaces:** wording conflict between U28/D-JPK-NODAEMON1 and D-JPK-MULTIUSER1.

**Gist:** State the privilege and lifetime boundary that keeps a shared Hangar optional and non-resident.

**Ratified (2026-07-15): A — Socket-activated transient verifier.** Optional administrator-installed broker exits when idle, never evaluates user source, rebuilds under ephemeral identities, and independently verifies promotion into shared storage.

Rejected: resident build daemon; no privileged process ever.

### 19. D-ECO-MEMBERS1 — how packages relate

**Answers/replaces:** package containment and workspace-noun questions left open by D-ECO-ROOTNAME1.

**Gist:** Decide whether monorepo packages contain package definitions or refer to independent packages.

**Ratified (2026-07-15): A — Flat members.** A monorepo root lists `members:` by reference, members cannot have members, and a single Package needs no membership field.

Rejected: full recursion (noun soup), a two-noun workspace split, and renamed sub-units.

```text
$ jet check
Error: member package `packages/api/package.jet` lists members from `packages/api/modules/package.jet`
 What: root `package.jet` references `packages/api/package.jet`, which also has a `members:` field
 Why: package membership has depth cap 1; members cannot have members
 Fix: remove the inner `members:` field and lift its references into root `package.jet`
```

### Decision order

Rows 1-19 are ratified in dependency order: vocabulary and membership, file growth, Output shape and selection, JetOS integration and proof, then Hangar retention and sharing. This stack is the durable decision record; `NEW:` markers identify ratified spellings whose implementation remains tracked on Tower cards.
