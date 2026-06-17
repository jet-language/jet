# E2-M18 — Interactive REPL (`jet repl`)

**Status:** **all owner decisions D-REPL1…D-REPL21 ratified 2026-06-16** (see
the decision tables below; ratified picks recorded in docs/spec/syntax-decisions.md).
Ready for the detailed implementation pass when E2-M18 is scheduled — the M4
interpreter (D-REPL4=A backend) must be green first. D-REPL14 carries an owner
refinement (prompt-based compile-or-file fallback) — see its row.

**Depends on:** E2-M4 (`jet dev` interpreter — whole-program tree-walker
extended from M9.5 comptime), E2-M3 (CLI polish, `--json` patterns). Soft
dependency on M13 LSP infrastructure if D-REPL11 chooses completion
integration.

**Error codes:** E18xx block (claim in docs/spec/diagnostics.md as implemented).

---

## Goal

Give beginners and library authors a **zero-ceremony way to try Jet** without
creating a file: type a snippet, see the result, learn from the same
diagnostics the batch compiler uses. The REPL is a teaching surface first;
release builds still go through `jet build` / rustc only.

**Non-goals until owner says otherwise:** replacing `jet run` for real
programs, benchmarking interpreted performance, or executing untrusted code
without an explicit sandbox story (web playground is a separate decision
gate).

---

## Why this is its own milestone (not folded into E2-M4)

E2-M4 (`jet dev`) optimizes **file-based** iteration: watch the import graph,
re-check on save, re-run the entry file. A REPL optimizes **line-based**
exploration: persistent bindings, incremental input, meta-commands, and
ownership semantics across prior inputs. Both can share the interpreter backend,
but the session model, UX, and test harness differ enough to plan separately.

---

## Owner decisions — ratify before any code

Recommendations are marked **Rec**; agents must not substitute if the owner
picks another option.

### Product and placement

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL1 | Ship a terminal REPL in Epoch 2? | **A** — yes, E2-M18 after E2-M4 · **B** — defer entire REPL to Epoch 3 · **C** — no terminal REPL; web playground only (D-REPL19) | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL2 | Web playground in scope for this milestone? | **A** — terminal only; playground is a later milestone · **B** — design terminal REPL now, playground spec stub only · **C** — terminal + playground ship together | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL21 | Milestone timing vs E2-M4 | **A** — separate E2-M18 after M4 interpreter is green · **B** — thin `jet repl` (expressions only) ships inside M4; M18 expands · **C** — defer all REPL work to Epoch 3 | **A** | ✅ ratified 2026-06-16 — A |

### Command surface

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL3 | How users start a session | **A** — explicit `jet repl` only · **B** — also `jet` with no args in a TTY (like `python`) · **C** — `jet repl` plus `jet repl <file.jet>` to seed a session from a file | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL12 | Relation to `jet eval --pure` (S60) | **A** — separate commands; REPL is impure-by-default · **B** — `jet repl --pure` runs a restricted pure subset · **C** — no REPL; only `jet eval --pure` for config (E2-M16) | **A** | ✅ ratified 2026-06-16 — A&B |
| D-REPL13 | Relation to `jet dev` | **A** — independent processes; share interpreter library only · **B** — `jet dev --repl` flag in the watch server · **C** — REPL is a mode inside the dev server (one long-lived binary) | **A** | ✅ ratified 2026-06-16 — A |

### Execution and semantics

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL4 | Execution backend | **A** — interpreter only (E2-M4); plain message when unsupported · **B** — compile+run each input via rustc (slow, full semantics) · **C** — hybrid: interpreter first, auto-fallback compile with user-visible warning | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL5 | What users can type | **A** — expressions and top-level statements (`val`, `var`, `print`, control flow) · **B** — also item declarations (`fn`, `struct`, `enum`, `trait`) · **C** — expressions only (everything else is `:load`) | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL6 | Hard rejects (must fail with a teaching message + workaround) | **A** — FFI, tasks/channels, `unsafe`/low-level gates, and anything M4 marks native-only · **B** — same as A plus package imports from `jet.toml` until resolver story is clear · **C** — allow imports/deps; REPL is a full project shell | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL14 | If a snippet needs native code | **A** — reject with "run `jet run` / `jet build` instead" · **B** — offer one-shot compile-and-run of the current session module to a temp binary | **A** | ✅ ratified 2026-06-16 — **owner refinement (prompt-based):** the REPL *prompts* the user — offer to compile & run the snippet (B's magic); if they decline, reject it and write it to a file, then point at `jet run` (A's safety + teaches the file→`jet run` path). REPL-I4 still holds (no release artifacts; temp/working-dir file only). |

### Session and ownership

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL7 | Session persistence model | **A** — one accumulating module; later inputs see earlier bindings · **B** — notebook cells (`:cell`); each cell isolated unless wired · **C** — A default, optional `:cell` mode | **C** | ✅ ratified 2026-06-16 — C |
| D-REPL8 | Ownership across inputs | **A** — real move semantics; moved-from bindings error on reuse with E02xx voice · **B** — REPL auto-clones on move to keep bindings usable (differs from batch compiler) · **C** — reject moves in REPL; only `view`/`ref` borrows cross-line | **A** | ✅ ratified 2026-06-16 — A |
| D-REPL9 | Multi-line input | **A** — brace/paren/bracket counting + `...` secondary prompt until balanced · **B** — require `;` to submit every fragment (no implicit blocks) · **C** — single-line only; paste multi-line as one submission | **A** | ✅ ratified 2026-06-16 — A |

### Project context

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL10 | `jet.toml` / working directory | **A** — sandboxed std-only unless `--project` points at a manifest · **B** — auto-detect `jet.toml` in cwd and load its import graph · **C** — always sandboxed; no project mode in v1 REPL | **A** | ✅ ratified 2026-06-16 — A (sandbox; note: no `jet.toml` — manifest is now `pack.jet`) |

### Terminal UX

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL11 | Line editor tier (I6) | **A** — std-only: minimal read loop, no history · **B** — owner-approved line-editing crate (history, basic editing) · **C** — B plus LSP-style completions from sema | **B** | ✅ **revised 2026-06-17 → A** (std-only). The compiler stays zero-crate; the REPL is compiler-internal so the D-DEP1 package-wrapping can't apply. Richer editing is a later upgrade that must re-earn an owner crate sign-off. (Was: C, 2026-06-16.) |
| D-REPL18 | If D-REPL11 ≠ A: external crate | **A** — `rustyline` · **B** — `reedline` · **C** — other (owner names crate; needs I6 sign-off) | — | ✅ **revised 2026-06-17 → moot**: D-REPL11 is now A (std-only), so **no external crate** ships with the REPL. (Was: A `rustyline`, 2026-06-16.) |
| D-REPL15 | Meta-commands (`:` commands) | **A** — `:quit` `:reset` only · **B** — A + `:load` `:type` `:help` · **C** — B + `:doc` `:imports` `:emit` (show generated Rust for session) | **B** | ✅ ratified 2026-06-16 — B |
| D-REPL16 | Showing results | **A** — print value of last expression when it has a value; `;` suppresses · **B** — always print type + value (`x: Int = 3`) · **C** — only explicit `print` (no implicit echo) | **A** | ✅ ratified 2026-06-16 — B (with `;` suppression) |
| D-REPL17 | Diagnostic voice | **A** — byte-identical to batch compiler (I4) · **B** — shorter headers; same codes and fix lines · **C** — REPL adds extra "in this session" context line | **A** | ✅ ratified 2026-06-16 — A |

### Web playground (if D-REPL2 ≠ A)

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL19 | Playground architecture | **A** — out of compiler repo; separate service using shared interpreter ABI · **B** — `jet playground serve` in compiler binary · **C** — defer playground indefinitely | **C** | ✅ ratified 2026-06-16 — C |

### Engineering and tests

| ID | Question | Options | Rec | Ratified |
|---|---|---|---|---|
| D-REPL20 | CI testing | **A** — transcript fixtures (`tests/repl/*.txt` → expected stdout/stderr) · **B** — A + scriptedPTY integration tests · **C** — manual smoke only (not acceptable for merge) | **A** | ✅ ratified 2026-06-16 — A |

---

## Invariants (REPL-I1…REPL-I6)

- **REPL-I1** REPL reuses lexer/parser/sema; no second typechecker.
- **REPL-I2** Interpreted evaluation uses the same tree-walker as E2-M4; divergence
  from compiled output is P0 (extend `tests/comptime_diff.rs` pattern).
- **REPL-I3** Diagnostic text for a given span matches batch mode (I4), unless
  D-REPL17 ratifies B/C with explicit snapshot exceptions.
- **REPL-I4** REPL never writes release artifacts; temp compile (if D-REPL14=B)
  uses a cache dir under `.jet/repl/` and documents cleanup.
- **REPL-I5** Session state is in-memory by default; history file location and
  opt-in are owner decisions in the detailed implementation pass.
- **REPL-I6** Every meta-command and hard-reject path has a transcript test.

---

## Scope (after decisions)

Likely in scope once D-REPL1=A and D-REPL21=A:

- `jet repl` subcommand in `src/main.rs` usage text.
- Session module builder: append parsed items, incremental sema, evaluate via
  interpreter.
- Prompt loop with D-REPL9 multi-line rules.
- Meta-commands per D-REPL15.
- Optional `--project <dir>` per D-REPL10.
- Transcript test harness.
- No numbered `examples/features/NN_*.jet` file (the REPL is interactive, not a
  runnable demo); the executable spec is a **documented transcript** checked into
  `tests/repl/basics.txt`.
- Guide chapter stub: "Try Jet in the terminal" (owner approves placement).

## Out of scope (unless owner promotes)

- JIT execution (E2-M4 phase 2).
- Async/tasks in session.
- FFI and `unsafe` gates.
- Package publishing, `jet add`, or lockfile mutation from inside REPL.
- Persistent session save/restore across process restarts (v2 nice-to-have).
- LSP code-lens eval (D-LSP13 — remains `jet dev` / REPL separate).
- Shareable URLs, WASM build, or sandboxed multi-tenant playground (D-REPL19).

---

## Implementation order (post-ratification)

1. Session data structure + incremental sema hook (bindings only, no file graph).
2. Interpreter bridge from E2-M4 with REPL fuel/timeout policy (document limit).
3. Input loop + multi-line reader + meta-command dispatcher.
4. Hard-reject table for D-REPL6 features with E18xx codes.
5. Transcript harness + first fixtures (hello, arithmetic, ownership move,
   moved-from error, `:reset`, unsupported FFI message).
6. Optional line-editor crate (D-REPL11/18) and completion (if C).
7. `--project` loader (D-REPL10) if ratified.
8. Docs: guide section, `jet explain` entries for new E18xx codes.

---

## Exit criteria

- `nix develop -c cargo test` green; `tests/repl/` transcript suite passes.
- `jet repl` starts, accepts `1 + 2`, prints `3` (or per D-REPL16).
- Declaring `val x = 10;` then `x * 2` prints `20` in accumulating mode.
- Moving a binding and reusing it surfaces the same E02xx diagnostic as batch.
- `:load examples/features/01_hello.jet` (if D-REPL15 ≥ B) runs or explains limits.
- FFI/tasks snippet fails with a plain what/why/fix per docs/spec/diagnostics.md.
- Unsupported interpreter features name `jet run` / `jet build` as the fix.
- No new external crates unless D-REPL11/18 ratified.

---

## Additional owner decisions (ratified 2026-06-17)

These were open questions promoted to decisions when the owner balloted them.

| ID | Question | Options | Ratified |
|---|---|---|---|
| D-REPL-FUEL | Fuel / timeout | A — cap per-input at ~10M interpreter steps; `user> :run` to allow unbounded | ✅ ratified 2026-06-17 — A: cap per-input steps; stops infinite loops in demos; `:run` to bypass |
| D-REPL-BANNER | Startup banner | A — show Jet version + "type `:help`" on start | ✅ ratified 2026-06-17 — A: show banner; warm for beginners, points at `:help` |
| D-REPL-COLOR | Color convention | A — respect `NO_COLOR` / `CLICOLOR` (consistent with rest of `jet` CLI) | ✅ ratified 2026-06-17 — A: respect `NO_COLOR`/`CLICOLOR`; `NO_COLOR=1 jet repl` is plain bytes |
| D-REPL-PRELOAD | Std preload | A — implicit `use std.io` so `print` etc. just work | ✅ ratified 2026-06-17 — A with teaching note: auto-import `std.io`, but on first use of an auto-imported symbol the REPL prints a one-line teaching note showing that it was auto-imported and what the explicit import would be. Benefit of magic without hiding the model. |
