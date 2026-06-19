# Architecture

## Pipeline

```
 Jet source (.jet)
        │
        ▼
   lexer.rs ──► tokens (every token has a byte Span)
        │
        ▼
  parser.rs ──► AST                     ┐
        │                               │  the FRONT END owns all
        ▼                               │  semantics and every
   sema.rs ──► checked AST              │  user-facing diagnostic
        │      (M2: + ownership check)  ┘
        ▼
 codegen.rs ──► boring Rust source
        │
        ▼
     rustc  ──► native binary      (verifier + optimizer; never
                                    speaks to users — see R5)
```

## Module map

| File           | Job                                  | May emit diagnostics? |
|----------------|--------------------------------------|-----------------------|
| Source/Syntax.rs  | every user-typeable keyword/sigil    | no                    |
| Source/Diagnostics.rs    | Span, Diagnostic, rendering          | renders them          |
| Source/Lexer.rs   | text → tokens                        | yes (E00xx)           |
| Source/Parser.rs  | tokens → AST, fail-fast              | yes (E00xx)           |
| Source/Sema.rs    | all semantic checks, collects all    | yes (E01xx, M2: E02xx)|
| Source/Codegen.rs | AST → Rust text                      | **never**             |
| Source/main.rs    | CLI, rustc invocation, ICE policy    | only I/O + ICE        |

## Rules

- **R1 — Codegen is dumb.** No checks, no decisions, no "see if rustc
  accepts it". If codegen needs to know something, sema should have
  established it.

  **I1 amendment (D-LL1, ratified 2026-06-16, E2-M13).** I1 originally read
  *"no `unsafe` in the language or generated code, ever (v1)."* The expert
  low-level tier (S58) amends it: generated `unsafe` appears **only** inside
  user-written gated regions — an `@unsafe { … }` block (which requires an
  `@audit("…")` reason, lint L3101) or an `@unsafe fn` contract, both
  unlocked by `use core.mem` — plus vetted std/mem internals. Ordinary,
  memory-safe Jet still emits **zero** `unsafe`; the boundary is enforced by
  sema (E3101/E3102/E3103) and tested in `tests/golden.rs` (every example but
  the audited `48_lowlevel` must contain no `unsafe`, and even there every
  `unsafe` must be a gated `unsafe {`/`unsafe fn` form). Codegen stays dumb:
  it lowers an already-checked `@unsafe` region straight to a Rust `unsafe`
  region and makes no safety decision of its own.
- **R2 — Sema is the gatekeeper.** Any program that passes sema must
  produce Rust that compiles. New language features land as: spec →
  parser → sema checks → codegen → tests, in that order.
- **R3 — Single surface.** User-typeable strings live in Source/Syntax.rs
  only. Renaming a keyword is a one-file change plus snapshot re-bless.
- **R4 — Spans everywhere.** Any AST node an error might point at carries
  its span. Adding a node without a span is a review-blocker.
- **R5 — ICE policy.** rustc failing on generated code prints the
  internal-compiler-error banner (Source/main.rs), exits 101, and is treated
  as a P0 bug. rustc's stderr is shown only inside that banner.
- **R6 — Name mangling.** User identifiers are emitted as `user_<name>`
  (`main` excepted) so user code can never collide with Rust keywords,
  macros, or std items.
- **R7 — Backend is swappable.** Nothing outside codegen.rs and the
  driver may know Rust is the target. Post-v1, a Cranelift or LLVM
  backend replaces codegen.rs without touching the front end.
- **R8 — Small, self-contained binaries.** The driver calls `rustc`
  directly with `strip=symbols` and thin LTO, so the linker keeps only
  what the program uses ("only link what's needed"). Output is one
  self-contained native binary. Floor: Rust's std links a baseline
  (low-hundreds-of-KB), accepted as the cost of a beginner-friendly
  std-backed runtime — we do NOT pursue `no_std` in v1 (it would remove
  the conveniences priority #2 depends on). A size-minimal profile
  (`opt-level="z"`, possibly `panic=abort`) is decision S15, exposed
  later as `jet build --small`; the default leans toward speed.
- **R9 — A file is a complete program.** `jet run foo.jet` compiles and
  runs a single file with no manifest, no project folder, no config.
  The compiler invokes `rustc` on one generated `.rs` file — it never
  creates or requires a Cargo project for user code. Agents must not add
  a mandatory project structure, lockfile, or manifest for users; any
  future multi-file/package story is opt-in and post-v1 (see roadmap).
- **R10 — Std is pay-for-what-you-call.** M10 standard-library modules are
  compiler-known namespaces, but importing them is free. Sema records the
  std helpers that a checked program can call, and codegen emits only those
  helper templates. A program that imports every std module but calls none
  should stay in hello-world size territory.

## Exit codes (stable contract)

The `jet` driver returns one of a small, stable set of exit codes so
scripts and CI can branch on the outcome without parsing output. This
table is pinned by the `tests/cli/` transcripts and is part of the public
contract — codes are never repurposed.

| Code | Meaning | Who produces it |
|------|---------|-----------------|
| `0`   | Success. (Includes the no-args greeting — orientation, not an error.) | driver |
| `1`   | User error: a reported diagnostic, a failed `check`, a missing file, a failed `test`. | driver |
| `2`   | Usage error: unknown subcommand (E2101), unknown/ambiguous flag (E2102), or a missing required argument. | driver |
| `70`  | A built program panicked at runtime (`panic`/`require`, S36). Forwarded through by `jet run`. | the user's program |
| `101` | Internal compiler error (I2/R5): rustc rejected generated code. A P0 bug, never the user's fault. | driver |

Presentation is TTY-aware (E2-M3): color and progress appear only when
the relevant stream is a terminal. `NO_COLOR` and `--color=never` force
plain output; `FORCE_COLOR` and `--color=always` force color; `--color=auto`
(the default) defers to TTY detection. Piped or CI output is always plain,
deterministic, ANSI-free bytes — scripts never parse escape sequences.

## Testing strategy

1. **ui snapshots** (tests/ui.rs): every diagnostic's exact text, pinned.
   The error messages are the product; treat snapshot diffs like UI diffs.
2. **golden examples** (tests/golden.rs): examples/ must front-end-pass,
   contain no `unsafe`, and — when rustc is present — build and print
   exactly examples/features/expected/*.out.
3. rustc-as-verifier: golden tests assert rustc accepts generated code,
   so a sema soundness hole becomes a loud test failure, not a shipped bug.

## Why transpile to Rust (recorded rationale)

The front end is hand-built either way; only the backend was a choice.
Rust gives: a soundness verifier for our ownership checker (critical when
agents write the compiler), LLVM optimization, cross-compilation, and std
— for free. Known costs, accepted: compile times stack on rustc's;
debuggers show generated Rust until M6+ tooling. Precedent: cfront, Nim,
TypeScript, Gleam.
