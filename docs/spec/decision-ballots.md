# Decision ballots — open owner queue

Every decision waiting on you, in one place. Ratified decisions live only in
[`syntax-decisions.md`](syntax-decisions.md); when you pick one here, it moves
there and leaves this file.

**To decide:** for each row, pick an option (or "Rec" to take the
recommendation). Each ballot names the plan it unblocks.

---

## 1. jetos config surface — syntax (blocks the module system)

These five pick the authoring syntax for jetos config files. Nothing in the
jetos module system gets implemented until they land. Worked examples below
each table. Plan: [`jetos-design.md`](../plans/jetpack-jetos/jetos-design.md).

| ID | Question | A (recommended) | B |
|----|----------|-----------------|---|
| D-OS2 | How you declare an option | `option a.b.c: T = default "doc"` one-liners | one `options: { … }` record per file |
| D-OS3 | Guard keyword | `when expr { }` (declarative, distinct from runtime `if`) | reuse `if` |
| D-OS4 | Priorities | prefix keywords: `default x = v`, `force x = v` | call-style: `x = default(v)` |
| D-OS5 | Per-app enable flags | auto: every `modules/apps/<n>.jet` gets `apps.<n>.enable: bool = false` free | each module hand-declares its enable |
| D-OS6 | User scope | `user.<name>.*` with `user.me` alias (multi-user ready) | single-user `home.*` |

Worked example of option A everywhere (one feature file, all three scopes):

```jet
// modules/apps/firefox.jet
option apps.firefox.policies: map<string, json> = {}
  "Enterprise policy JSON merged into policies.json"

when apps.firefox.enable {            // D-OS3 A: declarative guard
    sys.pkgs += [firefox]
    default sys.desktop.default_browser = "firefox"   // D-OS4 A: priority prefix
    user.me.files["~/.mozilla/policies.json"] =        // D-OS6 A: user.me alias
        json(cfg.apps.firefox.policies)
}
```

---

## 2. jetos platform & product (post-v1; recs from the Nix-replacement review)

Direction calls for jetos itself. All recommended **A** unless noted. These are
post-v1 and depend on M16 (pure eval + sandbox + signed cache); confirming them
now just locks the direction.

| ID | Question | Recommendation |
|----|----------|----------------|
| D-NX1 | Bootstrap 10k+ packages | tap cache.nixos.org read-only; native Jet recipes for the spine; measure migration |
| D-NX2 | What `jetos add` edits | the host file (smallest edit); auto-commit with `--no-commit` escape; `--host` for fleets |
| D-NX3 | Ephemeral `jetos try` | shell-scoped only; nothing recorded |
| D-NX4 | NixOS migration path | reporter/checklist, not an automatic converter |
| D-NX5 | v0 reference image | **one reference desktop image (x86_64).** Default desktop **GNOME**; KDE Plasma second, Cinnamon third (see below) |
| D-NX6 | Option-schema bootstrap | hand-write spine option modules; `JSON` pass-through for vendor config tails |

**Default desktop environment (owner direction):** GNOME is the default. KDE
Plasma is the second target, Cinnamon the third. The v0 image ships GNOME; KDE
and Cinnamon follow as alternate desktop modules.

---

## 3. Epoch 2 milestone ballots (open)

One row each, grouped by milestone. "Rec" is the agents' recommendation; pick it
or override. Detail lives in each milestone plan under `docs/plans/epoch-2/`.

| ID | Milestone | Question | Rec |
|----|-----------|----------|-----|
| D-REF2 | M5 references | Ship arena/owner patterns this milestone? | only if the parser example needs them |
| D-LIB1 | M6 libraries | Ship S61 (labels/defaults) + S62 (delegation) together in M6? | yes, both |
| D-LIB2 | M6 libraries | Generics step | associated types + default method bodies |
| D-JSON1 | M6 libraries | JSON decode strictness baseline | _(no rec — needs your call)_ |
| D-IO2 | M7 streaming I/O | Cleanup surface | RAII handle types (S63), drop on scope exit |
| D-PKGS4 | M8 packages | Yank / immutability rules | immutable releases; yank hides from new solves _(wants brief discussion)_ |
| D-NET1 | M10 services | TLS/HTTP dependency | rustls-class via the FFI tier, never hand-rolled |
| D-TEST1 | M11 testing | Property testing | in, only if a small shrinking design exists |
| D-TOOL2 (=D-TEST2) | M11 testing | `todo` typed-hole expression | defer unless the design is small |
| D-TOOL5 | M11 testing | Per-build capability summary | _(no rec — needs your call)_ |
| D-OBS3 | M12 observe | Metrics conventions | structured logs first; OTel-aligned metrics later |
| D-CROSS2 | M15 cross | Freestanding panic strategy | abort by default |
| D-CROSS3 | M15 cross | Embedded smoke test | documented local harness minimum |

---

## 4. REPL refinements (M18 — not yet decision IDs)

Small calls left from the REPL design. Pick a default for each; promote to a
ballot ID only if it touches syntax or a public CLI contract.

1. **Fuel / timeout** — cap interpreter steps per input to stop accidental
   infinite loops in demos? (Rec: yes, like comptime fuel.)
2. **Startup banner** — show Jet version + `type :help`, or stay silent?
3. **Color** — respect `NO_COLOR`/`CLICOLOR` like other `jet` commands? (Rec: yes.)
4. **Std preload** — implicit `use std.io;` or require explicit imports?

---

## Recently ratified (2026-06-17)

- **D-DEV4** — `jet dev` = the watch/interpret loop; `jet env` = drop into the
  `env.jet` dev shell. Recorded in `syntax-decisions.md`.

## Recently ratified (2026-06-16)

Recorded in `syntax-decisions.md` — not duplicated here:

- **U11–U18** — jetpack/jetos typed surface (`System`/`Image`/`Service`,
  `jetpack os`, library `use`, inferred constructors)
- **D-CBIND2/3/5/6, D-LL2** — C FFI bind timing/engine/strings/macros; `@audit`
  on `@unsafe`
- **S75/S76** — fan-out `f.[…]` and fixed-size lists `[T#N]`
- **D-REPL1…21** — terminal REPL (M18); see `m18-repl.md`
