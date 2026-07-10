# jetos — research appendix (frozen)

**Frozen by D-JETOS-FREEZE1.** jetos is not being built. This file preserves
research only; nothing here is current syntax law or Epoch 4 implementation
scope. Epoch 7 must reopen exact module, command, generation, image, and
activation ballots before any fixture, parser, evaluator, or CLI work.

**Card:** `#2 c27` (jetos generations) folds in here as frozen research.
Historical prereqs named Phase A/B/C surfaces; D-JETOS-FREEZE1 supersedes that
sequencing. Do not start OS0 until Epoch 7 re-ratifies the surface.

Run everything through `nix develop -c`. Zero external crates in the compiler
(I6). Every diagnostic needs `docs/spec/diagnostics.md` + a snapshot (I4).

Ranked priorities (philosophy.md) settle every call below: **safety first**
(activation is atomic or it did not happen; a bad generation always has a boot
entry to roll back to), **beginner experience second** (typed options with
what/why/fix, not evaluator traces; one `module system.laptop { }` boots a
machine), then performance, one mechanical path, simplicity, breadth.

> **Syntax caveat.** Worked examples below use the spellings sketched in
> `vision.md` (`module system.laptop { }`, `imports: find()`, `Service.{}`,
> `User.{}`) plus *illustrative placeholders* for surfaces that are not yet
> ratified (option declarations, `jet os` verbs, generation naming). Every
> placeholder is tagged to a ballot row and MUST NOT reach a fixture or golden
> before its decision ID is ratified (I7).

---

## What ships (the sentence)

A whole machine is one Jet closure: modules declare typed options, the merge
engine reduces them to one canonical value tree, jetpack realizes the closure
into the hangar, activation flips a generation atomically, and any prior
generation boots from the bootloader. NixOS outcomes; no second untyped
language, no evaluator trace, no `/etc` drift.

## What jetos v1 does NOT do

Named cuts, so scope is honest:

- **Not multi-host.** Single machine only. `fleet.*` push (U15) realization
  rides *on top of* single-host jetos and is out of Phase D v1.
- **Linux only** (U25). No Windows/macOS system tier — those stay jetpack-only.
- **No native init in v1.** System services generate **systemd units**; a native
  Jet supervisor is a later card (ballot D-JPK-OSINIT1 records the intent, v1
  answer is systemd). Dev services (U12) stay jetpack-supervised and separate.
- **No secure-boot / TPM / auto-FDE.** The installer may prompt for LUKS but
  jetos does not manage keys/enrollment in v1.
- **No binary-cache push for system closures** (TLS-gated, U24). Local realize
  only; envelope fields ride the A4 schema.
- **No `/tmp` litter, no daemon, no root-resident state** (U28/U22). Activation
  is the sole `sudo` boundary and it is transient.

If a jetos design cannot express a row of the HalcyonOmega parity baseline
(`README.md` §jetos Parity Baseline), the design is incomplete — OS4 is where
that list is discharged.

---

## Stage map

| Stage | Goal | One-liner |
|---|---|---|
| OS0 | typed option registry + merge | `option` declarations w/ type/default/docs/enum; three modules → one canonical value tree; scalar conflict + cycle diagnostics; order-independent |
| OS1 | import tree + host selection + check | `imports: find()`, one-char disable, discover `module system.<name>`, `jet os check` evaluates without building |
| OS2 | build generator + activation | closure → generation dir (bootloader entry, /etc, activation script); atomic switch; rollback; power-cut safe |
| OS3 | lift | adopt a running machine → emitted `module system.<name>`; private-option read rejected |
| OS4 | std option tree v0 | boot/fs/net/users/services/desktop options; real laptop boots from `system.laptop`; parity baseline discharged |
| OS-ISO | installable graphical image | `jet os image installer` → x86_64 Plasma Calamares ISO; QEMU boots to installer |
| OS-VM | scripted VM harness | build → boot → install → switch → rollback round-trip in CI |

Build order: **OS0 → OS1 → OS2 → (OS3 ∥ OS4) → OS-ISO → OS-VM.** OS-VM tooling
is stood up alongside OS2 (it is how OS2+ are tested) but its full round-trip
exit lands last.

---

## OS0 — typed option registry + merge engine

Today `options:` is captured as flat `key: value` **strings**
(`ModuleEval/System.rs`, `OptionPlan{ key, value }`). jetos needs a **typed
option tree**: each option is *declared* once with a type, default, docs, and
optional enum; modules *set* values; the merge engine reduces all sets to one
final value per option with deterministic list/map merge, priority-ordered
scalar resolution, and cycle detection.

### Worked example

```jet
// modules/net.jet — a module DECLARES options and SETS them
option net.hostname: Text
    = "jet-box"
    doc "The machine's hostname."

option net.firewall.ports: [Port]
    = []
    doc "TCP ports to open."

module system.laptop {
    net.hostname: "halcyon"
    net.firewall.ports: [22, 443]
}
```

```jet
// modules/net-extra.jet — a second module ADDS to the same list option
module system.laptop {
    net.firewall.ports: [8080]        // list-merges with the above
}
```

`jet os check @laptop` reduces this to:

```json
{ "net": { "hostname": "halcyon", "firewall": { "ports": [22, 443, 8080] } } }
```

List order follows **module discovery order** (OS1), which is itself
deterministic, so the reduction is stable under file-shuffle.

### Merge rules (extend `Merge.rs`, do not fork it)

`Merge.rs` already does sources-by-key / packages-concat-dedup / scalar-by-
priority. Options add one tree layer:

- **scalar option**: one winner. Two modules set different scalars with equal
  priority → `E09xx option-scalar-conflict` (both spans, both values).
  `force`/`default` priority (already in `Merge.rs`) breaks ties.
- **list option**: concatenate in discovery order; de-dup exact repeats.
- **map option**: merge by key; same key different value recurses (scalar rule
  at the leaf).
- **cycle**: option A's default reads option B which reads A → `E09xx
  option-cycle` naming the path. Reuse the pure-eval (`Comptime`) evaluator that
  already backs module fields; option defaults are pure expressions over other
  options' final values.

### Diagnostics

- `E09xx` `option-undeclared` — a module sets `net.hstname` (no such option).
  Fix: names the closest declared option (edit-distance).
- `E09xx` `option-type-mismatch` — set value's type ≠ declared type.
- `E09xx` `option-enum-reject` — value outside the declared enum; lists the
  allowed set.
- `E09xx` `option-scalar-conflict` — two equal-priority sets disagree.
- `E09xx` `option-cycle` — default-read cycle.

### Exit

Three modules merge to canonical JSON; scalar-conflict and cycle snapshots
pinned; a shuffled file list produces byte-identical JSON.

### Tests

`option_tree_merges_three_modules`; `list_option_concats_in_discovery_order`;
`scalar_conflict_snapshot`; `cycle_snapshot`; `shuffle_order_deterministic`
(permute discovery order, assert identical JSON); `undeclared/type/enum`
diagnostic snapshots.

### Depends on

Phase A `module system.laptop { }` role form; `Merge.rs`; `Comptime` pure-eval;
option-declaration spelling → **ballot D-JPK-OSOPT1**.

---

## OS1 — import tree + host selection + check/init

### Worked example

```jet
// system.jet (any filename; only pkg.jet is reserved)
module system.laptop {
    imports: find("./modules")            // discover the whole module tree
    net.hostname: "halcyon"
}
```

```
$ jet os check @laptop
  reading modules … 5 discovered (modules/net.jet, modules/desktop.jet, …)
  system laptop → linux.x64
  options: 34 declared, 12 set, 0 conflicts
  ok — nothing built (check only)
```

One-character disable (parity baseline): a module prefixed to opt out is skipped
by `find()` without editing the importer — same `find()` discovery `env.*`
already uses. Disable marker spelling is **ballot D-JPK-OSDISABLE1**.

`jet os check` evaluates + merges + host-selects but realizes nothing (offline,
U29-clean). `jet os init` scaffolds a starter `system.<name>` + `modules/`.

### Host selection

Today `JetOS.rs` uses `[config-path]@<host>` against a default
`~/.jet/config.jet`. Phase A canon says role modules are discovered by
declaration in any `.jet` file — so host selection must reconcile: `jet os
switch @laptop` discovers `module system.laptop` across the repo (or a canonical
system root). The `@host` selector, its default search root, and whether a bare
`jet os switch laptop` (no `@`) is allowed → **ballot D-JPK-OSHOST1**.

### Exit

Example repo, two hosts, five modules → identical JSON across discovery orders;
`jet os check` builds nothing and touches no network.

### Tests

`two_hosts_five_modules_stable_json`; `check_realizes_nothing`;
`unknown_host_lists_known` (reuse `E0980`); `disabled_module_skipped`;
`init_scaffolds_bootable_skeleton`.

### Depends on

OS0; `find()` discovery; Phase A filename canon; A1 dispatch seam.

---

## OS2 — build generator + activation + generations/rollback

The heart. Turn the reduced option tree + realized package closure into a
**generation directory**, activate it atomically, and make rollback a
first-class boot path. `JetOS.rs` already writes a content-addressed
`systems/<host>-<fp>/manifest.json` and flips `current`/`default` symlinks —
extend that from "record intent" to "assemble and activate a real system".

### Generation contents

Beyond today's `manifest.json`, a generation directory gains:

- **`etc/`** — the rendered `/etc` tree (raw file emission, source links,
  generated text, force/backup semantics — parity baseline).
- **`sw/`** — the system package closure (symlink farm into the hangar; the
  hangar objects are **GC roots** for as long as any generation references them,
  U22 — `jet clean` never collects a generation-reachable object).
- **units/** — generated systemd unit files (v1 init model; ballot
  D-JPK-OSINIT1).
- **`kernel` / `initrd`** — realized boot artifacts + a **bootloader entry**
  (systemd-boot / GRUB entry) naming this generation.
- **`activate`** — the idempotent activation script (swap `/etc`, reload units,
  run activation hooks).

### Worked example — switch, generations, rollback

```
$ jet os switch @laptop --name "pre-gpu-driver"
  building system laptop (linux.x64)
  realizing 214 packages … done (3 new, 211 cached)
  generation 47 assembled: laptop-9f3c1a2b0e4d  "pre-gpu-driver"
  [sudo] activating … /etc swapped, 6 units reloaded, bootloader updated
  ok — generation 47 is now current + boot default

$ jet os generations
  48  2026-07-03 14:22  current   "post-gpu-driver"
  47  2026-07-03 14:01            "pre-gpu-driver"
  46  2026-07-02 19:40            (unnamed)

$ jet os rollback
  rolling back to generation 47 "pre-gpu-driver"
  [sudo] activating … done
  ok — generation 47 is current; reboot to change the boot default? no (live-switched)
```

Activation is the **sole `sudo` boundary** (U28): transient, no resident daemon,
no root-owned state beyond the generation store. Generation naming (`--name` vs
`jet os rename` vs auto+override; collision; sort of named/unnamed) →
**ballot D-JPK-OSGEN1** (the `#2 c27` open question).

### Atomicity + power-cut safety

Ranked priority #1 (safety): activation never leaves a half-state.

1. Assemble the generation fully under a temp name; fsync.
2. Write the bootloader entry for the new generation **before** flipping
   `current` (so a crash mid-switch still has a bootable entry).
3. Flip `current`/`default` via temp-symlink + `rename` (already in `JetOS.rs`
   `point()`), which is atomic on the same filesystem.
4. Run `activate`; on failure, auto-revert `current` to the prior generation and
   surface `E09xx activation-failed` with the failing step.

Power-cut sim (OS-VM): kill QEMU mid-switch → machine boots the prior generation
every time.

### Secrets at activation (U13)

System secrets decrypt **into activation memory only** — a `tmpfs` mount, never
the generation dir, never the hangar, never the bootloader. Reads carry the
`Secret` effect. Where the encrypted system-secrets file lives (repo vs
`/etc/jet`), and how recipient keys reach a headless machine, →
**ballot D-JPK-OSSECRET1**.

### Diagnostics

- `E09xx` `activation-failed` — an activation step failed; prior generation
  restored; names the step.
- `E09xx` `generation-not-found` — `rollback <n>` names no generation.
- `E09xx` `os-needs-root` — activation without privilege; fix: re-run (transient
  sudo prompt), never "become a daemon".

### Exit

VM `switch → rollback` round-trip; power-cut sim boots the prior generation;
`jet clean` never collects a generation-reachable hangar object.

### Tests

`generation_dir_has_bootentry_before_pointer_flip`; `switch_then_rollback_vm`
(OS-VM harness); `powercut_boots_prior_generation`; `activation_failure_reverts`;
`generation_objects_are_gc_roots`; `secret_never_hits_disk`
(scan generation dir + hangar + bootloader for plaintext).

### Depends on

OS1; Provider/hangar realization; `Store` generations + `JetOS.rs` activation;
U22 GC roots; U28 sudo/no-daemon; U29 offline; U13 secrets; OS-VM harness.

---

## OS3 — lift (adopt a running machine)

Read a running (or foreign-managed) machine's state — installed packages,
enabled services, hostname, users, filesystems, `/etc` highlights — and emit a
`module system.<name>` that reproduces it, so a NixOS/deb/arch box can be
migrated without a hand rewrite.

```
$ jet os lift @halcyon
  probing running system (read-only) …
  packages: 214 mapped, 6 unmapped (see report)
  services: 18 enabled → systemd units
  wrote system.halcyon.jet  (review the 6 unmapped before switch)
```

Lift is **read-only** on the source machine (same probe discipline as U20
adapters). Encapsulation: a module may mark an option **private**; lift (and any
external module) reading a private option is rejected — `E09xx
option-private-read`. Private-option marker spelling → **ballot D-JPK-OSOPT1**
(same card as the declaration form).

### Exit

External-machine lift produces a module that `jet os check` accepts;
private-option read is rejected with a snapshot.

### Tests

`lift_reproduces_fixture_machine` (fixture machine-state → expected module);
`lift_probe_is_readonly`; `private_option_read_rejected_snapshot`;
`unmapped_package_reported_not_silent`.

### Depends on

OS0 (options/encapsulation); OS1 (module emission).

---

## OS4 — std option tree v0

The standard jetos option library — the substance that makes OS0's machinery
useful and discharges the parity baseline. Modules, each declaring options with
type/default/docs/enum:

- **boot** — bootloader (systemd-boot/GRUB enum), kernel selection (incl.
  CachyOS-class custom kernels), initrd, kernel params.
- **fs** — filesystems, mounts, swap, LUKS declaration.
- **net** — hostname, networkmanager/systemd-networkd, firewall, tailscale.
- **users** — `User.{ shell, groups, … }`, one module touching both system and
  user (Home-Manager-parity) scope.
- **services** — systemd unit generation from `Service.{}` records (pipewire,
  openssh, …), sharing the type vocabulary with U12 dev services.
- **desktop** — KDE/Plasma, display manager, Stylix-parity theming, fonts,
  Flatpak, portals.
- **pkgs** — system package set over the core + nix providers (stable/unstable
  pins, overlays-parity, AppImage/Flatpak, later native Jet packages).

```jet
module system.laptop {
    imports: find("./modules")
    boot.loader: .SystemdBoot
    fs.root: { device: "/dev/nvme0n1p2", type: .Ext4 }
    net.hostname: "halcyon"
    packages: [default.[firefox, ghostty, ffmpeg], unstable.zig]
    services: {
        openssh: Service.{ ports: [22] }
        pipewire: Service.{}
        tailscale: Service.{}
    }
    users: {
        nate: User.{ shell: default.fish, groups: [wheel] }
    }
}
```

Std option top-level namespace spellings (`boot`/`fs`/`net`/`users`/`services`/
`desktop`/`pkgs` and the enum values like `.SystemdBoot`, `.Ext4`) →
**ballot D-JPK-OSNS1** (naming menu, not a self-pick).

### Exit

A real laptop boots from `module system.laptop`; every parity-baseline row is
expressible; the HalcyonOmega `/home/nate/nixos` config round-trips through lift
→ check → switch.

### Tests

`parity_baseline_covered` (assert each baseline row has a declaring option);
`laptop_profile_realizes` (VM); `home_manager_dual_scope_module`; golden example
`examples/features/jetos/laptop/` with expected `jet os check` JSON (I5).

### Depends on

OS0–OS2; providers (core + nix, U16/U23); OS-VM.

---

## OS-ISO — installable graphical image

`kind: .Iso` on the image tier (U14/U14a) builds an x86_64 Plasma **Calamares**
installer ISO from a `system.<name>`, native/std-only (no `dd`-a-blob), boots in
QEMU to a graphical installer.

```jet
module image.installer {
    kind: .Iso
    from: system.laptop
    target: linux.x64
}
```

```
$ jet os image installer
  building installer ISO from system.laptop (linux.x64)
  squashfs 1.9 GiB, isolinux + Calamares (Plasma) …
  wrote jetos-installer-x86_64.iso  (sha256 3af9…, reproducible)

$ jet os image installer --run          # QEMU convenience
  booting ISO in QEMU … Calamares reached "Welcome"
```

Calamares theme/branding + ISO product naming → **ballot D-JPK-OSBRAND1**.
Installer default disk layout (GPT + ESP + root fs choice, swap, optional LUKS
prompt) → **ballot D-JPK-OSDISK1**. The installer writes a first
`system.<name>` closure and its bootloader entry (OS2 machinery), so a freshly
installed machine can `jet os switch`/`rollback` on first boot.

### Exit

`jet os image installer` emits a reproducible (same input → same sha256)
x86_64 Plasma Calamares ISO; QEMU boots it to the installer welcome screen.

### Tests

`iso_reproducible_digest`; `iso_boots_to_installer` (OS-VM); `iso_installs_then_
switches` (install into a blank QEMU disk, reboot, `jet os switch`).

### Depends on

U14 image `.Iso` tier + native image builder; OS2 (installed system uses the
generation/bootloader path); OS-VM.

---

## OS-VM — scripted VM harness

The test substrate every stage from OS2 on relies on. A std-only QEMU driver
(shell out to `qemu-system-x86_64`, gated behind a CI feature so it never runs
in the unit suite): boot a disk/ISO, drive it, snapshot, kill mid-op, assert.

Capabilities:

- boot an ISO to the installer; script an unattended install;
- boot an installed disk; run `jet os switch`/`rollback` and assert `current`;
- kill QEMU mid-`switch` (power-cut sim) and assert the prior generation boots;
- capture serial console for golden assertions.

### Exit

One CI job runs **build ISO → boot → install → switch → rollback** end-to-end
and asserts each transition; the power-cut variant boots the prior generation.

### Tests

The harness *is* the test; `os_vm_full_roundtrip` is the gating integration job.
Runs on the U25 Linux lane only, behind a `jetos-vm` CI feature flag.

### Depends on

QEMU present on the CI lane; OS2 activation; OS-ISO.

---

## Dependency edges back to Phase A–C

| jetos stage | Needs |
|---|---|
| OS0 | A3 `module system.laptop` role form · `Merge.rs` · `Comptime` pure-eval |
| OS1 | A2 filename canon · `find()` discovery · A1 dispatch seam |
| OS2 | hangar realize (Provider) · `Store` generations + `JetOS.rs` · U22 GC roots · U28 sudo/no-daemon · U29 offline · U13 secrets |
| OS3 | OS0 encapsulation · U20-style read-only probe discipline |
| OS4 | providers core+nix (U16/U23) · U12 service vocabulary |
| OS-ISO | U14/U14a image `.Iso` + native image builder |
| OS-VM | U25 Linux CI lane · QEMU |

---

## Ballot rows (owner decides; agents do not self-pick — I7)

Every owner-facing surface below is unratified. No spelling reaches a fixture or
golden until its decision ID ratifies.

| id (suggested) | question | options sketch | my rec |
|---|---|---|---|
| **D-JPK-OSOPT1** | How is a typed option declared (type/default/docs/enum) and marked private? | (A) `option net.hostname: Text = "x" doc "…"` keyword form; (B) a `Option.{}` value like other role records; (C) type-only decl, defaults set in a module | **A** — a declaration keyword reads like docs, gives the LSP a clear anchor, and keeps "declare once / set many" visibly distinct from setting a value (beginner-experience #2). Private via an `option … private` modifier. |
| **D-JPK-OSVERB1** | The `jet os` verb set + whether it's `jet os <verb>` (dispatched to a jetos engine) or a bare `jetos` binary. | (A) `jet os switch/build/rollback/generations/check/init/lift/image`; (B) top-level `jet switch/rollback/generations` (vision.md's older spelling); (C) a separate `jetos` binary users invoke directly | **A** — one command users type (U18) with `os` as the tier noun, dispatched across the A1 process seam (D-JPK-DISPATCH1=B). Reconciles vision.md's `jet switch` drift flagged in card #2. |
| **D-JPK-OSHOST1** | How is the target host selected + where is the system config searched? | (A) keep `@host` selector + `~/.jet/config.jet` default; (B) discover `module system.<name>` in the repo, select by bare name `jet os switch laptop`; (C) both — bare name in a repo, `@host` for an explicit path | **C** — bare name matches the role-module discovery canon for the common case; `[path]@host` stays for explicit/remote configs. Retires the required `config.jet` filename. |
| **D-JPK-OSGEN1** | Generation naming UX (the #2 c27 question). | (A) `--name` at switch; (B) separate `jet os rename <gen> <name>`; (C) auto-named with opt-in `--name` override; collision + named/unnamed sort in `generations` | **A + C** — auto-name every generation (timestamp/counter) so none is anonymous, `--name` overrides at switch; `generations` sorts newest-first, shows name-or-auto. No separate rename verb (one path, I8). |
| **D-JPK-OSNS1** | Std option top-level namespace + enum spellings (`boot`/`fs`/`net`/`users`/`services`/`desktop`/`pkgs`; `.SystemdBoot`, `.Ext4`, …). | naming menu — see OS4 | present a full aviation/systems naming menu at decision time; do not self-pick (owner wants rich menus). |
| **D-JPK-OSINIT1** | v1 system-service backend: generate systemd units, or a native Jet supervisor? | (A) systemd unit generation now, native supervisor a later card; (B) native supervisor from v1 | **A** — systemd is the pragmatic parity path and keeps Phase D scoped to the closure/activation problem; a native init is a large separate design. Records intent for the native tier. |
| **D-JPK-OSSECRET1** | Where system secrets live + how recipient keys reach a headless machine; the on-disk story. | (A) encrypted file in the repo, host key at `/etc/jet/host.key`, decrypt into tmpfs at activation; (B) `/etc/jet/secrets/` outside the repo; (C) both, by option | **A** — repo-committed ciphertext (reproducible, reviewable), per-host private key the one out-of-band secret, plaintext only ever in activation tmpfs (U13). |
| **D-JPK-OSBRAND1** | Calamares theme + ISO product/branding name. | naming + theme (logo, colors, welcome copy) | present branding options at decision time; ISO stem `jetos-installer-<arch>.iso` is a provisional default pending this. |
| **D-JPK-OSDISK1** | Installer default disk layout. | (A) GPT + ESP + single ext4 root, optional swap, LUKS prompt off by default; (B) btrfs root w/ subvolumes; (C) guided + manual, default = A | **C/A** — ship a guided default (A) plus manual partitioning; btrfs is an option, not the default (fewer footguns for beginners, #2). |
| **D-JPK-OSDISABLE1** | One-character module-disable marker for `find()` discovery. | (A) filename prefix (e.g. leading `_`); (B) a field in the module; (C) reuse whatever `env.*` `find()` already uses | **C** if `env.*` already has a marker (one path, I8); else **A** — a filename prefix is the "one-character disable" the parity baseline asks for. |
