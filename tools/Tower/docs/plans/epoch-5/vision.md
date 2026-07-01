# The jetpack/jetos vision — one language from script to datacenter

**Status:** vision draft for owner review, 2026-07-01. Builds on the ratified
surface in `unified-ecosystem.md` (U1–U10, D-PM1–8) — this doc adds the
end-state picture and the domain walkthroughs; it does not re-ratify anything.
Naming follows the latest ratifications: `pack.jet` (C-MANIFEST), root manifest
= `pack.jet` with `payload:` (supersedes U10's `jetpack.toml`; the
`unified-ecosystem.md` tables need that rename sweep).

---

## The sentence

**Everything your software needs — code, dependencies, tools, services,
machines — is declared in Jet, typed, checked, locked, and reproducible, with
one command per intent.**

Nix proved the results people want: perfect reproducibility, per-project
environments, declarative machines, rollback, one deduplicated store. Nix also
proved the cost people won't keep paying: a second untyped lazy language,
a learning cliff (derivation → stdenv → mkShell → overlay → flake), stack-trace
errors, bolted-on services/secrets, and disk churn. Jet keeps the results and
deletes the cost, because the config language *is* the application language —
typed, compiled, LSP'd, effect-checked, with what/why/fix diagnostics.

## Two names total — one file, one command

The whole surface is **one reserved file name and one command**:

- **`pack.jet`** is the only file name the tool ever looks for. Payload, deps,
  `env.dev`, `system.laptop`, `fleet.prod`, and workspace members either live
  in it or in any `.jet` file it discovers via `find()` (U4). The module
  namespace carries the role — a file named `env.jet` holding `module env.dev`
  is the *user's organizational choice*, never a requirement. Scaling means
  adding a module, not learning a new file name.
- **`jet`** is the only command users type. `jetpack` and `jetos` remain the
  engine binaries and layer names (the one-way arrow survives: `jet`
  dispatches unknown verbs to them by name, git-style, zero coupling — the
  compiler stays fully standalone). Verbs resolve against declared modules:
  no `system.*` module in scope → `jet switch` is a teaching error saying
  what to declare. (The D-TOOL-SPLIT research already found one-binary,
  tool-subcommands is the industry-winning shape.)

**[GATE U18: revises U10 file structure, D-WORKSPACE2 workspace.jet, and the
CLI split — ballot before implementation.]**

## The ladder — progressive disclosure, same syntax every rung

| Rung | You declare | Command | Nix equivalent you no longer need |
|---|---|---|---|
| 0 script | nothing — one `.jet` file | `jet run app.jet` | nix-shell shebangs |
| 1 package | `pack.jet`: `payload:` + `deps:` | `jet build` | flake.nix outputs |
| 2 environment | `module env.dev {}` | `jet dev` / `jet env` | devShell + devenv + direnv |
| 3 machine | `module system.laptop {}` | `jet switch` | NixOS configuration.nix + home-manager |
| 4 fleet | `module fleet.prod {}` | `jet push` | colmena / deploy-rs / NixOps |

Climbing a rung never invalidates the rung below, and never introduces a new
file name or command — only a new module namespace. Rung 0 never needs a
manifest (U7/PM-I8, sacred).

### Rung 0 — scripts with inline dependencies

```jet
// stats.jet — the only file on disk
use textkit#1.4

fn run() {
    print(textkit.wrap(input(), width: 72))
}
```

```
$ jet run stats.jet        # resolves, locks (cache keyed by file), runs
```

The `pkg#version` pin already ratified for refs rides the `use` statement.
This makes Jet the best scripting language on the machine — uv/bun-style
single-file ergonomics with a compiler behind them. **[GATE: inline-dep `use`
syntax — needs a ballot before implementation.]**

### Rung 1 — the package

```jet
// pack.jet
payload: {
    name: "pulseops"
    version: "0.4.0"
}

packages: {
    pulseops: executable { entry: "src/run.jet" }
}

deps: {
    textkit: "1.4.2"
    helpers: path("../helpers")
    stable: github@NixOS/nixpkgs#nixos-25.05     // nix provider, inferred (U9)
}
```

Lifecycle verbs live in code, not config: `fn run()`, `fn dev()`,
`fn build(b: BuildContext)` (D-BUILDENTRY1). The manifest declares *what the
package is*; behavior is Jet functions.

### Rung 2 — the dev environment, the devenv killer

```jet
// in pack.jet — or any discovered file; the module namespace is the role
module env.dev {
    use jetpack.presets as presets

    imports: [presets.web]                    // toolchain bundle, one line

    packages: [default.[postgres_16, ripgrep], unstable.zig]

    services: {
        postgres: Service.{ version: 16, port: 5432, init: "schema.sql" }
        redis: Service.{}
    }

    env_vars: { DATABASE_URL: "postgres://localhost:5432/pulseops_dev" }

    secrets: { STRIPE_KEY: secret("stripe-dev") }

    on_enter: () => print("pulseops dev ready")
}
```

```
$ jet env            # base env shell — tools only, nothing runs
$ jet env dev        # shell with the dev overlay — still nothing runs
$ jet dev            # EXPLICIT: env(base+dev) -> services -> fn dev() + reload
$ jet services logs postgres
```

**The env/dev split (U19/D-JPK-DEVCOMPOSE1, owner direction 2026-07-01):**
entering an environment and running project code are different intents with
different risk. `jet env [name]` realizes the environment (base `env.*` plus
an optional named overlay, U5 merge) and opens a shell — *no project function
ever runs from env entry*. `jet dev` is the explicit execution verb: it
realizes env(base + `env.dev`), waits for services, then runs `fn dev()`
(fallback `fn run` under watch/reload). `jet test`/`jet build` use
`env.test`/`env.build` the same way. Because `on_enter` hooks and service
definitions are also project-authored code, the first `jet env` in a fresh
repo shows a trust summary (hooks, services, sources) and requires a
direnv-style allow; the grant re-prompts when the env definition changes.
`jet env --trust` bypasses the prompt in one shot when you trust the source
(CI, scripts, your own repos); `jet config trust add github.com/acme/*`
pre-trusts sources by pattern so matching repos never prompt.

What this deletes from the Nix world: `mkShell`, devenv.nix, direnv glue,
process-compose YAML, `.envrc`, and the secrets shell script. Services are
supervised by jetpack (up/down/health/logs). Typing `services.` in the editor
completes every known service with its typed options and docs — the single
biggest daily-life gap in Nix, where option discovery means grepping nixpkgs.
**[GATE: Service/secret schema — ballot before implementation.]**

### Rung 3 — the machine (jetos layer)

```jet
// in the machine repo's pack.jet (default ~/.jet/, any repo you choose —
// never force-moved), or a discovered module file
module system.laptop {
    imports: find("./modules")

    packages: [default.[firefox, ghostty, ffmpeg]]

    services: {
        tailscale: Service.{}
        pipewire: Service.{}
    }

    users: {
        nate: User.{ shell: fish, groups: [wheel] }
    }
}
```

```
$ jet switch --name "pre-gpu-driver"   # atomic, named generation
$ jet generations                       # named history
$ jet rollback                          # instant
```

Same module grammar, same merge table (U5), same diagnostics as a dev shell.
NixOS + home-manager + flake-parts collapse into one model the user already
learned at rung 2. Named generations are the c27 card, delivered.

### Rung 4 — the fleet

```jet
// same repo, one more module
module fleet.prod {
    hosts: {
        web1: system.web.{ region: "us-east" }
        web2: system.web.{ region: "eu-west" }
        db:   system.database.{ replicas: 2 }
    }
}
```

```
$ jet push prod                # ssh deploy, staged, per-host rollback
```

Copy-with-update on a system module *is* the per-host override story — no
override/overlay spaghetti. **[GATE: fleet namespace — ballot before
implementation.]**

## The Nix bridge — adopt without asking permission

The three daily Nix flows map verb-for-verb (U16/D-JPK-BRIDGE1):

| You do today | With jet |
|---|---|
| `nix-shell -p nodejs ripgrep` | `jet env -p nodejs ripgrep` |
| `nix develop` (repo flake) | `jet env` — detects `flake.nix`/`devenv.nix` when no `env.*` modules exist, realizes the foreign devShell |
| `nix run nixpkgs#fastfetch` | `jet run nixpkgs@fastfetch` |

Phase 1 shells out to the nix binary underneath (ratified stopgap — needs Nix
installed, clear message otherwise); the UX contract is jet's and survives the
core provider absorbing realization later, so muscle memory built today never
breaks.

Interop is the adoption weapon, staged as consume → coexist → replace:

1. **Consume (day one).** Any flake input is a jetpack source via the inferred
   nix provider (U9, shipped policy): `stable: github@NixOS/nixpkgs#nixos-25.05`
   puts 100k nixpkgs packages behind typed refs. Requires Nix on the machine —
   the ratified stopgap.
2. **Coexist.** `jet env` can realize a foreign `flake.nix` devShell or
   `devenv.nix` directly — you can use jetpack inside a Nix team without
   converting anyone. `jet bridge flake` emits a `flake.nix` shim so Nix
   users consume *your* Jet packages. The door swings both ways, so migration
   is incremental and reversible — zero-risk adoption. **[GATE: bridge command
   surface.]**
3. **Replace.** The core provider grows native hermetic provisioning
   (toolchains, C libraries) into the hangar. When it covers a project's graph,
   Nix quietly stops being needed on that machine. No flag day, ever.

## The hangar — where Jet beats the derivation

One content-addressed, signed, air-gappable store (ratified). The difference is
what a build step *is*: not an opaque derivation built from string-spliced
bash, but a typed Jet function whose authority is in its signature.

- **Hermetic = effect-checked.** A pure recipe (`#()` or locked-input effects
  per D-BUILDPOLICY1) is reproducible by construction. Nix gets purity from a
  sandbox; Jet proves it in the type system and keeps the sandbox as
  belt-and-suspenders.
- **Impurity is declared, granted, and locked.** An impure step names its
  reason, the grant is recorded in `.jet/lock`, and `jet audit` lists every
  one with its call path. This is the same machinery as D-EFFBUDGET1 — one
  effect vocabulary audits app code, build code, env activation, and OS
  activation. No other ecosystem has one auditable authority story across all
  four.
- **Errors are Jet diagnostics.** A broken recipe fails with what/why/fix
  pointing at typed fields — never a 200-line evaluator trace.
- **Remote cache = hangar mirror.** CI pushes, laptops pull, signatures verify
  (cachix, built in). GC is generation-rooted and reports what it freed — and
  never litters `/tmp`.

## Domain walkthroughs — suit every need

**Web.** `presets.web` = wasm/js targets, browsers for e2e, DB services.
`jet dev` is the hot-reload loop; `image.oci` builds a distroless container
from the same manifest (`jet image oci` — dockerTools, typed). Deploy the
image or `jet push` a host; same language end to end. **[GATE: image.oci
namespace.]**

**Game dev.** `presets.game` = SDL/Vulkan/shader toolchains; asset pipeline is
`fn build(b)` (texture packing, shader compilation, enum-from-assets); @persist
hot reload keeps the play session across code edits; studio-private console
SDKs are just private sources with the same typed refs. Reproducible builds
mean a build from three years ago still ships a patch.

**Embedded/systems.** Cross-compilation is a typed field, not arcana:
`firmware: executable { target: thumbv7em }`. Toolchains pin into the hangar;
flash/probe is a service; the whole firmware image is lockfile-reproducible
for certification. Effect budgets + signed store + SBOM = the audit story
regulators actually ask for.

**Enterprise.** `effects: { deny: [Exec, Env] }` across the dependency graph
(D-EFFBUDGET1), signed hangar, `jet why openssl` provenance, generations
on every machine. Supply-chain review becomes CI.

**Data/ML.** CUDA/BLAS provisioning through providers (nixpkgs today, core
later); env pinning makes "works on my GPU box" a lockfile fact.

**General/scripting.** Rung 0 + `jet run github@owner/tool` ephemeral runs
(`nix run`, typed). The fastest path from "saw a tool" to "ran it, nothing
installed".

## The CLI — one command, one verb per intent

Users type `jet`. Package verbs dispatch to the jetpack engine, machine verbs
to the jetos engine — by external binary dispatch, so the compiler never
links either.

```
jet run / dev / build / test        # language + lifecycle fns
jet add textkit                     # edits pack.jet, resolves, locks
jet env                             # shell from env.* modules
jet services up|down|logs
jet run nixpkgs@fastfetch           # ephemeral tool run (ref, not a path)
jet why openssl                     # provenance: who pulled this in
jet gc / verify                     # store hygiene, with reports
jet bridge flake                    # emit flake.nix shim for Nix consumers
jet switch / rollback / generations # need a system.* module in scope
jet push prod                       # needs fleet.prod
```

## Why this beats Nix — ten theses

1. **One language** for app, build, env, and OS — typed, compiled, one LSP,
   one formatter, one diagnostics voice.
2. **Options autocomplete.** `services.` completes with types and docs.
   Discovery-by-grepping-nixpkgs dies.
3. **what/why/fix errors**, never evaluator traces.
4. **Effects beat sandboxes.** Purity proven in the type system; impurity
   declared, granted, locked, auditable — one vocabulary from app to OS.
5. **Progressive disclosure.** Script → package → env → machine → fleet, one
   rung at a time, nothing relearned, nothing invalidated.
6. **Services and process supervision in core** — devenv parity without the
   bolt-ons.
7. **Secrets first-class**: encrypted at rest, effect-gated at use, never in
   the store. **[GATE: secrets design.]**
8. **The bridge swings both ways** — consume flakes day one, export shims,
   migrate incrementally, zero-risk adoption.
9. **Tidy by default.** `.jet/` only; one hangar; no result symlinks; GC that
   reports; no `/tmp` litter.
10. **Fast eval.** Comptime-cached typed evaluation, manifest-only remote
    probes (U9) — no lazy-eval cliffs, no accidental nixpkgs clones.

## Open gates (ballot before any implementation)

| Gate | Question |
|---|---|
| U11 inline script deps | `use textkit#1.4` in rung-0 scripts: syntax + lock placement |
| U12 services schema | `Service` type surface: fields, health, supervision contract |
| U13 secrets | `secret(...)` source, encryption at rest, effect gating |
| U14 image namespace | `image.oci` / `image.iso` fields and build contract |
| U15 fleet namespace | `fleet.*` host maps, push/rollback semantics |
| U16 bridge | `jet bridge flake` + foreign devshell consumption scope |
| U17 jetos name | "jetos" is still a working title (naming ledger) |
| U18 two-names rule | One reserved file (`pack.jet`, module namespaces carry roles) + one command (`jet`, external dispatch to jetpack/jetos engines). Revises U10, D-WORKSPACE2, D-PM4. |

Everything else in this vision rides already-ratified machinery: U1–U10,
D-PM1–8, D-BUILDENTRY1/D-BUILDPOLICY1 (open, recs updated), D-EFFBUDGET1
(open), lifecycle verbs (proposal §36A), @persist (D-PERSIST1, open).
