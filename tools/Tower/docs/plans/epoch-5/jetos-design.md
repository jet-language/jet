# jetos — a declarative OS (Phase 2 design-of-record)

> **STATUS: DESIGN-OF-RECORD — post-v1, do not implement yet.**
> Depends on M12 layer 3 (`jet eval --pure`, sandboxed builds, signed
> caches) on the same store as docs/plans/jetpack-jetos/unified-ecosystem.md (§10: D-PM1…8).
>
> **All D-OS and D-NX decisions ratified 2026-06-17 (owner ballot):**
> D-OS2=B · D-OS3=B · D-OS4=C (priority map; bare assignment = default priority) · D-OS5=A · D-OS6=A · D-NX1=A · D-NX2=A · D-NX3=A · D-NX4=A · D-NX5=deferred to Epoch 3 · D-NX6=A
>
> Agents: do not implement until full ratification of all outstanding decisions and the sequencing prerequisites above are met.
>
> **⚠ Reconciled by `unified-ecosystem.md` (2026-06-16, owner-ratified).** The
> authoring surface is now: explicit **`module name {}`** with **`_`-prefix
> disable** (supersedes **D-OS1** "the file is the module"); reserved namespaces
> **`env`/`system`/`image`** with types `Env`/`System`/`Image` and packages
> `Pkg`; **`find("./modules")`** auto-discovery (generalizes D-OS7); the master
> config is **`config.jet`**, default location **`~/.jet/`** (not `/etc/…`); the
> single store is the **hangar** at `/etc/jet/hangar/`; the merge table lives in
> unified-ecosystem.md §6. The option/guard syntax (D-OS2/3/4) and the rest of
> this file stand until separately revised.
>
> This is the **detailed Phase 2 design** the consolidated plan
> (docs/plans/jetpack-jetos/README.md) sequences and references. Read the
> README first for phase order and owner decision gates, then this file for the
> module-system mechanics, merge rules, diagnostics, invariants, and milestones.
> Per the naming canon, **jetos is Phase 2**, built on the **jetpack** tool
> whose Phase 1 (a Nix-`shell`/`devenv`-class temporary environment) is the
> near-term, buildable-now work.

Audience: the project owner and implementing agents. v1 Jet source-library
package management architecture (D-PM1…8) is in docs/plans/jetpack-jetos/unified-ecosystem.md (§10); the active
Jetpack/JetOS sequencing source is docs/plans/jetpack-jetos/README.md.

---

## 1. What jetos is

NixOS, restated: **your entire operating system is one big package.**
Kernel, drivers, desktop, apps, config files — described in text, built by
the package manager, stored under a fingerprint, activated by flipping a
link. jetos is exactly that, with jetpack as the kitchen and Jet as the
build language.

What that buys a beginner-friendly Mint/CachyOS-style distro:

- **Unbreakable updates.** An update builds the complete new system as a
  new store path FIRST, then atomically re-points one symlink and adds a
  boot-menu entry. Power cut mid-update? You boot the old entry. There is
  no half-updated state to brick.
- **Save-slots for your whole computer.** `jetos rollback` or pick last
  week's generation in the boot menu.
- **Your machine in a text file.** Reinstall = clone repo, run one command.

```
config repo ──jet eval (pure)──▶ merged settings ──▶ ONE giant build
                                                        │ jetpack engine
                                       /etc/jet/hangar/ab12…-jetos-system/
                                                        │ activate
                                     symlink flip + bootloader entry  (atomic)
```

jetos itself is a thin layer: a **module system** that turns many small
config files into one merged settings tree, then hands jetpack one build.

---

## 2. The problem the module system solves

Naive approach: one giant `config.jet`. It works until it's 3,000 lines and
nothing is shareable.

What we want instead (your stated requirements):

1. **Dendritic layout** — files organized by FEATURE, not by machine:
   `firefox.jet` carries everything Firefox-related (the package, system
   policy, the user's prefs) in one liftable file.
2. **Lift-ability** — copy `modules/apps/firefox.jet` from a stranger's
   repo into yours and it just works, or fails with a clear error.
3. **Import-tree** — no import statements to maintain: every file in the
   folder is automatically part of the system.
4. **One way to do things** — no second config mechanism, ever.

The cost of "many files each contribute settings" is that you need RULES
for combining them. Those rules are the module system. That's all it is.

**Analogy:** the option tree is a giant settings panel SCHEMA (every switch
typed, documented, with a default). Modules are plugins that flip switches
on that panel. The merge engine is the referee deciding what happens when
two plugins touch the same switch.

---

## 3. Vocabulary

- **Option** — one declared setting: a path (`sys.desktop.environment`),
  a type, a default, a doc string. Options form a tree.
- **Module** — one `.jet` file that may (a) declare new options and
  (b) assign values to options. Nothing else.
- **Merge** — combining every module's assignments into one final tree.
- **Priority** — `default` < normal < `force`; the tie-breaking ladder.
- **Host** — a machine; `hosts/laptop.jet` is just a module that flips the
  switches for that machine.
- **std option tree** — the options jetos ships (boot, networking, users,
  desktop…), the equivalent of NixOS's built-in options.
- **Lift** — fetch one module file from another repo into yours.

---

## 4. The repo every jetos user has (fixed layout, generated by `jetos init`)

```
~/.jet/                 # default config location (a normal git repo; never force-moved)
  config.jet            # the master jetos config (U10/D-JPK20)
  .jet/lock             # pins std option tree + packages (single lockfile, U2)
  modules/**/*.jet      # feature modules — ALL auto-discovered via find()
  hosts/<name>.jet      # one per machine — also auto-discovered
```

Rules (one way to do things):
- Every `.jet` under those folders is discovered by `find("./modules")` (U4).
  There is no hand-maintained import list.
- Disable a `module name {}` with a leading `_` on the name (`module _draft {}`,
  U3 — supersedes the old "prefix the file" rule).
- Host selection: `--host <name>`, else the machine's hostname, else an
  error listing available hosts.
- File discovery order MUST NOT affect the result (§5 guarantees it;
  agents: shuffle-order golden test required).

---

## 5. The module system, slowly

### 5.1 Declaring an option

```jet
// PROPOSED syntax — gated on Decisions D-OS1..D-OS4 (§9)
option apps.firefox.policies: map<string, json> = {}
  "Enterprise policy JSON merged into Firefox's policies.json"
```

Reading or writing an option nobody declared is an error with did-you-mean:

```
error[J-M010]: unknown option `sys.desktop.enviroment`
  did you mean `sys.desktop.environment`?
  declared in: std/desktop.jet
```

### 5.2 A complete feature module

```jet
// modules/apps/firefox.jet — everything Firefox, in one liftable file
option apps.firefox.policies: map<string, json> = {}
  "Enterprise policy JSON"

when apps.firefox.enable {
    sys.pkgs += [firefox]                      // system: the package
    default sys.desktop.default_browser = "firefox"   // polite suggestion
    user.me.files["~/.mozilla/policies.json"] =        // user: prefs
        json(cfg.apps.firefox.policies)
}
```

`when` is a declarative guard — "in worlds where this option is true,
contribute the following" — not a runtime if. `cfg.…` reads other options'
FINAL merged values (resolved lazily; cycles are detected and reported:
`error[J-M030]: option cycle: a → b → a`).

### 5.3 A host

```jet
// hosts/laptop.jet
host { name: "laptop", arch: x86_64 }

sys.desktop.environment = gnome
apps.firefox.enable = true
apps.steam.enable   = true
```

### 5.4 Merge rules (fixed — memorize this table, it's the whole referee)

| Option type | Two modules assign it… |
|---|---|
| list / map entries | combined (lists concatenate; order = sorted by source path, so results never depend on discovery order) |
| scalar (bool/int/string/enum/pkg) | allowed only at DIFFERENT priorities; same priority ⇒ conflict error |

Priorities — D-OS4=C (ratified 2026-06-17):

- Bare assignment: `x = v` — normal priority (the common case)
- Map form: `x = [default: v]` — "use this unless anyone cares" (modules suggesting)
- Map form: `x = [force: v]` — "end of discussion" (two forces still conflict)
- Mixed: `x = [default: a, force: b]` — suggest `a`, allow callers to override, but `b` wins everywhere

A bare assignment with no map implies `default` priority when used in a module; in a host config it is normal priority. The `force` key always requires the explicit map form.

Worked example:

```jet
// modules/apps/firefox.jet
sys.pkgs += [firefox]

// hosts/laptop.jet
sys.pkgs += [vlc]
// ⇒ merged sys.pkgs = [firefox, vlc]   (lists combine)

// hosts/laptop.jet
sys.desktop.environment = "gnome"

// modules/apps/kde-tools.jet
sys.desktop.environment = "plasma"
// ⇒ error[J-M021]: conflicting values for sys.desktop.environment
//      hosts/laptop.jet:3            = "gnome"
//      modules/apps/kde-tools.jet:7  = "plasma"
//      why: scalar options take one value per priority level
//      fix: use [default: …] in the module, or [force: …] to override

// fix: kde-tools.jet lowers its claim to a suggestion:
sys.desktop.environment = [default: "plasma"]
// ⇒ laptop's normal-priority "gnome" wins. Resolved.

// service with explicit priority map (D-OS4 canonical form):
service sshd {
    priority = [default: 50, force: 100];
}
```

This conflict-instead-of-silent-override behavior is a FEATURE: it is the
moment a lifted module and your setup disagree, surfaced loudly with file
and line, in the docs/spec/diagnostics.md voice.

### 5.5 Why modules may not import each other (the liftability law)

If `firefox.jet` could `import "../desktop_helpers.jet"`, copying it to
another repo breaks. So: **modules communicate ONLY through options**
(OS-I2 below). A module's full interface = options it declares + std
options it touches. That interface is statically checkable — which is what
makes `jetos lift` safe:

```
$ jetos lift github:alice/jetos#modules/apps/zellij.jet
  reads 3 std options ✓ · declares 1 new (apps.zellij.layout) ✓
  → modules/apps/zellij.jet      run `jetos switch` to apply

# and the failure mode:
error[J-M040]: lifted module reads undeclared option `alice.theme.accent`
  that option is private to its source repo
  fix: also lift modules/theme.jet (declares it), or remove the read
```

### 5.6 Scopes

- `sys.*` — the machine: boot, kernel, networking, services, pkgs, desktop
- `user.<name>.*` — per-user files/pkgs/prefs (the home-manager role);
  `user.me` = the primary user (D-OS6)
- `apps.*` — feature toggles and per-app settings; the lifting layer

D-OS6=A (ratified 2026-06-17) — user scope examples:

```jet
// primary user (stable alias — no rename needed when sharing configs):
user.me {
    shell = "fish";
    pkgs  += [neovim, ripgrep];
}

// additional named user:
user.alice {
    shell = "zsh";
    home_manager = {
        programs.git.enable = true;
    };
}
```

`user.me` is a well-known name, not a runtime concept — it resolves to the
config owner at build time. Adding a second user (`user.bob { … }`) is purely
additive and does not require restructuring the file.

One feature file touching all three scopes is the dendritic payoff.

---

## 6. A day with jetos (CLI surface — complete)

```
$ jetos init                      # generates the layout in §4
$ jetos check                     # eval + merge + typecheck, NO side effects
                                  #   = CI for your config repo
$ jetos diff                      # what would change, before you commit
  Δ +ripgrep, firefox 126→127, sys.desktop.environment gnome
$ jetos switch                    # build new system in store, flip, add boot entry
  41 substituted ↓ · 2 built ⚒ · generation 23 → 24
$ jetos rollback                  # instant; or pick gen 23 in the boot menu
$ jetos switch --as-of 2026-03-01 # rebuild March's world (lockfile pin)
$ jetos lift <source>#<path>      # adopt someone's feature module
```

Nothing else. No channels, no imperative install command that bypasses the
config, no second tool.

---

## 7. Architecture: jetos is small on purpose

```
modules/ + hosts/ ─▶ jet eval --pure (per file)
                  ─▶ option registry + MERGE ENGINE   (the only new code)
                  ─▶ merged tree (canonical JSON)
                  ─▶ build generator: tree → one jetpack system build
                  ─▶ jetpack engine (JP1–JP5): store, cache, generations
                  ─▶ activation: symlink flip + bootloader entries
```

New code = merge engine, build generator, activation scripts, std option
tree. Everything hard (hashing, sandbox, caches, rollback storage) is
jetpack's job and is already specified there. The store is the single global
hangar at `/etc/jet/hangar/` (U2).

---

## 8. Invariants and milestones

Invariants (extend compiler I1–I8):
- **OS-I1** One way to do things: no manual imports, no alternate config
  mechanism, no per-repo helper libraries — std prelude only.
- **OS-I2** Modules communicate only through declared options.
- **OS-I3** Config evaluation is pure (`jet eval --pure`, S60).
- **OS-I4** Merge semantics fixed; exactly three priorities; no numeric
  override levels, ever.
- **OS-I5** Every J-M diagnostic: what/why/fix + snapshot (compiler I4).
- **OS-I6** `switch` requires a lockfile; the std option tree is versioned
  and pinned in it.

| MS | Needs | Build | Exit criteria |
|----|-------|-------|---------------|
| OS0 | M12 layer 3 + S60 | option registry + merge engine + J-M01x/02x/03x, driven through builtins (no new syntax yet) | golden: 3 modules merge to canonical JSON; conflict + cycle snapshots; shuffle-order determinism test |
| OS1 | OS0 | import-tree, `_` skip, host selection, `jetos check`, `jetos init` | example repo (2 hosts, 5 modules) → identical JSON regardless of discovery order |
| OS2 | layer 3 builds | build generator + activation (symlink, bootloader) + `switch/diff/rollback` | VM test: switch → rollback round-trip; power-cut simulation boots old generation |
| OS3 | OS1 | `lift` + module registry + J-M040 | lift from URL succeeds; private-option read rejected (snapshot) |
| OS4 | OS2 | std option tree v0: boot, networking, users, GNOME desktop (default; KDE Plasma #2, Cinnamon #3 follow as alternate modules) | a real machine boots entirely from hosts/laptop.jet |

Syntax-gated work (`option/when/force/default` parsing) lands only after §9
ratification; OS0–OS1 proceed without it via builtin-call form.

Agent guardrails: compiler I4 diagnostics for all J-M codes; never add a
config escape hatch "temporarily"; typed `sys.files[path]` + one narrow
sandboxed activation verb are the honest mitigations (see D-NX review notes).

---

## 9. Decisions — ratified and open

`D-OS1` (module file shape) is superseded by U3 — modules are explicit
`module name { }`, disabled with a leading `_`. `D-OS7` (entrypoint) is
superseded by U4 — `find("./modules")` auto-discovery. The D-NX rows are
prerequisited on D-PM1…8 (docs/plans/jetpack-jetos/unified-ecosystem.md §10) and M12.

### Config surface (D-OS2…6) — ratified 2026-06-17

| ID | Decision | Ratified |
|---|---|---|
| D-OS2 | Option declaration syntax | ✅ **B** — one `options: { … }` record per file; all options grouped under a single header block |
| D-OS3 | Guard keyword | ✅ **B** — reuse `if` for declarative guards; familiar keyword (note: jetos `if` is always a declarative merge-time guard, not a runtime branch) |
| D-OS4 | Priorities syntax | ✅ **C** — priority map: `sys.desktop.default_browser = [default: firefox, force: qutebrowser]`. A bare assignment with no map (e.g. `sys.desktop.default_browser = firefox`) implicitly uses `default` priority. `force` requires the explicit map form. |
| D-OS5 | Per-app enable flags | ✅ **A** — automatic enable flag; `apps.steam.enable: bool = false` is implicit; no `mkEnableOption` boilerplate |
| D-OS6 | User scope | ✅ **A** — `user.<name>.*` namespace; `user.me` alias for the primary user; adding a second user is additive with no restructure |

### Platform (D-NX1…6) — ratified 2026-06-17

| ID | Decision | Ratified |
|---|---|---|
| D-NX1 | Bootstrapping ~10k packages | ✅ **A** — tap `cache.nixos.org` as compatibility provider (read-only); hand-write native Jet builds only for the spine; measure migration over time |
| D-NX2 | What `jetos add` edits | ✅ **A** — edit the active (host) config file; auto-commit with `--no-commit` to stage only; orchestration/implementation refinement deferred |
| D-NX3 | Ephemeral `jetos try` | ✅ **A** — shell-scoped; nothing recorded; gone on exit; config untouched |
| D-NX4 | NixOS migration path | ✅ **A** — reporter, not auto-converter; checklist of what maps cleanly and what needs hand-porting |
| D-NX5 | v0 reference image | **Deferred to Epoch 3** |
| D-NX6 | Option-schema bootstrap | ✅ **A** — hand-write spine; JSON pass-through for tails (`sys.services.nginx.extraConfig = json({…})`) |

**Imperative front door (headline feature):** `jetos add/set/remove` edit
declarative config files only — never a second install database. Drift is
structurally impossible. `jetos try` serves ephemeral needs honestly.

**Import model reconciliation:** `use jetpack.firefox as firefox` binds
a local alias to the global `apps.firefox` option subtree (D-S16-USE — `use`
replaced `import`; reserved `jetpack`/`jetos` roots) — satisfies OS-I2
liftability.

**Dependency chain (post-v1):** M14 v1 → M12 store → layer 3
(`jet eval --pure`, sandbox, caches) → module system (OS0) →
`jetos switch` on VM (OS2) → imperative layer + reference image (OS4–5).
