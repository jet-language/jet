# Ad-hoc repo/binary adapters for jetpack

**Status:** proposal, 2026-07-02. Owner decision needed before implementation:
**D-JPK-ADAPTER1**. This extends the Epoch 5 jetpack vision for refs that have
neither `pack.jet` nor a usable Nix flake.

## Problem

Today jetpack has two clean paths:

- a source with `pack.jet` realizes through the first-party core provider;
- a source with Nix packaging realizes through the Nix bridge.

The missing path is the normal internet: a GitHub repo with only a `Makefile`, a
release tarball with a binary, a local executable, a Go/Cargo/npm project with no
flake, or a private tool that will never publish Jet metadata. Jetpack should not
make the user fork the repo, write a flake, or wait for nixpkgs. The user should
be able to define the last mile at the call site, lock it, and use it immediately.

## Recommendation

Add **adapter packages**: a typed Jetpack recipe that turns an already fetched
source or binary into a normal `Pkg`.

Adapters are **not providers**. Providers answer "where do the bytes come from?"
Adapters answer "how do these bytes become a package output?" That keeps U9's
provider inference intact and avoids a `via:` marker:

```
ref -> source provider fetches bytes -> adapter realizes output -> hangar -> env/run
```

If the fetched tree has `pack.jet`, use it. If it has `flake.nix`, the Nix bridge
may realize it. If neither exists, jetpack asks for or generates an adapter.

## Proposed Surface

An adapter is a `Pkg` value, so it can live anywhere packages already live: inline
in an `env.*` module for local use, or named in `pack.jet` when the project wants
to publish/share it.

```jet
module env.dev {
    packages: [
        Pkg.adapt(
            name: "taplo",
            source: github@tamasfe/taplo#v0.9.3,
            recipe: Recipe.cargo(package: "taplo-cli", bins: ["taplo"]),
        ),
    ]
}
```

Prebuilt binary archive:

```jet
module env.dev {
    packages: [
        Pkg.adapt(
            name: "toolx",
            source: fetch(
                "https://example.com/toolx-1.2.0-linux-x64.tar.gz",
                sha256: "4f8b...",
            ),
            recipe: Recipe.prebuilt(
                strip_prefix: "toolx-1.2.0",
                bins: { toolx: "bin/toolx" },
            ),
            platforms: [Platform.linux_x64],
        ),
    ]
}
```

Expert build recipe:

```jet
module env.dev {
    packages: [
        Pkg.adapt(
            name: "weirdctl",
            source: github@acme/weirdctl#8a31c9d,
            deps: [default.cmake, default.ninja],
            recipe: Recipe.build(fn(b: BuildContext) {
                b.exec(cmake, ["-S", ".", "-B", "build"])
                b.exec(ninja, ["-C", "build"])
                b.install_bin("build/weirdctl", as: "weirdctl")
            }),
        ),
    ]
}
```

Exact constructor names are ballot surface. The durable model is the important
part: adapter = `source + recipe + declared outputs + declared authority`.

## On-the-fly flow

`jet run <ref>` should become useful even when the upstream repo has no metadata:

```
$ jet run github@acme/weirdctl#8a31c9d
Error [E12xx]: `github@acme/weirdctl#8a31c9d` has no Jet package or Nix flake
 Why: jetpack needs package metadata or an adapter recipe before it can run a repo
 Fix: run `jet add github@acme/weirdctl#8a31c9d --adapt` to create one
```

`jet add <ref> --adapt` probes without executing code, drafts a recipe, and asks
the user to save it:

```
$ jet add github@acme/weirdctl#8a31c9d --adapt
  detected: CMake project, binary candidate `weirdctl`
  recipe:   cmake + ninja, install `build/weirdctl`
  writes:   module env.dev { packages: [Pkg.adapt(...)] }
```

For one-shot use, `jet run <ref> --adapt cargo --bin tool` can store the adapter
under `.jet/cache/adapters/<hash>/` and lock it. `jet add --save` later lifts the
same adapter into `pack.jet` or the chosen module. Non-interactive CI must pass
the recipe explicitly; no prompts.

Autodetect drafts, never executes. First build still goes through the U19 trust
gate and the build-policy authority checks.

## Recipe Families

Ship a small set first, all backed by the same adapter IR:

| Recipe | Use |
|---|---|
| `Recipe.prebuilt` | downloaded archive or repo-contained binary; expose files under `bin/` |
| `Recipe.copy` | local executable/script path, content-hashed into the hangar |
| `Recipe.cargo` | Cargo repo without a flake; declared package/bin |
| `Recipe.go` | Go module; `go build` output path |
| `Recipe.node` | npm/pnpm/bun tool with a declared executable |
| `Recipe.cmake` / `Recipe.make` | common native builds, tool deps explicit |
| `Recipe.build(fn(BuildContext))` | expert escape for everything else |

The curated recipes are sugar over `BuildContext`; they do not create a second
build mechanism.

## Reproducibility And Safety

- Lock identity includes source rev/tree hash, adapter text hash, recipe helper
  version, target platform, declared build-tool packages, effects/grants, and
  output hash.
- Network during build is denied unless it is a locked `fetch(url, sha256:)`.
- Ambient commands run only through `BuildContext`, with effect provenance in
  `.jet/lock`.
- Generated outputs install only under the package output root; PATH exposure is
  explicit through declared bins.
- A recipe cannot silently read host `/usr/bin` tools. Build tools are `Pkg`
  deps, using nixpkgs as the current stopgap and core provider later.
- Adapter probes are read-only: file names, package manifests, and release
  metadata only. No upstream script runs during probe.

## Diagnostics

Implementation should add E12xx diagnostics in the package/jetpack family:

- no usable package metadata: ref has no `pack.jet`, no flake, no saved adapter;
- ambiguous adapter autodetect: multiple plausible binaries/build systems;
- adapter output missing: declared bin was not produced;
- unpinned binary URL: `Recipe.prebuilt` needs a hash;
- undeclared build tool: recipe calls a tool not listed in `deps`;
- ambient network/exec denied: point at `BuildContext` authority/grants.

Every fix should name the next command or field: `jet add <ref> --adapt`,
`bins: { name: "path" }`, `sha256: "..."`, or `deps: [default.cmake]`.

## Implementation Plan

1. Add adapter IR and lock schema: `source`, `recipe`, `platforms`, `deps`,
   `bins`, `effects`, output hash.
2. Implement `Recipe.prebuilt` and `Recipe.copy` first; they prove binary use
   without a compiler toolchain.
3. Add curated build recipes (`cargo`, `go`, `node`, `cmake`, `make`) as thin
   `BuildContext` wrappers.
4. Add expert `Recipe.build(fn(BuildContext))` once D-BUILDPOLICY1's authority
   machinery is in place.
5. Wire `jet add --adapt` and `jet run --adapt`; make autodetect draft-only and
   offline-testable with fixtures.
6. Add `jet adapter explain <ref>` or equivalent debug output showing why a ref
   chose pack/flakes/adapter and what was locked.

## Ballot Shape

**D-JPK-ADAPTER1 - How should jetpack define and run non-pack/non-flake refs?**

- **Option A - adapter packages as `Pkg` values (recommended).** One package
  mechanism; adapters are recipes, not providers; inline or named use the same
  IR and lock path.
- **Option B - new provider kind.** `adapter@...` or `via: adapter` owns both
  fetching and realizing. Rejected by U9 pressure: provider markers leak policy
  back into refs.
- **Option C - require upstream metadata.** Users must add `pack.jet` or a flake
  to every repo. Rejected: kills the "use anything now" goal.

Recommendation: **A**. It gives Jetpack the missing escape hatch without weakening
the provider model, reproducibility contract, or one-mechanism rule.
