# Sidequest implementation plan — all 7 + D-JPK-FILES prereq

Ratified 2026-06-18. This is the execution order for a single end-to-end pass
(one workflow). Each step: write its failing test/example first → implement
parser → sema → codegen → `jet fmt` → its own tests green → claim its diagnostic
in `diagnostics.md` (I4) → update `spec.md` → **commit**. Invariants I1–I8 hold
throughout; rustc never speaks to users (I2), codegen stays dumb (I3).

## Ordering rationale

The three breaking changes (D-BIND1, S6-R, D-IF1) retire `val`/`var`/`;`/`when`,
which invalidates every existing example the moment they land. So all
green-preserving work goes first; the breaking trio runs back-to-back with a
**known red window** (full example/golden suite expected-red, gate on each
feature's own new tests + `cargo build`); then one **consolidation** phase runs
`jet fmt` over `examples/` to migrate the whole corpus mechanically (owner: fmt,
not by hand), re-blesses all snapshots, and restores full green.

These features share `lexer.rs` / `parser.rs` / `syntax.rs` / `sema.rs` /
`fmt.rs` / `codegen.rs`, so the pass is **sequential, in-place on master** — no
worktrees (owner preference), no parallel edits to shared files.

## Phases

| # | Item | Touches | Diag | Gate |
|---|------|---------|------|------|
| 0 | **D-JPK-FILES** (prereq) | syntax.rs `PAYLOAD_FILE`→`pkg.jet`; loader/manifest/jetpack/publish; new `jetpack.toml` TOML parser; manifest fixtures | E1214 | full green |
| 1 | **ext-optional-cli** | main.rs `resolve_source_path` | — | full green |
| 2 | **S19** finish | lexer/parser `while`/`for` teaching errors; `loop_forms` example | (S14 code) | full green |
| 3 | **D-LABEL1** | parser loop-label + `break/continue @name`; sema label scope; codegen Rust labels | E0987, E0988 | full green |
| 4 | **D-ILE1** | loader infer from `fn main()`; manifest `packages:` kind optional; sema | E0989, E0990 | full green |
| 5 | **D-BIND1** | lexer `::`/`:=`, retire `val`/`var`; parser binding; sema; syntax.rs; fmt | E0985 | own tests + build (corpus red) |
| 6 | **S6-R** | lexer terminator insertion + continuation suppression (`.`/binary-op next line); fmt; syntax.rs | E0986 | own tests + build (corpus red) |
| 7 | **D-IF1** (after S6-R) | parser `if` arm-mode; sema inferred comparator (`subject ==`); `when`→teaching; fmt | E0984 | own tests + build (corpus red) |
| 8 | **Consolidation** | `jet fmt` migrates `examples/` (sigils, no-`;`, `when`→`if`); re-bless ALL ui/golden snapshots; finalize diagnostics.md + spec.md | — | **full green** |

## Per-feature design references (do not re-derive)

- **D-JPK-FILES** → `d-jpk-files-structure.md` + syntax-decisions D-JPK-FILES.
- **ext-cli** → `ext-optional-cli.md`.
- **S19** → `s19-amend-loop-unification.md` + syntax-decisions S19.
- **D-LABEL1** → `d-label1-loop-labels.md` + syntax-decisions D-LABEL1. `@name`
  before `loop`; disambiguate from S82 `@Marker` by the following keyword.
- **D-ILE1** → `d-ile1-implicit-lib-exec.md` + syntax-decisions D-ILE1. Inference
  at the no-`pkg.jet` level and the `packages:` `kind`-optional level.
- **D-BIND1** → `d-bind1-binding-sigils.md` + syntax-decisions S2/D-BIND1.
- **S6-R** → `s6-r-no-semicolons.md` + syntax-decisions S6/S6-R. Continuation:
  next non-blank line starting with `.` or a binary/logical op suppresses
  insertion; `->`/`{` stay on the `)` line.
- **D-IF1** → `d-if1-if-universal.md` + syntax-decisions S68/D-IF1/D-IF2.
  `else` catch-all, braceless arm bodies (block for multi-statement), structural
  bare-value-vs-condition mix.

## Done means

- `nix develop -c cargo test` fully green (after phase 8).
- `nix develop -c jet run examples/features/01_hello.jet` prints `hello, world`.
- Every new diagnostic has a `diagnostics.md` entry + a ui snapshot (I4).
- Every feature has an example + expected output enforced by a golden test (I5).
- `tests/decisions.rs` green (ratified IDs now backed by `syntax.rs` entries).
- One commit per phase; final tree clean.
