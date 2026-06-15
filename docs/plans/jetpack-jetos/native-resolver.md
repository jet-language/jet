# Jetpack native resolver + named sources — design for ratification

**Status:** draft for owner concurrence, 2026-06-15. Supersedes the Phase-1
"orchestrate nix" posture in `README.md §3.4` where they disagree, per owner
direction below.

**Owner direction (2026-06-15):**
> Build a native resolver, but ensure the nix translation/resolver is an
> *extension of a core system* so we have a first-party system, then first-party
> nix support to leverage the existing nixpkgs repo until our ecosystem grows.
> We should build assuming we will overtake nix/nixpkgs with Jet and need to be
> better than it.
>
> For sources: we already need to name package sources, so declare them as named
> values but **use** the named values inline with the `source:package` syntax.

This doc turns that into (a) an architecture with a first-party core and Nix as
one provider behind a boundary, (b) a named-source pack-file surface, and (c) a
staged roadmap so each piece ships and is testable. It ends with the decision
rows (**D-JPK16, D-JPK17**) needed before the user-facing parts are coded.

---

## 1. Glossary (so the rest reads cleanly)

| Term | Meaning |
|---|---|
| **Resolver** | The core that turns a ref into a realized package (bytes + a `bin` dir), independent of where the bytes come from. First-party. |
| **Provider** | A pluggable backend the resolver calls to realize a ref: `core` (first-party Jet packages), `nix` (leverages nixpkgs), later others. |
| **Source** | A *named* upstream a user declares in `pack.jet`, e.g. `stable` → a pinned nixpkgs. The name is what appears before the `:` in a ref. |
| **Built-in source** | A source name that works with no declaration: `nixpkgs`, `github`, `path`. Today's behavior. |
| **Realization** | A provider's output for a ref: an output root + a `bin` dir, recorded in the Jetpack store. The unit every provider returns. |

The key reframe: **Jetpack's core owns resolution; Nix is one provider.** Today
the `nix` provider shells out to the `nix` binary. Tomorrow a richer `nix`
provider needs no installed `nix`, and a `core` provider realizes first-party Jet
packages with no Nix at all. The resolver, store, lock, env composition, and
shell never change as providers grow.

---

## 2. Architecture: core resolver + provider extensions

```
ref ──▶ resolve source name ──▶ pick provider ──▶ provider.realize(ref) ──▶ Realization
        (built-in or declared)   (core | nix | …)   (bytes + bin dir)        │
                                                                             ▼
                                                          record in Jetpack store ──▶ env ──▶ shell
```

A single trait is the seam (no new user syntax — already sanctioned by D-JPK5):

```rust
/// A backend that can realize a ref into bytes + a bin dir. First-party `core`
/// and the `nix` compatibility provider both implement this.
pub trait Provider {
    fn name(&self) -> &'static str;
    fn realize(&self, spec: &RefSpec, src: &ResolvedSource) -> Result<Realized, ProviderError>;
}
```

- **`core` provider (first-party).** Realizes Jet-native packages — a Jet package
  descriptor that says how to fetch + place files, built by Jetpack itself with
  no Nix. This is the system we grow our own ecosystem on, and the one we make
  "better than nix."
- **`nix` provider (extension).** Leverages nixpkgs. Stage 1 is today's
  `nix build --json` orchestration (works now on a NixOS box). Stage 2 removes
  the installed-`nix` requirement (see §4 R3). Either way it sits *behind* the
  same `Provider` trait, so it is explicitly an extension, not the manager.

This is exactly the owner ask: first-party core, first-party nix *support* as an
extension, ready to overtake nixpkgs as the `core` ecosystem grows.

---

## 3. Named sources in `pack.jet` (D-JPK17)

Owner shape: **declare sources as named values; use the names inline via
`source:package`.** A source name resolves to a built-in (`nixpkgs`/`github`/
`path`) or to a declared upstream + pin, and selects which provider realizes it.

### Worked example — the pack file

```jet
// pack.jet — named sources, used inline via source:package
import jetpack as pkg;

pub fn shell() -> [JSON] {
    return [
        // Declare named sources (name, upstream/pin). `via` picks the provider.
        pkg.source("stable",   "github:NixOS/nixpkgs/nixos-24.05");   // via nix (inferred)
        pkg.source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable");
        pkg.source("mine",     "github:halcyonomega/jet-pkgs", "core"); // via core (Jet pkgs)

        // Use the names inline. Same `source:package` syntax everywhere.
        pkg.packages([
            "stable:ripgrep",     // ripgrep from the 24.05 pin
            "unstable:neovim",    // neovim from unstable
            "mine:hello",         // a first-party Jet package
        ]);
        pkg.prompt("jetpack");
    ];
}
```

### Worked example — the terminal

```
$ jetpack run

  jetpack  resolving 3 packages …
           ▸ stable:ripgrep    ✓  ripgrep 14.1.0   (nix · nixos-24.05)
           ▸ unstable:neovim   ✓  neovim 0.10.2    (nix · nixpkgs-unstable)
           ▸ mine:hello        ✓  hello 0.1.0      (core · jet-pkgs)

  entering a temporary shell — type `exit` to leave, nothing is installed.
jetpack ~/work $
```

### Worked example — error (unknown source name)

```
$ jetpack run beta:neovim

  error: `beta` is not a known source
    Sources are the built-ins `nixpkgs`, `github`, `path`, or names you declare
    in pack.jet with `pkg.source(...)`. This pack declares: stable, unstable, mine.
    fix: add `pkg.source("beta", "<upstream>")`, or use one of the names above.
```

### Why this shape (recommendation)

- **One ref syntax, everywhere.** `source:package` is already ratified
  (D-JPK7/15); named sources just make the `source` token resolve through the
  pack file. Nothing new to learn at the call site.
- **CLI and pack file agree.** `jetpack run nixpkgs:fastfetch` (built-in) and
  `stable:ripgrep` (declared) are the same shape; declared names simply aren't
  available outside a project that declares them.
- **Provider is inferred, overridable.** A source's upstream implies a provider
  (`github:NixOS/nixpkgs/...` → nix; a Jet-pkgs repo → core). A later optional
  third arg (`pkg.source(name, upstream, via: "nix")`) can force it.

Rejected: a separate `packages_from("unstable", [...])` grouping (more
expressive but introduces a second way to attach a source — the owner explicitly
wants the inline `source:package` form to carry it).

---

## 4. Staged roadmap (each stage ships + is testable)

| Stage | Goal | Exit criteria |
|---|---|---|
| **R0** | Provider boundary: extract a `Provider` trait; today's nix path becomes the `nix` provider; a `core` provider stub exists | internal refactor; all current JPK tests still green; `realize` dispatches by source |
| **R1** | Named sources (D-JPK17): declare in pack.jet, use inline | golden: a pack with `stable`/`unstable`/`mine` resolves each to the right provider/pin; unknown-source diagnostic snapshot |
| **R2** ✅ | First-party `core` provider: a Jet package descriptor (`pkg.package(...)`) + native builder for fetch-and-place packages (no Nix). Source selected by `pkg.source(name, upstream, "core")`; `path:`, `github:`, and git URL upstreams supported, content-addressed into the store | **done** — `mine:hello` realizes from a local repo with no `nix` on PATH and its `bin` lands on PATH; remote git fetch is covered by provider tests |
| **R3** | `nix` provider without an installed `nix`: **tvix** (`tvix-eval` + store/substituter glue) behind the `nix` provider, isolated by a jetpack-scoped cargo feature/crate (I6 waiver per D-JPK16) | evaluate the needed nixpkgs attr + substitute from `cache.nixos.org`; the hard one, staged on its own |

R0–R2 are tractable on today's language and remove the "must install nix" wall
for *first-party* packages immediately. R3 is where "no nix installed, ever"
becomes true for nixpkgs packages, and is the genuinely large piece — it needs a
Nix-language evaluator (lazy, functional) plus a binary-cache client, because the
cache is keyed by the hash that only evaluation produces.

---

## 5. Decisions — ratified 2026-06-15

**D-JPK16 — Native-resolver posture & Nix-eval engine.** *(ratified)*
Core resolver owns realization; providers (`core`, `nix`, …) are extensions
behind one trait. For the no-installed-`nix` goal (R3), the interim engine is
**tvix** — the Rust reimplementation of Nix — used as a support shim behind the
`nix` provider until a first-party Jet translator replaces it. Natural fit since
Jet transpiles to Rust. **I6 waiver scoped to Jetpack's `nix` provider only:**
tvix + its dep tree must be isolated (separate crate or non-default cargo
feature) so the `jet` compiler stays std-only. Sequencing unchanged: ship
core-first (R0–R2), then R3. Note: `tvix-eval` evaluates Nix but does not by
itself realize/substitute packages, so R3 also needs store/substituter glue.

**D-JPK17 — Named-source pack surface.** *(ratified, owner shape)*
`pkg.source("<name>", "<upstream/pin>")` declares a named source; packages use it
inline as `<name>:<package>` (the ratified ref syntax). Built-in names
`nixpkgs`/`github`/`path` need no declaration; a one-arg `pkg.source("nixpkgs")`
sets the default for bare entries. R1 routes all named sources through the `nix`
provider; the `core` provider and an explicit `via:` override arrive with R2.

---

## 6. What is already done (Phase 1 baseline this builds on)

The shipped `jetpack` binary (refspec, provider, store, packfile, shell, cli;
33 unit + 10 e2e tests; `examples/jetpack/`; `docs/guide/07-jetpack.md`) is the
substrate. R0 refactors `provider.rs` behind the trait without changing any
user-facing behavior; everything else here is additive.
