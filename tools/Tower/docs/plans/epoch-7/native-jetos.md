# Epoch 7 — native jetos

**Governing decisions** (ratified 2026-07-09, card #363): D-JOS-NATIVE1=A
(jetos is a from-scratch standalone OS; building through the NixOS module
system is forbidden — the earlier `--real` tier produced a reskinned NixOS
and was rejected), D-JOS-STORE1=A (the hangar is the on-disk store; nixpkgs
closures live under a hangar-managed compat root in stage 1 and re-root
natively later), D-JOS-NIXEVAL1=C (no `nix` binary anywhere in the product
path — jetpack implements its own nixpkgs evaluation/fetch/build pipeline
this epoch), D-JOS-PARITYBAR1=A (exit bar below), D-JOS-NIXBACKEND2=C (the
jet→NixOS realizer survives only as a clearly-labeled migration tool).

**Exit bar** (D-JOS-PARITYBAR1=A): jetos natively delivers the NixOS-class
architecture — typed module/option system, immutable generations, atomic
activation, live switch, rollback, fully declarative system — plus a
terminal baseline, a graphical baseline, one-line desktop swaps
(GNOME/KDE/Hyprland/Niri; wayland/x11), and the owner's entire ~/nixos
config running natively. Nothing is baked into jetos: the whole system is
driven by the user's jet config (defaults exist; composition is the
user's), authored by hand or through jetos Studio. Module breadth beyond
the bar grows by demand.

**Evidence base**: four audits in the session scratchpad (`epoch7/
nixos-anatomy.md`, `desktop-glue-matrix.md`, `no-nix-pipeline.md`,
`repo-inventory.md`) — NixOS assembly anatomy with per-subsystem artifact
specs, the desktop glue matrix behind one-line swaps, the no-nix pipeline
scoping (tvix prior art, cache coverage reality), and the honest inventory
of existing machinery (what is REAL vs facade) plus per-card verdicts.

## What already exists and stays (audited REAL)

Module eval + typed options with priority resolution; generation ledger
with atomic `current`/`default` symlink flips and rollback; real /etc
rendering (passwd/shadow/group/fstab/os-release/pam.d skeletons); real
systemd unit text generation + `.wants` enablement; bootable root
projection; installer media with a real guided install script (GPT/ESP/
ext4/Limine); the QEMU proof harness with honest `real-guest` vs
`plumbing` tiers; the semantic NixOS importer. These are the foundation —
epoch cards extend them, never rebuild them.

## Pillars

### P0 — Package pipeline without nix (jetpack)
The adapter that lets a standalone jetos leverage nixpkgs. Stages:
S1 substitution (drv semantics + narinfo/ed25519/NAR fetch into the
hangar compat root — ships a fully free cached desktop), S2 full
nixpkgs-grade evaluation (bit-exactness harness against a golden corpus
from the local store), S3 sandboxed builder (userns, reference scanning,
fixed-output fetches — unlocks unfree wraps like steam/discord, custom
flake inputs, cache misses). Key risks: eval bit-exactness (wrong store
path = silent cache miss), eval performance at nixpkgs scale, sandbox
correctness. Prior art: tvix (Rust) proved drv/narinfo compatibility and
partial nixpkgs eval — reuse is an owner ballot (external crates, I6,
license). The epoch removes every `nix`/`nix-store` shellout from the
product path and adds a CI gate that keeps them out.

### P1 — Native system assembly (jetos)
Reimplement, natively and per the anatomy audit's artifact specs: profile
union environment (symlink union, collision policy, gschema/mime/XDG
composition); complete /etc engine (sidecar modes, atomic apply);
activation engine (topo-sorted idempotent snippets, setuid wrapper dir,
tmpfiles, dry-run, `/run/current-system` flip); live-switch transaction
planner (semantic unit diff → start/stop/restart/reload); users/groups
reconciliation (mutable-password preservation, atomic rewrites); PAM
stack generator; dbus/udev/hwdb/fontconfig/session-env composers; systemd
unit generation v2 (upstream unit composition, drop-ins, masks, hardening
keys); native initrd builder (kernel module closure, systemd-initrd) and
first-party kernel packaging that never borrows the host kernel;
generation profiles wired to Limine menu entries with GC roots.

### P2 — Graphical jetos
A shared graphical-substrate module (session registry = the swap seam,
portals, pipewire, polkit, logind) plus thin per-DE delta modules
(GNOME/KDE/Hyprland/Niri) and a display-manager layer (gdm/sddm/greetd,
auto-derived with override). One-line swaps with typed assertions for
invalid combos (niri/hyprland are wayland-only). Terminal and graphical
baseline profiles as *defaults*, entirely overridable. The live-desktop
proof harness runs against native builds only.

### P3 — Breadth: reopen the facades honestly
Twelve breadth cards were closed on JSON-facts facades (fleet deploy,
options search, containers/microVMs, image variants — which also violates
D-JOS-IMAGEPROOF1 by labeling sparse markers "built" —, hardware
detection, lifecycle, disks/impermanence, app library, theming, desktop
breadth, flatpak, Studio). Each reopens with its original scope and a
native, proof-gated exit. Studio (#235/#264/#357) becomes the live config
authoring app over the same typed option registry.

### P4 — Migration and acceptance
The semantic importer stays central. The jet→NixOS realizer is repackaged
behind an explicit migration surface (labeled as building a NixOS system
for A/B comparison during transition — including removing the dishonest
distro rebranding). Owner-config acceptance (#337) reopens with the
native bar. The epoch closes on the same proof harness this session
validated: real QEMU guests, live-session assertions, screenshots.

## Sequencing spine

P0-S1 + P1 run in parallel (substitution feeds the compat root the
assembly consumes). Graphical (P2) starts once P1 boots a native terminal
baseline in the VM. Breadth (P3) and migration (P4) ride behind. The
epoch's first headline proof: **a native jetos terminal baseline booting
in QEMU with zero nix binary involvement**; second: **the GNOME baseline
via substitution only**; third: **one-line swap matrix green**; fourth:
**owner config native**.

## Ratified owner decisions (2026-07-10)

- B-E7-DESKTOPNS1=E: `services.desktop.*` with `.Auto` derivation and typed
  invalid-combination assertions.
- B-E7-BASELINE1=D: terminal and graphical baselines materialize every choice
  directly into `config.jet`; terminal is lean-modern, graphical is complete
  without requiring a command line, and expert edits are never silently healed.
- B-E7-IDENTITY1=E: `YY.MM` releases plus alphabetically ordered
  aviation-navigation codenames; first release is `26.10 "Apex"`.

No owner ballot remains open for this plan. New syntax, dependencies, or policy
exceptions discovered during implementation still require the normal ballot
protocol.
