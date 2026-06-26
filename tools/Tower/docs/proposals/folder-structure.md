# Proposal: simpler, consistent top-level folder layout

Status: PROPOSE-ONLY (owner approval required). Nothing here is executed.
Scope: the monorepo's top level + workspace crate placement + stray-file
cleanup. Explicitly **out of scope**: anything inside `tools/Tower/` or
`tools/Tower-v2/` (v2 rebuild in flight — see the single coordination item
at the end).

## The binding constraint

The layout is pinned by hardcoded paths, not aesthetics. Any move that
crosses one of these needs a coordinated code edit; any move that doesn't is
free. This partitions the whole problem:

**Frozen by tests / embeds — do not move without editing the referencing code:**
- Golden + capstone + showcase tests freeze: `examples/features/`,
  `examples/features/expected/`, `examples/capstone/logbook/`,
  `examples/showcase/expected/`, and `stdlibs/jet.archive`
  (`tests/golden.rs`, `tests/capstone.rs`, `tests/cross.rs`).
- `include_str!` embeds freeze, relative to their crate:
  - `Source/main.rs` → `../examples/features/16_wordcount.jet`
  - `Source/Explain.rs` → `../docs/spec/diagnostics.md`
  - `crates/jet-codegen/src/Codegen/mod.rs` → `../Prelude/{Core,CoreLib,Mem}.rs`
  - `crates/jet-driver/src/FFI.rs` → `Prelude/{Regex,Archive,Db}.rs`

So `examples/`, `docs/spec/`, and `stdlibs/` **stay where they are**. They
are test-pinned, and `stdlibs/` is Jet-package source (its own `pkg.jet` +
nested Cargo packages), not a Rust workspace crate — it does not belong under
`crates/`.

`flake.nix` does **not** pin crate paths (only a help-text echo mentions
`Source`); the Nix build is `cargo build` over the workspace, so crate moves
that update `Cargo.toml` need no flake edit.

## Current top-level (verified)

```
Source/            root `jet` package source: lib.rs, main.rs, Bin/Jetpack.rs (2nd bin),
                   LSP/, Publish/, ~24 PascalCase .rs files. Path-referenced as the
                   root package's lib+bins in /Cargo.toml.
crates/            7 seam crates: jet-foundation jet-lexer jet-parser jet-comptime
                   jet-sema jet-codegen jet-driver
jet-jit/           workspace crate (loose at root, NOT under crates/)
jet-net/           workspace crate (loose at root, NOT under crates/)
stdlibs/           jet.archive ring-stdlib package source (Jet + nested Cargo)
docs/              spec/, reference/ (reference/errors/)
examples/          ~211 .jet across showcase/features/capstone/graphics/jetpack*/
                   simple_exec/workspace/  + one loose `test.jet`
tests/             ~50 .rs + fixtures/ cli/ dev/ lsp/ observe/ release/ repl/ ui/ ui_lint/
tools/             perf/, Tower/, Tower-v2/
editors/           jet.tmGrammar/, tree-sitter/, vscode/, zed/
assets/
build/             UNTRACKED compiler output (gitignored, physically present)
target/            UNTRACKED Cargo output (gitignored)
flake.nix flake.lock Cargo.toml Cargo.lock .gitignore
README.md  CLAUDE.md  LICENSE  cleanup-overhaul-prompt.md (stray scratch)
```

The two real inconsistencies:
1. **Workspace members live in two places.** 7 in `crates/`, 2 loose at root
   (`jet-jit`, `jet-net`), 1 as `Source/`. A reader can't find "all the
   crates" in one spot.
2. **`Source/` is the odd name out.** The internal file style (PascalCase,
   nested module dirs) is already identical across `Source/` and every
   `crates/jet-*` (e.g. `jet-codegen/src/Codegen/mod.rs`,
   `jet-driver/src/FFI.rs`). The only divergence is the top-level *dir name*:
   `Source` vs `jet-*`. Casing of files is a non-issue.

## Target layout

Principle: **`crates/` is the single home of every Rust workspace member;
everything else at root is a distinct, named part of the monorepo.**

```
crates/
  jet-foundation/ jet-lexer/ jet-parser/ jet-comptime/
  jet-sema/ jet-codegen/ jet-driver/
  jet-jit/         ← moved from /jet-jit
  jet-net/         ← moved from /jet-net
  jet/             ← (OWNER QUESTION 1) the root app package, moved from /Source
stdlibs/   docs/   examples/   tests/   tools/   editors/   assets/
flake.nix  Cargo.toml  README.md  CLAUDE.md  LICENSE
```

Done in two stages by risk:

- **Stage A — safe zone (recommend now).** Move `jet-jit/` and `jet-net/`
  into `crates/`, plus scratch-file cleanup. Touches **zero** `include_str!`
  and zero test paths — only 3 `Cargo.toml` lines.
- **Stage B — coordinated (OWNER QUESTION 1).** Move `Source/` → `crates/jet/`.
  Touches only `include_str!` relative paths + the bin/lib `path =` lines in
  `/Cargo.toml`. Recommended end-state, but gated on the owner because it is
  the high-touch move and concerns the app-vs-crate framing.

## Move map

| # | From | To | Rationale | Risk / required edits |
|---|------|----|-----------|-----------------------|
| 1 | `jet-jit/` | `crates/jet-jit/` | All workspace members under `crates/`. | LOW. Edit `/Cargo.toml` `members` (`"jet-jit"`→`"crates/jet-jit"`) and root dep (`jet-jit = { path = "jet-jit" }`→`"crates/jet-jit"`). No `include_str!`, no test path. |
| 2 | `jet-net/` | `crates/jet-net/` | Same. | LOW. Edit `/Cargo.toml` `members`, and `crates/jet-comptime/Cargo.toml` dep `path = "../../jet-net"`→`"../jet-net"`. No `include_str!`, no test path. |
| 3 | `cleanup-overhaul-prompt.md` (root) | delete | Scratch/working prompt, not project doc. | LOW. Confirm owner doesn't still want it; if keep, move to `tools/` not root. |
| 4 | `examples/test.jet` (loose) | delete or → `examples/features/` | Only loose `.jet` at the `examples/` root; breaks the subdir convention. | LOW-MED. `Source/main.rs:786` *mentions* `examples/test` in a comment illustrating ext-optional resolution — verify nothing demos against it before deleting. Safest: relocate + update that comment, or keep as the documented ext-resolution demo. |
| 5 | `Source/` | `crates/jet/` | Single crate home; removes the one odd top-level dir name. | **HIGH (gated, OWNER QUESTION 1).** Edit every `include_str!` in `Source/` (`../examples/...`, `../docs/spec/diagnostics.md`) for the new depth; edit `/Cargo.toml` `[lib] path`, both `[[bin]] path` (`jet`, `jetpack`), and add to `members`. No file renames (style already matches). Re-run full suite. |

Non-moves worth stating explicitly (so the owner sees they were considered):
- `stdlibs/` — stays. Test-pinned (`stdlibs/jet.archive`) and it's Jet-package
  source, not a Rust crate.
- `examples/`, `docs/spec/` — stay. Frozen by golden tests + `include_str!`.
- `editors/`, `tools/perf/`, `assets/`, `tests/` — stay. No simplification win;
  `editors/install.sh` scripts assume their current relative paths.

## Stray / artifact files

| Path | State | Recommendation |
|------|-------|----------------|
| `cleanup-overhaul-prompt.md` (root) | tracked? scratch prompt | delete (move 3) — not a durable doc. |
| `examples/test.jet` | loose example | relocate/delete (move 4) — confirm the main.rs comment first. |
| `build/` (root) | untracked, gitignored, physically present | `rm -rf build/` to declutter the tree; already ignored, regenerates. |
| `target/`, `result` | untracked, gitignored | fine, leave. |
| `examples/workspace/.jet/workspace.lock` | **tracked**, header says "generated by jetpack; do not edit" | **OWNER QUESTION 2.** It's a generated artifact that's committed. `tests/workspace.rs` reads `workspace.jet` and builds the plan programmatically — it does **not** read the `.lock` — so gitignoring it is test-safe. Recommend: gitignore `examples/**/.jet/workspace.lock` and stop tracking, unless it's intentionally committed as a "what jetpack produces" reference fixture. |

Note: `.gitignore` already covers `build/`, `/target`, `**/.jet/clinks/`,
`/result`. Adding `**/.jet/workspace.lock` (pending OWNER QUESTION 2) would
keep generated lockfiles out of the tree consistently.

## Owner questions (genuine forks — not guessing)

**OWNER QUESTION 1 — move `Source/` into `crates/jet/`?**
Pro: one place for every workspace member; kills the only inconsistent
top-level name; the app becomes a normal crate. Con: highest-touch move
(every `Source/` `include_str!` + the bin/lib paths). It does **not** require
any file rename — internal PascalCase style already matches the other crates.
- (a) Yes — `crates/jet/` (recommended end-state; do after Stage A).
- (b) Keep `Source/` as the root app package (an "app at root, libraries in
  `crates/`" split), do only Stage A. Lower risk, leaves the name divergence.
Recommendation: (a), sequenced after Stage A lands and is green.

**OWNER QUESTION 2 — `examples/workspace/.jet/workspace.lock`: gitignore or
keep committed?** Test-safe to gitignore (no test reads it). Keep only if it's
deliberately a committed "this is what jetpack emits" fixture.

## One coordination item (not for this proposal to resolve)

**Tower v1 / v2 consolidation.** `tools/Tower/` (retiring archive) and
`tools/Tower-v2/` (live rebuild) coexist under `tools/`. Restructuring either
is out of scope here. Flagging it as the single open coordination item: once
v2 is the sole live PM tool, decide whether v1 becomes `tools/Tower-archive/`,
moves out of the repo, or is deleted — owner's call, separately.
