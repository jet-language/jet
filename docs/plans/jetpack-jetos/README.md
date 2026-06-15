# jetpack & jetos — consolidated plan (owner concurrence)

**Status:** draft for owner concurrence, 2026-06-15.
**Owner direction (2026-06-15):** make **jetpack** a real package manager and,
while building it, treat it as independent from the `jet` binary. The headline
feature is a Nix-`shell`/`devenv`-class **temporary environment** —
`jetpack run github:sadjow/claude-code-nix` drops you into a beautiful shell
with that package available, updates the prompt, and `exit` leaves cleanly.
Phase 1 commands are **`jetpack run/build/list/clean/add/remove`**. Later,
`jet run github:...` can be plumbed to Jetpack. The root Jet pack file is the
Jet equivalent of `flake.nix`; ratified filenames are **`pack.jet`** and
**`pack.lock`**. **jetos** (the OS) is Phase 2 and is built *on top of* jetpack.

**If you are a fresh agent picking this up:** read this file top to bottom, then
`docs/plans/jetpack-jetos/halcyonomega-nixos-audit.md`, then the two background
briefs `docs/research/jetpack-config.md` and `docs/research/jetos.md` (older;
superseded by this file where they disagree).
**Do not write code yet** unless the owner explicitly asks you to proceed. All
D-JPK decisions are ratified in `docs/admin/02-syntax-decisions.md`. Build the
failing tests/examples first once the owner asks you to proceed.

---

## 1. Naming canon (the cleanup)

Three names, three layers. Use these exactly everywhere from now on.

| Name | Is | Owns | Binary today |
|---|---|---|---|
| **jet** | the **language** + compiler toolchain | `jet run/build/test/fmt/check/lsp`; current `jet add/remove` are transitional package-manager commands that may later plumb to jetpack. For Phase 1, do not depend on `jet` for Jetpack. | `jet` (exists) |
| **jetpack** | the **package manager engine and Phase 1 CLI** | system/binary **packages** and **environments**: `jetpack run/build/list/clean/add/remove`, dev shells, nixpkgs interop through a translation provider, native Jetpack builder path, generations/store. Jetpack manages packages; Nix is a provider, not the manager. | `jetpack` (to build) |
| **jetos** | the **operating system** (working title) | a whole declarative machine + installable ISO. **Uses jetpack underneath** | (Phase 2) |

Rules of thumb:
- "A library I `import` and compile into my program" → current **jet** package
  flow (`jet.toml`/`jet.lock`); later `jet add/remove` may plumb through jetpack.
- "A program/binary I want available in a shell, from nixpkgs or a Jet package" →
  **jetpack** (`jetpack run nixpkgs:fastfetch`, `jetpack run github:owner/repo`)
  during Phase 1.
- "My whole computer described in text, atomic upgrades, rollback, ISO" → **jetos**.

Collisions to fix (see §7): the example library `examples/jetos/lib/jetpack.jet`
and `import jetpack as pkg` currently use the *jetpack* name for an in-Jet merge
library. After ratification, the **tool** is jetpack; the example library should be
folded into the real tool or renamed so the name means one thing.

---

## 2. The two phases

```
Phase 1 — jetpack: temporary & project environments  (near-term, shippable now)
  jetpack run <source>:<package/path> ─▶ resolve ref ─▶ translate provider ─▶
  realize store paths under Jetpack control ─▶
  spawn subshell: PATH + pretty prompt ─▶ run/expose pkg ─▶ `exit` returns clean

Phase 2 — jetos: the declarative distro                (depends on jetpack + layer 3)
  modules/ + hosts/ ─▶ merge engine ─▶ system recipe ─▶ jetpack store/build ─▶
  activation (symlink flip + boot entry) ─▶ generations/rollback ─▶ installable ISO
```

Phase 1 is deliberately **buildable on today's language** — it orchestrates Nix to
realize packages and composes the shell itself (std-only Rust: `std::process`,
`std::env`). It does **not** need pure-eval / M12 layer 3. Phase 2 (own builder,
activation, ISO) wants the layer-3 foundations from `docs/research/jetos.md`.

---

## 3. Phase 1 — jetpack environments (the Nix-shell / devenv replacement)

### 3.1 The experience (the whole point)

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

Requirements the owner stated, mapped to behavior:

| Owner ask | Behavior |
|---|---|
| "pack.jet is the jet version of flake.nix" | The Jet pack file is the root project/environment file (`pack.jet` + `pack.lock`). Jet pack file takes priority; `flake.nix` fallback is translated by Jetpack |
| "put the user into a temporary shell based on that config/pack file" | `jetpack run <source>:<package/path>` enters a subshell; the env is described by the Jet pack file or by a translated Nix flake fallback |
| "compatible with nix packages or jet packages" | `<source>:<package/path>` may target nixpkgs, a GitHub pack repo, a flake fallback, or a native Jetpack package; same command, same shell |
| "using the existing nixpkgs binary from the github repo" | Jetpack translates the Nix provider output and manages lock/state/shell/store itself. Nix can provide build data/cache access; Jetpack owns the package lifecycle |
| "The prompt should be updated" | the subshell gets a distinct, pretty prompt. Default visible label is `jetpack`; options may show package/source too |
| "typing exit should exit the user from the shell" | the subshell is a normal child shell; `exit`/Ctrl-D returns to the original shell, env restored |
| "it should look beautiful" | reuse the `nh`/ansi aesthetic already in `examples/jetos/lib/ansi.jet`; quiet, aligned, colored, TTY-aware (`--no-color`, `NO_COLOR`) |
| "fully functional… so I can test development progress" | golden-tested non-interactive paths + a real interactive smoke test |

### 3.2 Command surface (ratified for Phase 1)

For now, Jetpack is independent from `jet`:

- `jetpack run nixpkgs:fastfetch` — resolve the nixpkgs `fastfetch` attr through
  Jetpack's Nix provider translator and enter a temporary Jetpack shell.
- `jetpack run github:halcyonomega/my-fastfetch-jet-config` — classify the
  remote ref, realize the environment/package, and enter a temporary Jet shell.
- `jetpack run <ref> -- cmd…` — run a command inside that temporary environment and
  exit.
- `jetpack build [<ref>]` — realize the current project or remote ref without
  entering a shell.
- `jetpack list` — inspect realized Jetpack environments/store entries.
- `jetpack clean` — remove unused Jetpack-managed entries.
- `jetpack add <source>:<package/path>` / `jetpack remove <source>:<package/path>` —
  edit `pack.jet` (declarative front door; never a hidden install DB).

`jet run main.jet` remains the existing local-file language workflow. Later,
after Jetpack is functional, `jet run github:...`, `jet shell`, and possibly
`jet add/remove` can become plumbing over the Jetpack commands. Do not build
that coupling into the Phase 1 implementation.

### 3.3 The Jet pack file (Jet's `flake.nix`)

A project directory has one root Jet pack file describing its inputs,
environment, package outputs, apps, dev shells, and later JetOS system outputs.
This role is ratified as the Jet equivalent of `flake.nix`: the root
configuration object for a project repo, not a random ad hoc script. Ratified
filenames: **`pack.jet`** and **`pack.lock`**. The earlier prototype name was
`config.jet`.

Minimal Phase 1 shape (directive syntax is ratified as the shippable v1 surface;
the prettier fluent surface remains the target evolution):

```jet
// pack.jet — a Jetpack dev environment (Jet's flake equivalent)
import jetpack as pkg;

pub fn shell() -> [JSON] {
    return [
        pkg.source("nixpkgs");
        pkg.packages(["ripgrep", "fd", "claude-code"]);
        pkg.prompt("jetpack");              // default prompt label
    ];
}
```

`jetpack run github:owner/repo` with **no local project** treats the repo as a
pack source. Jetpack first looks for the Jet pack file. If absent, it may
translate a `flake.nix` fallback. `jetpack run nixpkgs:fastfetch` targets the
nixpkgs source and package attr directly.

### 3.4 Architecture (orchestrate Nix; compose the shell ourselves)

```
<source>:<package/path>
      ──▶ classify source: nixpkgs | github | local path | future registry
      ──▶ load pack file first; if absent and source is Nix-compatible,
          translate flake/provider metadata
      ──▶ realize through provider: native jetpack builder or Nix provider
      ──▶ record Jetpack-owned lock/state under /etc/jet
      ──▶ compose env: PATH += /etc/jet/store/<...>/bin ; collect run/wrapper bins
      ──▶ spawn subshell: $SHELL with generated rcfile, visible `jetpack` prompt,
          JETPACK_ENV marker; inherit tty
      ──▶ user works; `exit`/Ctrl-D ──▶ child exits ──▶ parent env untouched
```

Notes:
- **Provider boundary is explicit.** Jetpack owns the package lifecycle. Nix is a
  compatibility provider used to translate/build nixpkgs and flakes; a native
  Jetpack builder can sit beside or replace it.
- **Nix dependency is provider-specific.** If `nix` is absent and the selected
  source needs the Nix provider, `jetpack run nixpkgs:fastfetch` fails with a
  clear diagnostic + install pointer. Native Jetpack sources should not require
  Nix.
- **No new crates (I6).** Phase 1 core is `std::process::Command` + a generated
  shell rcfile. Beautiful output reuses the ansi module's approach.
- **Determinism for tests.** Non-interactive paths (`jetpack run <ref> -- cmd`,
  resolution, env composition) are golden-tested with captured `nix build --json`
  fixtures, exactly like `examples/jetos/nix/fixtures/`. One interactive smoke
  test asserts prompt-set + `exit` round-trip.

### 3.5 Phase 1 milestones

Use `JPK-N`. Each is done only when its exit criteria pass as tests (project I4/I5:
every diagnostic has a snapshot; every feature ships an example + expected output).

| MS | Goal | Exit criteria |
|---|---|---|
| **JPK-0** | Scaffold independent `jetpack` entrypoint, command parser, and `<source>:<package/path>` classifier | D-JPK1…15 ratified; commands parse; ref-classifier unit-tested (`nixpkgs:fastfetch`, GitHub pack repo, local project) |
| **JPK-1** | Provider translation layer + Nix provider fixture path | golden: resolve `nixpkgs:fastfetch` from a fixture → Jetpack store path; flake fallback fixture translates; missing-`nix` provider diagnostic snapshot |
| **JPK-2** | Compose env + spawn subshell with pretty prompt; `-- cmd` non-interactive path | golden: `jetpack run <fixture-ref> -- claude --version` prints version, exits 0, parent env unchanged; interactive smoke: prompt set, `exit` returns |
| **JPK-3** | Project pack file + `jetpack build/add/remove` reusing the merge engine | golden: a sample project resolves its packages; add/remove edit pack file and re-resolve |
| **JPK-4** | Beauty + polish: TTY-aware color, progress/summary lines, `--no-color`, `jet explain`-style errors, `clean`/`list` | golden human + `--no-color` output pinned; docs page `docs/guide/` written; one real example dir under `examples/` |

### 3.6 Reuse from the existing `examples/jetos/` slice

- `lib/ansi.jet` aesthetic → the jetpack output style.
- `lib/jetpack.jet` merge engine (lists combine+dedup+sort; scalars by priority) →
  reused when a project has multiple env contributions.
- `nix/fixtures/*.json` capture pattern → jetpack's deterministic resolution tests.
- The `nh`-style CLI shape in `jetos.jet` → the jetpack command UX.

---

## 4. Phase 2 — jetos (the declarative distro + ISO)

Phase 2 is the existing vision in `docs/research/jetos.md` (NixOS restated in Jet),
now explicitly **built on the jetpack tool from Phase 1**. The merge engine,
option tree, dendritic modules, hosts, generations, and `switch/diff/rollback`
described there are unchanged; jetpack provides the store/build/activation
substrate that doc assumes.

### 4.1 Parity baseline: `~/nixos` / HalcyonOmega NixOS

Owner direction: **there must not be anything supported in the current
HalcyonOmega NixOS setup that JetOS cannot represent.** The prototype end state
should preserve the structure and capability of `/home/nate/nixos` while making
the syntax and ergonomics substantially better. The detailed audit lives in
`docs/plans/jetpack-jetos/halcyonomega-nixos-audit.md`.

Observed baseline structure:

- The Jet pack file maps to root `flake.nix`: pinned inputs, follows relationships,
  common args (`username`, `system`, Git identity), and exported hosts/modules.
- `modules/hosts/<host>/` maps to host profiles: `halcyon`,
  `halcyon-plasma-beta`, and `iso`, including hardware config and host variants.
- `modules/core/**` maps to OS primitives: bootloader/kernel profiles, initrd,
  filesystems, audio, networking, localization, users, security, virtualization,
  graphics/input/bluetooth/smartcard, zram, XDG MIME defaults, Wayland/Xorg.
- `modules/apps/**` maps to dendritic app modules: terminal commands, shells,
  development tools, gaming, general desktop apps, utilities, Flatpak/AppImage,
  per-app config files, user packages, and services.
- `modules/desktop-environments/kde/**` maps to first-class desktop environment
  modules: KDE Plasma, plasma-manager, panels/widgets/shortcuts/window rules,
  login manager, KDE Connect, ydotool, display-manager selection.
- `modules/nix/**` maps to provider/package-manager integration: nixpkgs config,
  substituters, Home Manager integration, nh-like helper UX, shared modules.
- `overlays/**`, `overlays/patches/**`, and custom derivations map to package
  overrides, source pins, patches, wrappers, generated scripts, and custom
  build/install phases.
- `assets/**` maps to tracked files available to modules: wallpapers, themes,
  logos, config JSON, shell scripts, and patch files.

JetOS parity requirements:

- Import-tree composition with `_`/parking support and no hand-maintained
  module list for ordinary modules.
- Multiple package inputs and follow/pin semantics (`nixpkgs`, unstable,
  plasma beta/master, Home Manager, Stylix, NUR, gaming, Flatpak, browsers,
  local path inputs such as the Jet repo).
- Host outputs, host variants, and ISO outputs from the same project root.
- Special args / shared values without boilerplate: username, system, host,
  Git identity, unstable packages, and inputs.
- NixOS-style options: declared options, enums, defaults, force, conflict
  diagnostics, and final-value reads.
- System and user scopes in one feature module, replacing the NixOS +
  Home Manager split with one JetOS scope model.
- External module consumption equivalent to Home Manager, plasma-manager,
  Stylix, nix-flatpak, nixcord, vicinae, and similar ecosystems.
- Packages from nixpkgs, flake inputs, overlays, custom derivations, Flatpak,
  AppImage, and later native Jet packages.
- Raw file generation and source linking equivalent to `xdg.configFile`,
  `environment.etc`, config text blobs, JSON/JSONC, shell scripts, desktop MIME
  maps, and app settings.
- Activation hooks/scripts for unavoidable imperative edges such as Flatpak
  reconciliation, while keeping the declarative desired state authoritative.
- Overlay/patch support, including `overrideAttrs`, source replacement,
  patch files, wrapper scripts, env vars, and custom install phases.
- Asset-addressing that is ergonomic and repo-relative, without stringly path
  gymnastics.
- Desktop ergonomics as first-class modules, especially KDE Plasma structure:
  panels, widgets, shortcuts, window rules, power/session behavior, login
  manager, theme, icons, cursor, fonts, wallpaper, and MIME defaults.
- VM/ISO support at parity with the current graphical Calamares Plasma ISO:
  buildable image, install tools, curated packages, experimental features, and
  boot sanity checks.

This parity baseline is a requirement, not inspiration. If a future JetOS design
cannot express one of these patterns, the design is incomplete.

New for the owner's stated goal ("an installable iso so I can test in a VM"):

| MS | Goal | Exit criteria |
|---|---|---|
| OS0–OS4 | as in `docs/research/jetos.md` §8 (merge engine → import-tree → activation → std option tree) | unchanged; gated on M12 layer 3 + S60 pure-eval |
| **OS-ISO** | a bootable installable image built by jetpack | `jet os build --image` / `jetpack build --image` (spelling TBD) produces an x86_64 graphical ISO; boots in QEMU/VM; reaches a login; parity target is the current `modules/hosts/iso/iso.nix` Calamares Plasma image |
| **OS-VM** | a scripted VM test harness | CI/local: build ISO → boot in QEMU → `switch` → `rollback` round-trip; power-cut sim boots prior generation (jetos.md OS2) |

Phase 2 stays **post-v1 / research-gated** until pure-eval (S60) and M12 layer 3
land — do not start OS code before those. Phase 1 (jetpack environments) has no
such gate and is the near-term work.

---

## 5. Sequencing & dependencies

```
Phase 1 (jetpack environments)            no pure-eval needed — buildable now
  JPK-0 ─▶ JPK-1 ─▶ JPK-2 ─▶ JPK-3 ─▶ JPK-4
                                  │
                                  ▼  (proves merge engine + store on real envs)
Phase 2 (jetos)                   needs: M12 layer 3 + S60 pure-eval + jetpack store
  OS0 ─▶ OS1 ─▶ OS2 ─▶ OS3 ─▶ OS4 ─▶ OS-ISO ─▶ OS-VM
```

Relationship to Epoch 2 (`docs/plans/epoch-2/README.md`): jetpack Phase 1 is a new
near-term track the owner has prioritized for testing development progress. It
overlaps the Epoch 2 package/supply-chain milestones (E2-M8) and the layer-3 work
(E2-M16) but is *not blocked* by them. Owner should place Phase 1 in the schedule
(decision E2-V12 / the new D-JPK gates below).

Relationship to M12 (`docs/plans/epoch-1/m12-packages.md`): M12's `jet.toml` /
`jet.lock` source-library work is the pre-Jetpack dependency mechanism. For
Jetpack Phase 1, `jetpack add/remove` own package/environment edits to the pack
file. Existing `jet add/remove` should be treated as transitional; later they
can be replaced by plumbing to `jetpack add/remove` where the ref belongs to
Jetpack's domain.

---

## 6. Decision gates (owner calls — needed before code)

Per CLAUDE.md, user-facing CLI/naming surface needs owner ratification before
implementation. Each option below is shown with a concrete example so it's
decidable at a glance (owner decision-doc style).

| ID | Question | Options (with example) | Rec |
|---|---|---|---|
| **D-JPK1** | Is `jetpack` independent or hidden behind `jet` first? | **Ratified:** build `jetpack` as an independent binary first; later `jet` can delegate to it. | **Ratified 2026-06-15; amended 2026-06-15** |
| **D-JPK2** | Verb set for Phase 1 | **Ratified:** `jetpack run/build/list/clean/add/remove`. | **Ratified 2026-06-15; amended 2026-06-15** |
| **D-JPK3** | The pack-file author surface | **Ratified A for Phase 1:** directive form (ships today): `pkg.packages(["ripgrep","claude-code"])` (see §3.3). Evolve to fluent form after first-party jetpack module support. Root file: `pack.jet`. | **Ratified 2026-06-15** |
| **D-JPK4** | What happens to existing `jet add/remove`? | **Ratified:** treat current `jet add/remove` as transitional pre-Jetpack commands; future plumbing may route relevant refs to `jetpack add/remove`. | **Ratified 2026-06-15** |
| **D-JPK5** | Nix dependency posture for Phase 1 | **Ratified:** Jetpack owns packages; Nix is a compatibility provider translated by Jetpack, not the package manager. Native builder path remains part of the architecture. | **Ratified 2026-06-15** |
| **D-JPK6** | Fate of Forge | **Ratified:** salvage useful notes/features, then remove `examples/capstone/forge/`. | **Ratified 2026-06-15** |
| **D-JPK7** | Where Phase 1 sits + ref syntax | **Ratified:** Jetpack next; refs are `<source>:<package/path-to-package>`, e.g. `nixpkgs:fastfetch` and `github:halcyonomega/my-fastfetch-jet-config`. | **Ratified 2026-06-15** |
| **D-JPK8** | What is the Jet pack file? | **Ratified role:** Jet's equivalent of `flake.nix`: root repo config for inputs, package/env outputs, dev shells, and later JetOS system/ISO outputs. Filenames: `pack.jet` + `pack.lock`. | **Ratified 2026-06-15** |
| **D-JPK9** | Are direct `jetpack ...` commands public? | **Ratified:** yes for Phase 1; use direct `jetpack ...` commands while building the package manager. | **Ratified 2026-06-15** |
| **D-JPK11** | Remote ref contract | **Ratified:** pack file first; if absent, translate `flake.nix` fallback through Jetpack. | **Ratified 2026-06-15** |
| **D-JPK12** | State/store roots | **Ratified:** system-style roots `/etc/jet/` and `/etc/jet/store/`; choose implementation details carefully for permissions/dev mode. | **Ratified 2026-06-15** |
| **D-JPK14** | Shell prompt support | **Ratified:** bash/fish/zsh; default visible prompt label is `jetpack`; prompt style/options supported. | **Ratified 2026-06-15** |
| **D-JPK15** | Nix compatibility syntax | **Ratified:** support flakes and nixpkgs attrs through `<source>:<package/path>`; users should not type `#`. | **Ratified 2026-06-15** |
| **D-JPK13** | Pack file and lockfile naming | **Ratified A:** `pack.jet` + `pack.lock` ("Jet packs"; avoids repeating `jet`). | **Ratified 2026-06-15** |

Background decisions already recorded and still in force: jetos.md D-OS1..7,
D-NX1..6; jetpack-config.md D-JP1..5. This file supersedes their *sequencing*
(Phase 1 first) but not their *content*.

---

## 7. Reconciliation / cleanup actions (do as part of JPK-0)

1. **Terminology sweep.** Apply §1 canon across `docs/` and `README.md`. The name
   *jetpack* = the tool; stop using it for the in-example merge library.
2. **`import jetpack as pkg` collision.** Decide (with D-JPK3) whether the project
   config imports a first-party `jetpack` module (Path A) or keeps the local
   library; until then the example keeps its local `lib/jetpack.jet` but the docs
   note it is a stand-in for the future built-in.
3. **forge capstone.** Per D-JPK6/D-JP4, useful Forge ideas were saved in
   `docs/plans/jetpack-jetos/forge-salvage.md`; `examples/capstone/forge/` is
   removed so Jetpack is the only package-manager path.
4. **Roadmap pointer.** `docs/admin/05-roadmap.md` gains a short "jetpack & jetos"
   subsection pointing here (done 2026-06-15).
5. **Research-doc status notes.** `docs/research/jetos.md` and
   `docs/research/jetpack-config.md` get a banner pointing to this consolidated
   plan as the live sequencing source (done 2026-06-15).

---

## 8. Out of scope for Phase 1 (say no, keep it shippable)

- Reimplementing the Nix builder/sandbox (Phase 2 / never if we keep orchestrating).
- A native binary cache or signing (Phase 2; jetos.md layer 3).
- Pure-eval enforcement of `pack.jet` (nice-to-have; S60, Phase 2).
- Multi-user / system activation / bootloaders / generations of the *whole machine*
  (that's jetos, Phase 2).
- Windows/macOS shells (Linux first; the VM/ISO target is x86_64 Linux).
