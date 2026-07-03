# Epoch 4 — jetpack / jetos tracking file

**Status:** active plan, refreshed 2026-07-02 from Tower card
`c9jetpackgates` and today's follow-up decisions. This folder now has three
tracked files:

- [`README.md`](README.md) — current decisions, status, and sequencing.
- [`vision.md`](vision.md) — product / UX target.
- [`implementation.md`](implementation.md) — executable agent plan.

Older split files were folded into these three: `unified-ecosystem.md`,
`jetos-design.md`, `IMPLEMENTATION-STATUS.md`,
`payload-env-separation.md`, and `ad-hoc-adapters.md`.

---

## Current Canon

The latest decisions override stale prose in older Epoch 4 notes:

- **The reserved package file is `pkg.jet`, not `pack.jet`.**
  `D-JPK-FILENAME2=B` keeps the shipped `pkg.jet` name and amends the U18
  two-names text. Do not rename fixtures or docs back to `pack.jet`.
- **Role namespaces live in module declaration names.**
  `D-JPK-MODBODY1=A`: write `module env.dev { packages: [...] }`,
  `module system.laptop { ... }`, `module image.server { ... }`,
  `module fleet.prod { ... }`. The shipped contribution form
  `module dev { env.dev: Env.{ ... } }` becomes teaching syntax, not a second
  canonical form.
- **Only `pkg.jet` is a reserved filename.**
  Role modules may live in any `.jet` file the user chooses and are discovered
  by declaration via `find()`, never by required filenames like `env.jet`,
  `workspace.jet`, `config.jet`, `build.jet`, or `fleet.jet`.
- **Users type `jet`; engines are separate executables.**
  `D-JPK-DISPATCH1=B`: Jetpack / jetos verbs must cross a git-style process
  boundary (`jetpack`, `jetos`, or future engine binary), with exit-code,
  `--json`, diagnostics, and version-skew contracts. Do not pile U11-U19 onto
  the old in-process `jet::Jetpack::run` path.
- **The OS product name is `jetos`.**
  `D-JPK-OSNAME1=A`; trademark sweep remains pre-release work.

`pkg.jet` still owns package identity and publishable package metadata. Dev
environments, machines, images, fleets, services, and workspace membership are
role modules. That preserves the old package/env separation while deleting
required role filenames.

---

## Ratified Gates

| Gate | Decision | Outcome | Implementation meaning |
|---|---|---|---|
| U11 | `D-JPK-SCRIPTDEP1` | A | `use pkg#ver` inside a bare script; `jet run` resolves and locks by file hash; `jet lock <file>` writes a sidecar; `jet init` lifts deps into `pkg.jet`. |
| U12 | `D-JPK-SERVICE1` | A | `services:` in `env.*`; jetpack supervises project-local processes; `jet services up/down/health/logs`; `jet dev` health-gates before running code. |
| U13 | `D-JPK-SECRET1` | A | `secret("name")`; encrypted repo file; activation-time memory-only decrypt; reads require `Secret` effect; no plaintext in hangar. |
| U13a | `D-JPK-SECRETCRYPTO1` | A | Use a vetted crypto bridge for age-style encryption; compiler stays zero-external-crate. |
| U14 | `D-JPK-IMAGE1` | A | `image.*` can build `.Oci` containers and `.Iso` installers. OCI layout is direct from hangar objects. |
| U14a | `D-JPK-OCITOOL1` | C | Native/std-only deterministic OCI layout now; registry push gated on TLS, with temporary `skopeo` bridge allowed only as staging. |
| U15 | `D-JPK-FLEET1` | A | `fleet.*` host maps and `jet push`; parse/capture now, realization waits for single-host jetos. |
| U16 | `D-JPK-BRIDGE1` | A | `jet env -p`, foreign `flake.nix`/`devenv.nix` consumption, `jet run nixpkgs@tool`, `jet bridge flake`. |
| U17 | `D-JPK-OSNAME1` | A | Spell the OS `jetos`. |
| U18 | `D-JPK-TWONAMES1` + follow-ups | amended | One reserved file (`pkg.jet`) and one user command (`jet`); role modules discovered by declaration; engines dispatched as executables. |
| U19 | `D-JPK-DEVCOMPOSE1` | D | `jet env [name]` enters a tools-only shell and never runs project functions; `jet dev` explicitly runs `fn dev()` inside `env(base + env.dev)`. |
| U20 | `D-JPK-ADAPTER1` | A | Ad-hoc adapters: `Pkg.adapt(source:, recipe:)` turns fetched bytes into packages for refs with no `pkg.jet`/flake/nixpkgs path; `jet add <ref> --adapt` drafts from read-only probes; curated recipes over one `Recipe.build(fn(BuildContext))`; constructor names are follow-up ballot surface. |
| U21 | `D-JPK-CHANNEL1` | A | Channel refs (`#latest`, `#v0.x`, `#main`) resolve only in `jet update` / first `add`; lock stays exact; `jet outdated` read-only; unlocked channel ref in CI is an error. |
| U22 | `D-JPK-GC1` | B (amended 2026-07-03) | Hangar disk contract: auto-GC ages out unreferenced objects (30d default, opportunistic, no daemon) + manual `jet clean` (GC + hangar optimize: hardlink/dedup, `nix store optimise` equivalent, one pass) + honest `jet hangar du`; lockfile/generation-reachable never collected; zero-/tmp guarantee golden-tested; build scratch hangar-scoped and crash-cleaned. |
| U23 | `D-JPK-NONIX1` | A | No-Nix machines: everything Nix-free realizes; bridge-needing packages fail with one E12xx naming them + both fixes (install Nix / `--adapt`); never holds realized packages hostage. |
| U24 | `D-JPK-CACHE1` | A | Binary cache: output-hash-addressed HTTP protocol, signed objects, hash-verified on arrival; envelope fields (output hash, platform, signature, provenance) frozen into hangar/lock schema NOW; protocol/push are later cards behind the TLS gate. |
| U25 | `D-JPK-PLATFORM1` | A | Linux + macOS + Windows all tier-1 native for jetpack (hangar/core/adapters/services/secrets/trust); per-platform CI; platform break = P1. Nix bridge stays Linux/macOS; jetos stays Linux. |
| U26 | `D-JPK-DISCOVER1` | A | Discovery: `jet search` + `jet info` on a fast local offline index (same metadata the resolver uses) + LSP completions/hover for package names and typed option fields in env modules. |
| U27 | `D-JPK-BUILDDBG1` | A | Failed builds: `--shell-on-fail` shell inside preserved scratch at the failing step (sole exception to U22 cleanup; GC-swept), `jet explain <ref>` (resolution path + locked identity), `jet logs <pkg>` persisted per-step with `--json`. |
| U28 | `D-JPK-NODAEMON1` | A | Standing constraint: no resident daemon, no root (transient sudo for jetos activation only); unprivileged sandboxing with honest fallback warning + `sandbox require`; file-lock coordination; violations require a new ballot. |
| U29 | `D-JPK-OFFLINE1` | A | Offline guarantee: realize-class verbs never touch network when lock satisfied; `--offline` makes any would-be fetch a loud error; network-class verbs refuse under it; golden test severs network and sweeps all verbs. |

---

## Shipping Baseline

Built before this refresh:

- `jet` and `jetpack` binaries exist.
- `pkg.jet` manifest parser exists for `payload:`, `deps:`, `packages:`,
  `edition`, with package diagnostics in E12xx.
- Typed module evaluation exists for the older contribution form:
  `EnvPlan`, `SystemPlan`, `ServicePlan`, `ImagePlan`, `find("./modules")`,
  source merge, and diagnostics around E0960-E0983.
- Providers `core` and `nix` realize into the hangar; `.jet/lock` exists.
- `workspace.jet` / `module workspace { members: ... }` partially exists from
  earlier cards, but U18 follow-up changes the filename rule: `workspace` is a
  role module, not a required file.
- Hyphenated names in package/module/system/image/env name positions are
  ratified and implemented.

Do not assume the shipped implementation already matches the current canon. The
first work in Epoch 4 is a reconciliation pass.

---

## Sequencing

```
Phase A — foundation
  dispatch seam + pkg.jet canon + module-declaration role form + filename cleanup

Phase B — independent surfaces
  U11 script deps
  U14 image capture / native OCI layout
  U15 fleet parse/capture
  U19 env/dev split + trust gate

Phase C — env runtime
  U12 services
  U13 secrets
  U16 Nix bridge

Phase D — jetos realization
  single-host switch/generations/rollback
  fleet push realization
  ISO / VM test harness
```

Edges:

- Phase A comes first; every later gate depends on names, module shape, and
  process dispatch.
- U19 gates U12, U13, and U16 because it defines what `jet env`, `jet dev`,
  and trust mean.
- U14 parse/capture can land before real push support; registry push waits on
  TLS.
- U15 surface can land before realization; `jet push` must give an honest gated
  message until single-host jetos exists.
- U24 envelope fields (output hash, platform, signature slot, provenance) land
  in Phase A with the hangar/lock schema — they are the reason CACHE1 was
  decided early; the protocol itself is a later card.
- U22 (disk contract) and U29 (offline guarantee) are Phase A constraints with
  golden tests; U28 (no daemon / no root) is a standing constraint CI asserts
  from Phase A onward.
- U20 adapters land with Phase B; `Recipe.prebuilt`/`copy` first (no
  BuildContext), `Recipe.build(fn)` waits for the D-BUILDPOLICY1 authority
  slice from e5's build-as-Jet card. U21 channels and U27 build debugging ride
  the same wave. U26 discovery follows once provider metadata is indexable.
- U25 platform tiers: Windows/macOS CI lanes stand up in Phase A; the Nix
  bridge is exempt (Linux/macOS by nature) and U23's diagnostic covers the gap.

---

## jetos Parity Baseline

jetos must be able to represent the current HalcyonOmega NixOS setup audited
from `/home/nate/nixos` on 2026-06-15. Required coverage:

- pinned stable/unstable nixpkgs and external inputs;
- multi-host configs, ISO host, hardware config, variants;
- KDE/Plasma, Home Manager, Stylix, Flatpak, NUR, nixcord, nix-gaming,
  CachyOS kernel, Ghostty, Vicinae, browser flakes, Spicetify;
- module tree discovery with one-character disable;
- option declarations with type/default/docs/enums, final-value reads, cycle
  diagnostics, deterministic list/map merge, and clear scalar conflicts;
- one feature module touching system and user scope;
- raw file emission, source links, generated text, force/backup semantics;
- nixpkgs stable/unstable, overlays, custom derivations, Flatpak, AppImage,
  later native Jet packages;
- graphical Calamares Plasma installer ISO and QEMU boot/install/switch/
  rollback tests.

If a future jetos design cannot express one of these, the design is incomplete
unless the owner explicitly grants an exception.

---

## Card lane (2026-07-03 prep pass)

Every e4 card has a vetted plan; ballots below are the only owner input left.
Sequence (workOrder; jetpack first per owner directive, then FFI program):

| # | Card | Plan | State |
|---|------|------|-------|
| 99 | build-from-source + ring shipping | package-build-from-source.md | deciding — D-JPK-RINGSHIP1, D-JPK-BUILDTOOL1; slices T0/T1 buildable now |
| 176 | vision gates U11–U19(+U20–29) | implementation.md | deciding — D-JPK-ADAPTNAME1 only; rest buildable |
| 90 | workspace continuation | workspace-continuation.md | ready — slices A–D, zero ballots |
| 3 | signed package cache | signed-package-cache.md | ready — zero ballots |
| 13 | package signing (Ed25519) | package-signing.md | ready, after #3 (index dep); crypto already approved (D-DEP-CRYPTO1) |
| 179 | toolchain as dependency (U30) | toolchain-as-dependency.md | ready — zero ballots |
| 85 | CAS build cache contract | cas-build-cache.md | ready — includes cache-poisoning race fix |
| 180 | FFI program frame | ffi-interop-program.md | deciding — D-FFI-PY1, D-DEP-PY1, D-JPK-EXTPROV1; Phase 0 binder seam buildable now |
| 124 | JS/npm + Swift interop (P0) | ffi-interop-program.md | deciding — D-FFI-JS1, D-FFI-SWIFT1 |
| 5 | plugin target | ../sidequests/plugin-target.md | deciding — D-PLUGIN-EXPORT1, D-PLUGIN-VERSION1; substrate buildable now |
| 2 | jetos generations | jetos-generations.md | frozen — gated on Phase A/D prereqs |
| 9 | flagship slices (Tower-in-Jet web app) | — | frozen — owner deferred to e4 end |

Cross-cuts: #13 shares one signature field with #3's index schema; #5 and
#124 share one wasmtime wrapper; D-JPK-BUILDTOOL1 underpins #85's
reproducibility contract; D-JPK-RINGSHIP1=C would ride #179's toolchain object.

## Open / Proposed

Ten open ballots (2026-07-03 prep pass), all rendered in Tower's Decide lane:
D-JPK-RINGSHIP1 · D-JPK-BUILDTOOL1 · D-JPK-ADAPTNAME1 (the adapter-spelling
follow-up predicted below) · D-FFI-PY1 · D-FFI-JS1 (amends D-NPMTYPE1's
hand-authored-stub floor — explicit) · D-FFI-SWIFT1 · D-DEP-PY1 (I6 CPython
runtime approval) · D-JPK-EXTPROV1 (npm/PyPI/SwiftPM providers) ·
D-PLUGIN-EXPORT1 · D-PLUGIN-VERSION1.
