# Jetpack pack ABI — the typed authoring surface (next surface after Phase 1)

**Status:** design-of-record for the **fluent/typed `pack.jet`** that D-JPK3
promises *after* first-party jetpack-module support. Migrated 2026-06-15 from the
former `docs/research/functional-pack-debrief.md` (which modeled a functional
`pack.jet` against `flake.nix`). The **core-language** pieces this surface needs
(field punning, `module` declarations, contextual empty-list inference,
expression bodies, arbitrary `for … in <expr>` heads) are balloted as **Group 16
(D-FP1…6)** in docs/spec/decision-ballots.md. This file owns the **Jetpack-specific**
design: the typed ABI, ref spellings, package-ref sugar, the three use-case
categories, and dispersed-file merge rules. New owner calls are **D-JPK18,
D-JPK19** (added to README §6).

Phase 1 ships the directive form (D-JPK3 option A); this is its evolution, not a
replacement of the ratified Phase 1 surface.

## Goal

Keep Nix's best property — declarative, function-shaped configuration — while
making the authoring experience smaller, typed, and harder to misuse. A `pack.jet`
should read like domain data first, language mechanism second, and be at least as
short as the equivalent Nix dev shell.

## Three use cases (keep them conceptually separate)

They share the resolver, store, lock model, and `jetpack` module, but the files
*mean* different things. Do not let later categories leak scope into earlier ones.

| # | Category | Purpose | Boundary |
|---|---|---|---|
| 1 | **Jet project package manager** (Cargo-like) | dependencies for a project written in Jet. Uses `jet.toml`/`jet.lock` per S52/M12. `pack.jet` is optional — only a dev shell / external tools. | `jet.toml` = "what Jet code does this depend on?"; `pack.jet` = "what external tools should be in this project's shell?" |
| 2 | **Jetpack as a system package manager** (not JetOS) | packages/shells/profiles/generations/rollback on an existing Linux/macOS box (`nix profile`/`home-manager`-lite/`devenv`). | Manages packages, shells, profiles, generations. Does **not** configure bootloaders, services, users, kernel, or declarative OS state. |
| 3 | **JetOS** | declarative whole-machine config with eventual NixOS parity: options, modules, hosts, activation, ISO/VM outputs, pure eval. | Phase 2; builds on jetpack. JetOS owns OS/module semantics; jetpack owns package resolution/store/profiles. See `jetos-design.md`. |

## Typed pack ABI

Pack files should not force authors to name the same shape three times. When the
expected type is known, Jetpack interprets a plain data literal. One declaration
keyword — `module` — and everything else is an ordinary typed value.

```jet
module root {
    sources: {
        default: github@NixOS/nixpkgs/nixos-24.05,
        unstable: github@NixOS/nixpkgs/nixpkgs-unstable,
    },
    shells: {
        dev: Shell {
            packages: [
                default.[ripgrep, fd, jq],
                unstable.neovim,
                "mine@hello",            // string escape hatch for uncovered refs
            ],
            env: [ env("RUST_LOG", "debug"), secret("DATABASE_URL") ],
            prompt: "jetpack",
        },
    },
}
```

Conceptual lowered form (ordinary Jet, what the sugar desugars to):

```jet
import jetpack as jp;
pub pure fn module() -> jp.PackModule {
    return jp.PackModule {
        sources: [ jp.source("default", "github:NixOS/nixpkgs/nixos-24.05"), … ],
        shells: { "dev": jp.Shell { packages: [ jp.pkg("default@ripgrep"), … ] } },
    };
}
```

ABI types (first-party `jetpack` module, **ordinary types, not keywords**):
`PackModule`, `Shell`, `Source`, `PackageRef`, `Env`, `System`, `Profile`,
`Image`, and later JetOS `Module`/`Option`/`Service`. Authoring vocabulary:

```text
module root { … }     # root aggregate (root pack.jet)
module vscode { … }   # imported contribution
Shell { … }  Profile { … }  System { … }  Image { … }
```

Rules: a `module name { … }` body has expected type `PackModule`; a `dev: Shell {
… }` field gives its body expected type `Shell`; `packages:` gives its list item
type `PackageRef`; declarations in `pack.jet` are implicitly public and lower to
pure exported fragments. The package/source shorthand applies **only** inside
fields whose expected types are Jetpack types — the `module` declaration itself is
core Jet (Group 16 D-FP3); LSP parses one shape everywhere with no DSL injection.

## D-JPK18 — Source ref spelling *(open)*

Source refs should be data, not function calls. Provider on the left, the
provider-specific path on the right.

| Option | Example | Verdict |
|---|---|---|
| **At ref** | `github@NixOS/nixpkgs/nixos-24.05` | **Recommended** — avoids `:` collision; reads "provider-at-target"; clean in typed `sources:` maps |
| Colon URI | `github:NixOS/nixpkgs/nixos-24.05` | Reject for authoring (`:` is busy; package refs already moved off colon); keep as string compatibility |
| String only | `"github:NixOS/nixpkgs/nixos-24.05"` | Keep as compatibility/escape hatch |
| Function call | `github("NixOS/nixpkgs", "nixos-24.05")` | Internal/lowered examples only — too noisy |
| Arrow / `::` | `github -> …` / `github::…` | Reject (punctuation collisions with arms/paths) |

**D-JPK18:** ratify `provider@target` as the authoring spelling; keep colon
strings as a compatibility parser only. Note this **revises** the Phase-1 ratified
`<source>:<package/path>` ref form (D-JPK7/D-JPK15) for the *next* surface — keep
the colon classifier for Phase 1 compatibility.

## D-JPK19 — Package-ref sugar in typed package lists *(open)*

Inside fields whose expected item type is `PackageRef` (Jetpack context only):

| Option | Example | Verdict |
|---|---|---|
| Dot ref | `default.ripgrep` | Best single-package spelling (≈ `pkgs.ripgrep`) |
| Scoped list | `default.[ripgrep, fd]` | Best multi-package shorthand (≈ `with pkgs; [ ripgrep fd ]`); **owner-preferred next syntax** |
| Default-source bare | `[ripgrep, fd, unstable.neovim]` | Shortest; **must be limited to package-list context** |
| String ref | `"default@ripgrep"` | Compatibility/escape hatch |
| Function call | `pkg("default", "ripgrep")` | Generated/internal only |

**D-JPK19:** ratify dot refs + `source.[a, b]` scoped lists as Jetpack-context,
type-directed sugar; do **not** generalize `x.[…]` to all Jet expressions, and do
**not** make bare package names normal Jet identifiers (extends D-JPK17's inline
named-source refs). `default` is the fallback source for unqualified refs.

## Dispersed pack files + merge rules

Flake-parts-style composition without one giant file. A fragment is
self-contained (its own sources, packages, shells, settings, later JetOS
options); the root imports the tree and merges typed contributions.

```jet
module root {
    sources: { default: github@NixOS/nixpkgs/nixos-24.05 },
    imports: tree(["./apps", "./shells"]),
}
```

```jet
// apps/vscode.jet — self-contained; added without editing root pack.jet
module vscode {
    sources: { vscode-source: github@microsoft/vscode/main },
    packages: [ vscode-source.vscode ],
    settings: { "editor.formatOnSave": true },
}
```

Merge is **typed, not text concatenation**:

| Field | Merge rule |
|---|---|
| `sources` | merge by key; duplicate names with different refs conflict unless explicitly overridden |
| `packages` | concatenate, de-duplicate, preserve source identity |
| `shells` | merge by key; package lists combine; scalar fields conflict unless priority-marked |
| `profiles` | merge by key; package lists combine |
| `systems` / `images` | merge by key; scalar conflicts are diagnostics unless overridden |
| scalar settings | one value wins only by explicit priority (`default`/`force`, JetOS-style layers) |

This gives Jetpack the flake-parts/import-tree model and JetOS a direct
foundation (a JetOS feature file is the same shape with `options:`/`services:`).

## Validation Jetpack owns (better errors than Nix)

Jetpack owns package refs, so it validates at the authoring boundary instead of
failing deep in a derivation:

```text
error: package names cannot contain spaces
 --> pack.jet:12:20
fix: did you mean `ripgrep`?
```

Also: unknown source names, malformed refs, unsupported platforms, and unknown
`Shell`/`System` fields become local diagnostics.

## Boundaries vs. core language (do not over-pull)

Per the debrief's documentation audit — keep these in the right home:

- **Field punning, `module` decl, empty-list inference, expression bodies,
  `for … in <expr>` heads, list spread** → core, balloted as **Group 16 (D-FP1…6)**.
- **Pure pack evaluation** → S60 + **E2-M16** (`m16-pure-eval-layer3.md`); Phase 2.
- **Typed ABI, ref spellings, package sugar, merge rules, categories** → this file
  (Jetpack track), D-JPK18/19.
- **JetOS option/module syntax** (`option`/`default`/`force`) → `jetos-design.md`,
  Phase 2; do not pull into Jetpack Phase 1 package files.

## Preferred minimal path

1. Add typed Jetpack ABI types (`Shell`, `Source`, `PackageRef`).
2. Add Jetpack-context package refs in `Shell.packages` (D-JPK19).
3. Add core `module name { … }` declarations (D-FP3) lowering to typed pure
   fragments, visible to LSP.
4. Add field punning (D-FP1) for all struct literals.
5. Harden collection-transform inference (D-FP4) so package lists can be built
   functionally without dropping to loops.
