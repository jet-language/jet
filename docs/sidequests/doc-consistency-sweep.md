# Doc & path consistency sweep (c138)

Mechanical cleanup — no decision. Code is correct; docs lag. Single source of
truth is `Source/Syntax.rs` + `docs/spec/syntax-decisions.md`.

## 1. `#Audit` → `#Unsafe("reason")` drift (D-UNSAFE2, ratified 2026-06-22)

D-UNSAFE2 folded the audit reason into `#Unsafe("reason")` and **retired the
separate `#Audit("…")` marker (E0055)**. `spec.md` and
`examples/showcase/lowlevel.jet` are already correct; these still teach the
retired two-marker (or older `@unsafe`/`@audit`) form:

- `CLAUDE.md` I1 (~lines 51/53/55) — `@unsafe { … }` / `@unsafe fn` + `@audit("…")`.
  Two generations stale (`@`→`#` was D-ATTR1 2026-06-19; `#Audit` retired by
  D-UNSAFE2). **Owner-owned file — flag, don't edit without owner nod.**
- `README.md:69` and `:41` — FAQ + lowlevel showcase row list `#Audit`.
- `docs/spec/diagnostics.md:599/604/606/608` — fix-text teaches
  `#Audit("…") #Unsafe { … }`. File self-contradicts: E0055 at `:118` already
  says `#Audit` is retired.
- `docs/spec/architecture.md:63-64` — "`#Unsafe { … }` … requires an `#Audit("…")`".
- `docs/spec/roadmap.md:63/128` — E2-M13 highlight lists `#Audit`/`#Unsafe`.

Correct form everywhere: `#Unsafe("why this is safe") { … }` / `#Unsafe("…") fn`;
the reason **is** the argument; no separate audit marker.

## 2. Rules R1–R7 → R1–R10
`CLAUDE.md:14` and `docs/README.md:23` describe `architecture.md` as "rules
R1–R7"; it actually defines **R1–R10** (R8 small binaries, R9 file-is-a-program,
R10 pay-for-what-you-call). (CLAUDE.md is owner-owned — flag.)

## 3. Dead links
- `docs/reference/errors/*.md` (13 files incl. README) footer →
  `../admin/04-diagnostics.md` (nonexistent). Should be `../../spec/diagnostics.md`.
- `docs/spec/roadmap.md:24` and `docs/spec/syntax-decisions.md:888` link
  `decision-ballots.md` at a wrong relative path; ballots live at
  `docs/ballots/decision-ballots.md`.

## 4. Stale `src/` → `Source/` path comments
- `Source/CLI.rs:9` (`src/syntax.rs` → `Source/Syntax.rs`)
- `Source/Prelude/CoreLib.rs:1446` (`src/sha256.rs` → `Source/SHA256.rs`)
- `Source/CBind.rs:11` (`src/cffi.rs` → `Source/CFFI.rs`)
- `Source/Comptime/Methods.rs:67`, `Source/Comptime/mod.rs:85` (`src/…` → `Source/…`)
- `Source/Manifest.rs:3` (`src/jetpack/packmanifest.rs` → current `Source/Jetpack/…`)

(Leave user-facing project-layout paths like `src/main.jet` per D-TGT4 — those
are correct.)

## 5. Example comments teaching retired `when` (code is clean) :: 
- `examples/features/69_distinct_types.jet:2` — `` `Name :: distinct Base` `` →
  `` `Name :: distinct Base` `` (code below already uses `::`).
- `examples/features/75_arena_regions.jet:40` — illustrative `stash :: first` →
  `stash :: first`.
- `examples/features/11_enums.jet:1` — "exhaustive when" reads as the retired
  `when` keyword; reword to "exhaustive matching".

## Done =
All sites use ratified spellings; no dead links; `nix develop -c cargo test`
green (golden examples unaffected — only comments change). For the two
owner-owned files (`CLAUDE.md`, `docs/README.md`), surface the fixes to the owner
rather than editing unilaterally.
