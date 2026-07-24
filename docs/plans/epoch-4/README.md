# Epoch 4 — jetpack tracking file

**Status:** active Jetpack plan. The 2026-07-09 production audit found the
existing implementation useful but below Nix package-manager parity; schema,
fixture, and policy-model completion no longer count as shipped capability.
This folder owns the package-manager/environment substrate that JetOS later
consumes.

The executable master plan is
[`world-class-package-manager.md`](world-class-package-manager.md). It defines
the Nix parity matrix, best cross-ecosystem transplants, ordered card program,
owner ballots, and binding live/hostile acceptance lanes. It supersedes stale
sequencing or “later protocol” claims below while preserving ratified law.

Durable tracking remains in four files:

- [`README.md`](README.md) — current canon and navigation.
- [`vision.md`](vision.md) — product / UX target.
- [`truth-matrix.md`](truth-matrix.md) — acceptance truth.
- [`world-class-package-manager.md`](world-class-package-manager.md) —
  executable master plan.

Older split plans and world-domination follow-up slices were implemented or
folded into Tower's active replacement program (#395, #421-#434).

---

## Current Canon

The latest decisions override stale prose in older Epoch 4 notes:

- **The reserved package file is `pkg.jet`, not `pack.jet`.**
  `D-JPK-FILENAME2=B` keeps the shipped `pkg.jet` name and amends
  D-JPK-TWONAMES1's reserved-file text. Do not rename fixtures or docs back to
  `pack.jet`.
- **Role namespaces live in module declaration names.**
  `D-JPK-MODBODY1=A`: active Jetpack work writes `module env.dev { ... }`
  and `module image.server { ... }` for OCI images. `system.*`, disk images,
  OS generations, and activation commands are frozen jetos research by
  D-JETOS-FREEZE1, not current Epoch 4 build scope. The shipped contribution form
  `module dev { env.dev: Env.{ ... } }` becomes teaching syntax, not a second
  canonical form.
- **Reserved filenames are `pkg.jet`, `env.jet`, and `workspace.jet`.**
  `pkg.jet` owns package identity and publishable package metadata. `env.jet`
  is the dev-shell role file; `workspace.jet` is the monorepo index carrying
  `module workspace { ... }`. Other role modules may live in any discovered
  `.jet` file; their role is declared by module name (`module env.dev { ... }`,
  `module image.server { ... }`). There are no required `config.jet`,
  `build.jet`, or `fleet.jet` filenames.
- **Users type `jet`; engines are separate executables.**
  `D-JPK-DISPATCH1=B`: Jetpack / jetos verbs must cross a git-style process
  boundary (`jetpack`, `jetos`, or future engine binary), with exit-code,
  `--json`, diagnostics, and version-skew contracts. Do not pile the historical
  package-gate surfaces onto the old in-process `jet::Jetpack::run` path.
- **The OS product name is `jetos`.**
  `D-JPK-OSNAME1=A`; trademark sweep remains pre-release work.

`pkg.jet` still owns package identity and publishable package metadata. Dev
environments and workspace membership have their reserved role files
(`env.jet`, `workspace.jet`); images and other role modules are discovered by
declaration. That preserves the package/env/workspace separation while deleting
unratified role filenames.

---

## Ratified Gates

| Gate | Decision | Outcome | Implementation meaning |
|---|---|---|---|
| U11 | `D-JPK-SCRIPTDEP1` | A | `use pkg#ver` inside a bare script; `jet run` resolves and locks by file hash; `jet store lock <file>` writes a sidecar; `jet init` lifts deps into `pkg.jet`. |
| U12 | `D-JPK-SERVICE1` | A | `services:` in `env.*`; jetpack supervises project-local processes; `jet services up/down/health/logs`; `jet dev` health-gates before running code. |
| U13 | `D-JPK-SECRET1` | A | `secret("name")`; encrypted repo file; activation-time memory-only decrypt; reads require `Secret` effect; no plaintext in hangar. |
| U13a | `D-JPK-SECRETCRYPTO1` | A | Use a vetted crypto bridge for age-style encryption; compiler stays zero-external-crate. |
| U14 | `D-JPK-IMAGE1` | A | `image.*` can build `.Oci` containers. `.Iso` installers are frozen jetos research. OCI layout is direct from hangar objects. |
| U14a | `D-JPK-OCITOOL1` | C | Native/std-only deterministic OCI layout now; registry push gated on TLS, with temporary `skopeo` bridge allowed only as staging. |
| U15 | `D-JPK-FLEET1` | A | Fleet host maps remain research/capture only; rollout waits for Epoch 7 jetos ballots. |
| U16 | `D-JPK-BRIDGE1` | A | `jet env -p`, foreign `flake.nix`/`devenv.nix` consumption, `jet run tool@nixpkgs`, `jet bridge flake`. |
| U17 | `D-JPK-OSNAME1` | A | Spell the OS `jetos`. |
| U18 | `D-JPK-TWONAMES1` + follow-ups | amended | Reserved files are `pkg.jet`, `env.jet`, and `workspace.jet`; role modules are shaped by declaration; engines dispatched as executables. |
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
  source merge, and diagnostics around E0960-E0983. D-JETOS-FREEZE1 makes the
  `SystemPlan`/whole-machine pieces inert research until Epoch 7; do not treat
  them as shipped OS syntax.
- Providers `core` and `nix` realize into the hangar; `.jet/lock` exists.
- `workspace.jet` / `module workspace { members: ... }` partially exists from
  earlier cards; the current rule is reserved filename plus role declaration,
  not either one alone.
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
  D-JPK-SCRIPTDEP1 script deps
  U14 OCI image capture / native OCI layout
  U19 env/dev split + trust gate

Phase C — env runtime
  U12 services
  D-JPK-SECRET1 / D-JPK-SECRETCRYPTO1 secrets
  U16 Nix bridge

Frozen research — jetos realization (Epoch 7)
  single-host switch/generations/rollback
  fleet push realization
  ISO / VM test harness
```

Edges:

- Phase A comes first; every later gate depends on names, module shape, and
  process dispatch.
- D-JPK-DEVCOMPOSE1 gates D-JPK-SERVICE1, D-JPK-SECRET1, and D-JPK-BRIDGE1
  because it defines what `jet env`, `jet dev`, and trust mean.
- U14 OCI parse/capture can land before registry push support; registry push
  waits on TLS. Disk images wait for Epoch 7 jetos.
- Fleet rollout and `jet push` wait for Epoch 7 jetos ballots; no jetos
  implementation is active in Epoch 4.
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

## jetos Research Appendix

Frozen by D-JETOS-FREEZE1. These notes preserve the target, but no `system.*`,
fleet rollout, disk-image, generation, or activation spelling here is current
syntax law. Future Epoch 7 ballots must re-open the exact surface. jetos should
eventually represent the current HalcyonOmega NixOS setup audited from
`/home/nate/nixos` on 2026-06-15. Required coverage:

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

## Live work

Tower owns Jetpack card sequencing, decisions, blockers, and proof state:

```sh
nix develop -c node plugins/tower/tower.mjs status
```

This directory retains only durable master plans and acceptance matrices. Per-card
plans are deleted once their work is shipped or fully represented in Tower.

## Jetpack health

Run `jetpack doctor` first when package-manager state looks wrong. It reads the
Hangar metadata and hashes each realized output, checks configured registry or
mirror endpoints, detects abandoned locks and objects unused for 30 days, and
checks that the default publishing key pair exists. It never repairs or deletes
state.

Network registries are not contacted by default. Local `file://` registries are
always checked; pass `--online` to probe HTTP(S) endpoints, or `--offline` to
force the offline-safe report. `--json` emits the same ordered checks for tools.
Healthy, degraded, and broken reports exit 0, 1, and 2. Follow each printed fix;
`jetpack clean` handles stale Hangar/cache objects, while `jet registry keygen`
creates the publishing key.
