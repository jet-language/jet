# Zed/tree-sitter grammar modernization (c137)

The Zed extension highlights a **stale dialect** of Jet. Real `.jet` files are
full of `ERROR` nodes and keyword fragments light up inside identifiers
(`format`→`for`, `inner`→`in`, `asset`→`as`, `reference`→`ref`, `mutable`→`mut`,
`usage`→`use`, `tests`→`test`). Two multiplicative root causes.

## Root cause 1 — no keyword extraction (`word:` token)

`editors/tree-sitter/grammar.js` (and its downstream copies
`editors/zed/grammar-repo/grammar.js`, `editors/zed/grammars/jet/grammar.js`)
declare **no `word:` token**. Without `word: $ => $.identifier`, tree-sitter does
no keyword interning: each keyword string (`for`, `in`, `as`, `val`, `ref`,
`mut`, `pub`, `use`, `test`, …) is a free token with no "must be a whole word"
guard. In the ERROR/partial states that dominate live editing, a keyword token
wins against `identifier` for a prefix — the reported bleed.

**Fix:** add `word: $ => $.identifier`.

**Residual subtlety:** a single `word` token covers one lexeme class.
`identifier` is `/[a-z_]…/` and `type_identifier` is `/[A-Z]…/` — disjoint. The
uppercase primitive literals (`Int`, `Float`, `Bool`, `String`, `Char`, `Error`,
`List`, `Map`, `Ptr`) stay un-interned, so `Integer`/`Listener`/`Mapping`/
`Pointer` can still mis-light their prefix. Fold the primitives into
`type_identifier` recognition (or `token(prec(...))` whole-word) rather than bare
string literals.

## Root cause 2 — the grammar is an old dialect (dominant cause)

The grammar models a `;`-terminated, `val`/`switch` language that no longer
exists. Ground truth is `Source/Syntax.rs`. Retire/replace:

**Retired constructs the grammar still defines** (delete):
- `val_stmt: choice("val","var")` — bindings are sigils now: `name @= expr`
  (immutable), `name := expr` (mutable). `val`/`var` are teaching-error-only.
- `switch_stmt`/`switch_arm`/`switch_else` with keyword `"switch"` — the
  multi-way branch is keyword-less multi-arm `if`. **Verify the exact live
  spelling against `Source/Syntax.rs`** (D-IF1 retired `when`; D-IF3/c134 may
  revise the `==` marker) before encoding it; examples are the ground truth
  (`examples/features/71_pattern_matching.jet`).

**Modern constructs real examples use that the grammar can't parse** (add):
- sigil bindings `@=`, `:=` (and destructuring LHS `[a, b] @= …`)
- multi-arm `if subj == { Pattern -> { … } … }` with enum-payload, or-patterns
  `A(x) | B(x)` (D-PATO), range patterns `lo..hi` (D-PATR), wildcard `_`
  (D-PATW), bare-variant arms, `else ->` catch-all
- capability sigils `~T ^T &T *T` in type/expr position, `self`/`~self`
  receivers — **coordinate with c124/c127/c131** (D-CAP7/D-CAP9): sigils are
  partly shipped; keyword retirement of `mut/take/view` is c124 Phase 6. The
  grammar should track the *ratified* sigil model, not the retired keywords.
- fan-out `f.[a,b,c]` (S75) and namespace fan-out `s.{…}`
- `??`, dotted `use core.x.y as y`, enum-constructor calls `Type.Variant(args)`
- map/typed-collection literals (`[:]`, `[K, V]`), index-assign `m["k"] = v`
- `#Unsafe("reason")` / `#Uninit` / `#`-attributes (none modeled — every `#`
  errors), `spawn`
- block comments `/* … */` with nesting (only `//` and `///` exist today)
- **newline-terminated statements** — the grammar *requires* `;` on
  `struct_field`/`val_stmt`/`expr_stmt`/`return_stmt`; modern Jet omits them, so
  nearly every statement and field ERRORs. This single change is the biggest win.

## Query files (must change in lockstep — a dangling node ref is a hard Zed load failure)
- `highlights.scm` — drop `switch`/`val`/`var`; add `@=`/`:=` operators,
  capability sigils, `#`-attributes, pattern/arm nodes, `spawn`, `??`, fan-out.
- `outline.scm` — add `impl_block`, `test_block`, item-level `@=` bindings
  (e.g. `UserId @= distinct Int`), enum variants.
- `indents.scm` — remove `(switch_stmt)`; add the `if … == { }` arm block.
- `config.toml` — add `/* */` block-comment pair.

## Build / ship pipeline (`editors/zed/install.sh`)
1. Edit **`editors/tree-sitter/grammar.js`** (authoritative; install.sh copies it
   into `grammar-repo/`). Keep keyword lists derived from `Source/Syntax.rs` (I7).
2. `cd editors/zed/grammar-repo && tree-sitter generate` → regenerates
   `src/parser.c`/`grammar.json`/`node-types.json`; then rebuild
   `grammars/jet.wasm`. Or just `FORCE=1 editors/zed/install.sh` (regenerates,
   rebuilds wasm, recommits grammar-repo, re-pins the new commit into
   `extension.toml`).
3. Update the four `.scm`/`config.toml` query files in the same change.
4. Reinstall the dev extension in Zed (remove old → Add Dev Extension → reload).

## Done = 
Open a spread of real examples (10_structs, 30_json, 41_fan_out, 71_pattern_matching,
90_capability_sigils, 96_all_sigils) in Zed with the dev extension and confirm:
zero keyword bleed inside identifiers, no ERROR-driven mis-highlight, and modern
constructs (`@=`, `:=`, `#Unsafe`, sigils, patterns, fan-out) colored correctly.

No new user-facing syntax — this only makes the editor track already-ratified
syntax, so no ballot is required. Coordinate with the active implementing agent
and with c124/c127/c131 so the grammar lands on the final sigil vocabulary, not a
transitional one.
