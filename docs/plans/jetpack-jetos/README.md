# jetpack & jetos — sequencing, milestones, and parity plan

**Status:** active plan, naming current to ratified **U1–U10** (2026-06-16).

> **Design-of-record:** the authoring surface, files, namespaces, types, and
> merge rules are defined by [`unified-ecosystem.md`](unified-ecosystem.md)
> (owner-ratified). This file owns **sequencing, milestones, the provider
> roadmap, and the jetos parity baseline** — not syntax. The Phase-2 jetos
> mechanics live in [`jetos-design.md`](jetos-design.md). Ratified decisions
> (D-JPK*, U1–U10, D-OS1/7) live only in `docs/spec/syntax-decisions.md`; this
> file no longer restates them.

Ratified-naming reminder (full table in `unified-ecosystem.md` §10): package
manifest **`payload.jet`** (block `payload:`), project env **`env.jet`**, system
config **`config.jet`** (default `~/.jet/`), single lockfile **`.jet/lock`** in
the **`.jet/`** managed folder, one global store the **hangar** at
**`/etc/jet/hangar/`**, source refs **`provider@target`**, imports `use "<path>"`.

---

## 1. Naming canon — three names, three layers

| Name | Is | Owns | Binary |
|---|---|---|---|
| **jet** | the **language** + compiler | `jet run/build/test/fmt/check/lsp/dev`; runs code. Knows nothing about jetpack. | `jet` (ships) |
| **jetpack** | the package-manager **engine + CLI** | reads manifests, resolves `provider@target` sources, realizes the hangar store, discovers/merges the module tree, builds environments: `jetpack run/enter/build/list/clean/add/remove`. Jetpack owns the package lifecycle; Nix is a provider, not the manager. | `jetpack` (ships) |
| **jetos** | the **OS** (working title) | a whole declarative machine + installable ISO. Built *on top of* jetpack. | (Phase 2) |

**Dependency arrow is strictly one-way: `jetos → jetpack → jet`.** This is what
keeps "jet usable on its own" true forever.

Rules of thumb:
- "A library I `use` and compile into my program" → a `payload.jet` **package**;
  a `library` package realizes as staged source.
- "A program/binary I want available in a shell" → `jetpack run nixpkgs:fastfetch`
  / `jetpack run github@owner/repo`.
- "My whole computer described in text, atomic upgrades, rollback, ISO" → jetos.

---

## 2. The two phases

```
Phase 1 — jetpack: temporary & project environments  (buildable on today's language)
  jetpack run <ref> ─▶ classify provider@target ─▶ realize into the hangar ─▶
  compose env (PATH + pretty prompt) ─▶ spawn subshell ─▶ `exit` returns clean

Phase 2 — jetos: the declarative distro            (gated on M12 layer 3 + S60 pure-eval)
  modules/ + config.jet ─▶ merge engine ─▶ system recipe ─▶ hangar build ─▶
  activation (symlink flip + boot entry) ─▶ generations/rollback ─▶ installable ISO
```

Phase 1 needs no pure-eval / M12 layer 3 — it orchestrates providers (std-only
Rust: `std::process`, `std::env`) and composes the shell itself. Phase 2 wants
the layer-3 foundations from `jetos-design.md`.

---

## 3. Phase 1 — jetpack environments (the Nix-shell / devenv replacement)

### 3.1 The experience

```
$ jetpack run nixpkgs:fastfetch

  jetpack  resolving nixpkgs:fastfetch … fastfetch 2.x
           ▸ 1 package · 142 MB · cache.nixos.org   ✓ ready in 3.1s

  entering a temporary shell — type `exit` to leave, nothing is installed.

jetpack ~/work $ fastfetch --version
fastfetch 2.x
jetpack ~/work $ exit

  left the temporary shell. your machine is unchanged.
```

A project's `env.jet` describes a richer environment (typed `module {}` surface,
`unified-ecosystem.md` §2.2); `jetpack enter <name>` / `jet dev` enters it.

### 3.2 Command surface

`jetpack run/enter/build/list/clean/add/remove` (ratified D-JPK2). `add`/`remove`
edit the manifest declaratively — never a hidden install DB. `jet dev` is the
Scale-2 front door and delegates to `jetpack enter`. CLI refs accept the
shorthand `source:package` (`nixpkgs:fastfetch`); the typed authoring surface in
files uses `provider@target` (`github@owner/repo/rev`).

### 3.3 Architecture — core resolver, providers behind one seam

```
ref ─▶ classify source ─▶ pick provider ─▶ provider.realize(ref) ─▶ Realized (bytes + bin dir)
       (provider@target)   (core | nix)                              │
                                                                     ▼
                              record in the hangar ─▶ compose env ─▶ spawn subshell
```

- **Jetpack's core owns resolution; a provider is a pluggable backend** behind
  one `Provider` trait (`src/jetpack/provider.rs`).
- **`core` provider (first-party).** Realizes Jet-native packages with no Nix —
  the system we grow our own ecosystem on, designed to overtake nixpkgs as it
  grows. A target carrying a `payload.jet` realizes through `core`.
- **`nix` provider (compatibility).** Leverages nixpkgs so the whole ecosystem
  comes for free as the fallback. Today it orchestrates `nix build --json`; the
  no-installed-`nix` engine is staged as R3 below.
- **Provider kind is inferred (U9)**, never declared: `payload.jet` present →
  core; `nixpkgs@…` → always nix; any other target → nix flake fallback. The
  probe never clones a nixpkgs-sized repo — it peeks at `payload.jet` only.
- **No new compiler crates (I6).** Phase 1 core is `std::process::Command` + a
  generated shell rcfile. The R3 tvix engine is the **only** sanctioned crate,
  isolated to the `nix` provider (see D-JPK16).
- **Determinism for tests.** Non-interactive paths are golden-tested with
  captured `nix build --json` fixtures; one interactive smoke test asserts
  prompt-set + `exit` round-trip.

### 3.4 Phase 1 milestones

| MS | Goal | Status |
|---|---|---|
| **JPK-0** | independent `jetpack` entrypoint, command parser, `provider@target` classifier | ✅ shipped (`refspec.rs`) |
| **JPK-1** | provider layer + Nix provider fixture path; missing-`nix` diagnostic | ✅ shipped |
| **JPK-2** | compose env + spawn subshell with pretty prompt; `-- cmd` non-interactive path | ✅ shipped |
| **JPK-3** | project `env.jet` + `jetpack build/add/remove` reusing the merge engine | ✅ shipped (typed `module {}` surface) |
| **JPK-4** | beauty/polish: TTY-aware color, `--no-color`/`NO_COLOR`, `jet explain` errors, `clean`/`list` | ✅ shipped |

Provider sub-roadmap (was native-resolver R0–R3; D-JPK16/17 ratified):

| Stage | Goal | Status |
|---|---|---|
| **R0** | extract the `Provider` trait; today's nix path becomes the `nix` provider | ✅ shipped |
| **R1** | named sources (`sources: { name: provider@target }`), used inline as `Pkg` refs | ✅ shipped |
| **R2** | first-party `core` provider: fetch-and-place Jet packages, content-addressed, no Nix | ✅ shipped |
| **R3** | `nix` provider with **no installed `nix`** — **tvix** (`tvix-eval` + store/substituter glue) behind the `nix` provider, isolated by a jetpack-scoped cargo feature (I6 waiver per D-JPK16). `tvix-eval` evaluates Nix but does not substitute — R3 also needs binary-cache client glue. | ⏳ **pending — the large remaining piece** |

---

## 4. Phase 2 — jetos (declarative distro + ISO)

Phase 2 is the design in [`jetos-design.md`](jetos-design.md) (NixOS restated in
Jet), built on the jetpack engine. It stays **post-v1 / research-gated** until
pure-eval (S60) and M12 layer 3 land — do not start OS code before those.

### 4.1 Parity baseline: `~/nixos` / HalcyonOmega NixOS (a requirement, not inspiration)

**There must not be anything supported in the current HalcyonOmega NixOS setup
that jetos cannot represent.** Audit source: `/home/nate/nixos`, 2026-06-15. If a
future jetos design cannot express one of these, the design is incomplete; the
escape is an explicit owner-approved exception.

- **Root / inputs** (`flake.nix` → `payload.jet` + module roots): pinned inputs
  (stable + unstable nixpkgs, plasma beta/master, flake-parts, import-tree,
  disko, Home Manager, plasma-manager, NUR, Stylix, nixcord, nix-gaming, CachyOS
  kernel, nix-flatpak, Ghostty, Vicinae, vscode-extensions, browser flakes,
  Spicetify, a local Jet path), follows/pin semantics, named package sets, and
  shared values (username, system, host, Git identity) without arg boilerplate.
- **Module tree** (`find` + `_`-disable): hosts, core OS primitives
  (boot/kernel/initrd, filesystems, audio, networking, localization, users,
  security, virtualization, graphics/input/bluetooth/smartcard, zram, XDG MIME,
  Wayland/Xorg), apps (terminal/shells/dev/gaming/desktop/utilities,
  Flatpak/AppImage, per-app config, services), KDE/Plasma DE modules
  (plasma-manager panels/widgets/shortcuts/window-rules, login manager, KDE
  Connect, ydotool, display-manager), nix/provider integration, overlays/patches,
  and tracked `assets/**`.
- **Options & merge:** declared options with type/default/docs/enums; the three
  priorities `default < normal < force`; final-value reads with cycle
  diagnostics; deterministic list/map merge independent of discovery order;
  conflict errors with file/line and a clear fix.
- **Scopes:** one feature module touching both system and user scope (replacing
  the NixOS + Home Manager split); raw file emission, source links, generated
  text, and force/backup semantics; external user-module ecosystems
  (home-manager/plasma-manager/Stylix/nix-flatpak/nixcord/vicinae) need an
  integration path.
- **Packages:** nixpkgs (stable/unstable), flake inputs, overlays, custom
  derivations (`fetchFromGitHub` + pinned hash + patches + wrapper + env +
  install phase), Flatpak, AppImage, and later native Jet packages; ergonomic,
  repo-relative asset/patch addressing.
- **Hosts & ISO:** multiple host outputs and host variants (alternate input,
  overlay, display-manager/stateVersion overrides) from one root; hardware-config
  story; a graphical Calamares Plasma installer ISO; a QEMU/VM boot+install test.
- **Imperative edges:** declarative desired state stays authoritative; activation
  hooks (e.g. Flatpak reconciliation) are supported but clearly marked, ordered,
  testable, and explainable in diffs.

### 4.2 jetos milestones

`OS0–OS4` are in `jetos-design.md` §8 (merge engine → import-tree → activation →
std option tree), all gated on M12 layer 3 + S60 pure-eval. The owner's stated
"installable ISO I can test in a VM" goal adds:

| MS | Goal | Exit criteria |
|---|---|---|
| **OS-ISO** | a bootable installable image built by jetpack | `jetos build --image` (spelling per ballot) produces an x86_64 graphical ISO; boots in QEMU; reaches a login; parity = the current `modules/hosts/iso` Calamares Plasma image |
| **OS-VM** | a scripted VM test harness | build ISO → boot in QEMU → `switch` → `rollback` round-trip; power-cut sim boots prior generation (jetos-design OS2) |

---

## 5. Sequencing & dependencies

```
Phase 1 (jetpack environments)            no pure-eval needed — JPK-0..4 + R0..R2 SHIPPED
  JPK-0 ─▶ JPK-1 ─▶ JPK-2 ─▶ JPK-3 ─▶ JPK-4        R3 (tvix) still pending
                                  │
                                  ▼  (proves merge engine + hangar on real envs)
Phase 2 (jetos)                   needs: M12 layer 3 + S60 pure-eval + the hangar
  OS0 ─▶ OS1 ─▶ OS2 ─▶ OS3 ─▶ OS4 ─▶ OS-ISO ─▶ OS-VM
```

Relationship to M12 (`docs/plans/epoch-1/m12-packages.md`): jetpack's
`payload.jet` manifest + `.jet/lock` are the package mechanism (U1/U2/U10
amended S52). Existing `jet add/remove` are transitional; later they may plumb to
`jetpack add/remove`.

The live status of what is built vs. pending is tracked in
[`IMPLEMENTATION-STATUS.md`](IMPLEMENTATION-STATUS.md) — read it before assuming a
feature exists.

---

## 6. Decision status

All Phase-1 surface decisions are **ratified** and recorded in
`docs/spec/syntax-decisions.md`: D-JPK1–23, D-JPK16/17 (native resolver +
named sources), and U1–U10 (the unified authoring surface). jetos `D-OS1`
(file-is-module) and `D-OS7` (entrypoint) are **superseded** by U3/U4.

`System`/`Image`/`Service` field semantics (U11–U14, U18), `config.jet` loading +
the `jetpack os` tier (U15/U16), consumer-side library import (U17), and
hyphenated names in package/module/system/image positions (`S84`, finalist 2) are
now **ratified and shipped** — see
[`IMPLEMENTATION-STATUS.md`](IMPLEMENTATION-STATUS.md).

**Still open** (do not implement — owner ballots in
`docs/spec/decision-ballots.md`):
- jetos config-surface syntax: `D-OS2/3/4/5/6` (`option`/`when`/`force`/`default`
  declaration form, enable flags, user scope) — see `jetos-design.md` §9.

---

## 7. Out of scope for Phase 1 (keep it shippable)

- Reimplementing the Nix builder/sandbox (Phase 2 / never if we keep orchestrating).
- A native binary cache or signing (Phase 2; jetos-design layer 3).
- Pure-eval enforcement of `payload.jet` (S60, Phase 2).
- Whole-machine activation / bootloaders / generations (that's jetos, Phase 2).
- Windows/macOS shells (Linux first; the VM/ISO target is x86_64 Linux).
