# 03 — Architecture

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
| src/syntax.rs  | every user-typeable keyword/sigil    | no                    |
| src/diag.rs    | Span, Diagnostic, rendering          | renders them          |
| src/lexer.rs   | text → tokens                        | yes (E00xx)           |
| src/parser.rs  | tokens → AST, fail-fast              | yes (E00xx)           |
| src/sema.rs    | all semantic checks, collects all    | yes (E01xx, M2: E02xx)|
| src/codegen.rs | AST → Rust text                      | **never**             |
| src/main.rs    | CLI, rustc invocation, ICE policy    | only I/O + ICE        |

## Rules

- **R1 — Codegen is dumb.** No checks, no decisions, no "see if rustc
  accepts it". If codegen needs to know something, sema should have
  established it.
- **R2 — Sema is the gatekeeper.** Any program that passes sema must
  produce Rust that compiles. New language features land as: spec →
  parser → sema checks → codegen → tests, in that order.
- **R3 — Single surface.** User-typeable strings live in src/syntax.rs
  only. Renaming a keyword is a one-file change plus snapshot re-bless.
- **R4 — Spans everywhere.** Any AST node an error might point at carries
  its span. Adding a node without a span is a review-blocker.
- **R5 — ICE policy.** rustc failing on generated code prints the
  internal-compiler-error banner (src/main.rs), exits 101, and is treated
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

## Testing strategy

1. **ui snapshots** (tests/ui.rs): every diagnostic's exact text, pinned.
   The error messages are the product; treat snapshot diffs like UI diffs.
2. **golden examples** (tests/golden.rs): examples/ must front-end-pass,
   contain no `unsafe`, and — when rustc is present — build and print
   exactly examples/expected/*.out.
3. rustc-as-verifier: golden tests assert rustc accepts generated code,
   so a sema soundness hole becomes a loud test failure, not a shipped bug.

## Why transpile to Rust (recorded rationale)

The front end is hand-built either way; only the backend was a choice.
Rust gives: a soundness verifier for our ownership checker (critical when
agents write the compiler), LLVM optimization, cross-compilation, and std
— for free. Known costs, accepted: compile times stack on rustc's;
debuggers show generated Rust until M6+ tooling. Precedent: cfront, Nim,
TypeScript, Gleam.
