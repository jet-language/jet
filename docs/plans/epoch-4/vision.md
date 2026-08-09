# Epoch 4 Vision — one language from script to datacenter

**Status:** product target, refreshed 2026-07-02. This document says what the
experience should feel like. [`world-class-package-manager.md`](world-class-package-manager.md)
defines binding implementation and acceptance.

## The Sentence

Everything your software needs — code, dependencies, tools, services, secrets,
images, machines, and fleets — is declared in Jet, typed, checked, locked, and
reproducible, with one command per intent.

Nix proved the outcomes people want: reproducible environments, declarative
machines, rollback, one deduplicated store. Jet keeps those outcomes and removes
the second untyped language, evaluator traces, service glue, and file-name
sprawl.

## Two Names In Practice

Users learn:

- **`package.jet`** — the only reserved package filename.
- **`jet`** — the only command they type.

Everything else is a role module:

```jet
// package.jet
name: "pulseops"
version: "0.4.0"

packages: {
    pulseops: executable { entry: "src/run.jet" }
}

deps: {
    textkit: "1.4.2"
}
```

```jet
// toolchain.jet, dev.jet, ops/laptop.jet, or any other .jet file
module env.dev {
    packages: [default.[postgres_16, ripgrep], unstable.zig]
    services: {
        postgres: Service.{ version: 16, port: 5432, init: "schema.sql" }
    }
    secrets: { STRIPE_KEY: secret("stripe-dev") }
}

module system.laptop {
    packages: [default.[firefox, ghostty]]
    services: { tailscale: Service.{}, pipewire: Service.{} }
}

module image.server {
    kind: .Oci
    from: packages.pulseops
    expose: [8080]
}

module fleet.prod {
    hosts: {
        web1: system.web.{ region: "us-east" }
        web2: system.web.{ region: "eu-west" }
    }
}
```

The filename is organization. The declaration is meaning.

## The Ladder

| Rung | User writes | Command |
|---|---|---|
| 0 script | one `.jet` file, optionally `use pkg#ver` | `jet run app.jet` |
| 1 package | `package.jet` with bare `name`/`version`, `deps`, `packages` | `jet build`, `jet add`, `jet registry publish` |
| 2 environment | `module env.dev { ... }` in any `.jet` file | `jet env`, `jet dev`, `jet services` |
| 3 machine | `module system.laptop { ... }` | `jet switch`, `jet store rollback`, `jet store generations` |
| 4 fleet | `module fleet.prod { ... }` | `jet push prod` |

Climbing a rung adds a module namespace, not a new configuration language.

## Script Dependencies

```jet
// stats.jet
use textkit#1.4

fn run() {
    print(textkit.wrap(input(), width: 72))
}
```

`jet run stats.jet` resolves, locks by file-content hash, and runs. `jet store lock
stats.jet` writes a sidecar for committed reproducibility. `jet init` lifts the
inline deps into a generated `package.jet`. Rung 0 stays manifest-free.

## Env vs Dev

Entering an environment and running project code are different intents.

- `jet env [name]` realizes the base env plus optional overlay and opens a
  shell. It never runs project functions.
- `jet dev` realizes `env(base + env.dev)`, waits for services, then runs
  `fn dev()` with fallback to `fn run()`.
- `jet test` and `jet build` use `env.test` / `env.build` overlays the same way.

First entry to a repo with hooks, services, sources, or secrets shows a trust
summary. The grant is keyed by environment-definition hash and re-prompts when
that hash changes. `jet env --trust` is the deliberate one-shot bypass for CI
or already-trusted repos.

## Services

Services are typed values in env modules, supervised by jetpack:

```jet
module env.dev {
    services: {
        postgres: Service.{ version: 16, port: 5432, init: "schema.sql" }
        redis: Service.{}
    }
}
```

`jet dev` starts them under `.jet/services/`, health-gates app startup, captures
logs, and cleans up on `jet services down`. System services in `system.*` share
the type vocabulary but have a jetos lifecycle.

## Secrets

```jet
module env.dev {
    secrets: {
        STRIPE_KEY: secret("stripe-dev")
        DB_PASS: secret("db-dev")
    }
}

fn charge() =[Net, Secret]=> Receipt ? Error {
    key :: secrets.get("stripe-dev")?
    ...
}
```

Secrets are encrypted at rest in the repo, decrypted only into activation
memory, never written to the hangar or lockfile, and every read carries the
`Secret` effect. The crypto backend is an isolated vetted bridge, not a compiler
dependency.

## Images

```jet
module image.server {
    kind: .Oci
    from: packages.pulseops
    expose: [8080]
    env_vars: { RUST_LOG: "info" }
}
```

`jet image server` builds a deterministic distroless OCI layout directly from
hangar objects. `kind: .Iso` produces a jetos installer image once the jetos
realization tier lands. Registry push is gated on a native TLS story; a `skopeo`
bridge may be used only as temporary staging.

## Nix Bridge

The bridge matches the three daily Nix flows:

| Today | Jet |
|---|---|
| `nix-shell -p nodejs ripgrep` | `jet env -p nodejs ripgrep` |
| `nix develop` in a flake repo | `jet env` detects `flake.nix` / `devenv.nix` when no `env.*` exists |
| `nix run nixpkgs#fastfetch` | `jet run fastfetch@nixpkgs` |

`jet bridge flake` emits a generated `flake.nix` shim so Nix users can consume
Jet packages and envs. Adoption is consume, coexist, replace; no flag day.

## jetos

jetos is NixOS restated in Jet: the whole machine is one package-like closure
built by jetpack, stored in the hangar, activated atomically, and rolled back by
generation.

```jet
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
$ jet switch --name "pre-gpu-driver"
$ jet store generations
$ jet store rollback
```

The module system must support typed options, defaults, `force`, final-value
reads, cycle diagnostics, deterministic list/map merges, scalar conflicts, host
variants, installer ISO, VM tests, and one module touching both system and user
scope.

## Ad-Hoc Adapters

Some useful things have no `package.jet`, no flake, and no nixpkgs equivalent. The
escape hatch (ratified, D-JPK-ADAPTER1=A) is an adapter package:

```jet
module env.dev {
    packages: [
        Pkg.adapt(
            name: "weirdctl",
            source: "acme/weirdctl#8a31c9d@github",
            deps: [default.cmake, default.ninja],
            recipe: Recipe.build(steps: [
                .exec(tool: "cmake", args: ["-S", ".", "-B", "build"]),
                .exec(tool: "ninja", args: ["-C", "build"]),
                .install(src: "build/weirdctl", dest: "bin/weirdctl"),
            ]),
        ),
    ]
}
```

Adapters are recipes over fetched bytes. They do not become a provider kind and
they do not weaken provider inference.

## Why This Beats Nix

1. One language for app, build, env, image, machine, and fleet.
2. Typed options and completions instead of grepping package sets.
3. what/why/fix diagnostics instead of evaluator traces.
4. Effects and grants provide one audit vocabulary across app code, build code,
   env activation, and OS activation.
5. Progressive disclosure: script to package to env to machine to fleet.
6. Services and process supervision are first-class.
7. Secrets are encrypted, effect-gated, and store-free.
8. Nix interop works on day one and can be replaced incrementally.
9. `.jet/` contains generated project state; the hangar contains realized
   artifacts; no `result` symlink or `/tmp` litter — a golden-tested guarantee
   with auto-GC and honest `jet hangar du` (U22).
10. Deterministic module discovery and locking make old builds reproducible.
11. Any vendor, latest release: adapters for no-metadata refs (U20) plus
    channel refs and `jet update` (U21) — no forking, no waiting on nixpkgs.
12. No daemon, no root (U28); realized-once-works-offline-forever (U29);
    Windows, macOS, and Linux all tier-1 (U25).
13. Discovery lives in the terminal and the editor (U26); failed builds hand
    over the crime scene, not a log wall (U27); binary-cache substitution is
    designed in from the start (U24).
