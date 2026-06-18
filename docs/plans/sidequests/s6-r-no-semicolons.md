# S6-R — No visible semicolons

**Status: ratified 2026-06-18 (option B — Go-style lexer insertion)** — recorded
in `syntax-decisions.md` (supersedes S6's required-`;` rule); ready to implement.

No `;` in source. The lexer inserts a synthetic terminator at line ends; the
grammar and diagnostics stay terminator-based. This is a large, cross-cutting
change — it touches every example and many ui snapshots.

## Plan

1. **Lexer** (`src/lexer.rs`) — after emitting each token, if the line ends and
   the last token is in the statement-ending set (identifier, literal, `break`,
   `continue`, `return`, `)`, `]`, `}`), emit a synthetic `Semicolon` before the
   newline is consumed. Identical to Go's rule.
2. **Gotcha — trailing `->` in return types.** A newline before `->` would
   insert `;` after `)`. Fix: `->` and `{` do **not** trigger insertion (add to
   the non-inserting set), and require `-> Type` and `{` to stay on the
   parameter-close line (already S44 house style). Emit a targeted
   `E_UNEXPECTED_TOKEN` ("`->` must stay on the closing-`)` line") for the
   broken multi-line form.
2a. **Continuation suppression (ratified 2026-06-18).** Before inserting a
   terminator, **peek the next non-blank line**: if its first token is `.`
   (continues an S69 method/field chain) or a binary/logical operator (`&&`,
   `||`, `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`), **do not
   insert**. This keeps S69 dot-chains (`examples/features/38_method_chain.jet`)
   and line-broken expressions parsing. This lookahead is the only nontrivial
   part of the lexer change — test it directly (chain, broken boolean, broken
   arithmetic, and the negative case where a genuine statement follows).
3. **Parser** — no changes; the synthetic `;` is transparent.
4. **Diagnostics** — retire `E_MISSING_SEMI` (the lexer now handles insertion).
   New code **`E0986`**: `-> Type`/`{` split from the closing `)` (the one layout
   error). Claim in `docs/spec/diagnostics.md` (I4) with a ui snapshot.
5. **Teach `jet fmt`** (`src/fmt.rs`) to emit no `;` (strip them on output). The
   example corpus is migrated mechanically in the shared final consolidation
   phase by running `jet fmt` over `examples/` and re-blessing snapshots once —
   not hand-stripped here.
6. **`src/syntax.rs`** — the `KW_SEMICOLON` constant stays (the token still
   exists internally); update its doc comment to cite S6-R (lexer-inserted, not
   user-typed).
7. **`syntax-decisions.md`** — S6 body and S44 already updated to reflect S6-R;
   confirm no other doc still claims "semicolons required."

## Sequencing note

S6-R, D-BIND1, and D-IF1 all change how every example is written. Implement the
grammar/lexer/fmt + new tests for each, then migrate the whole corpus once via a
`jet fmt` pass in the final consolidation phase. D-IF1's arm termination depends
on S6-R, so land S6-R before D-IF1.
