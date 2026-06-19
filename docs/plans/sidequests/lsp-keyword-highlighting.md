# Sidequest: LSP keyword highlighting — substring leak & source-of-truth drift

## Goal

Resolve the bug-fixes.md item: *"keywords that are contained in other words or
contexts, like variable names or function names are lsp highlighted as keywords,
where `print` -> the letters in are highlighted."*

Decoded, the example is precise: in `pr-IN-t` the keyword **`in`** (`KW_IN`) is
being matched **inside** the identifier `print`. That is classic
keyword-as-substring highlighting — a highlighter matching keyword text without
word boundaries / without token awareness.

This plan triages where that can happen across every highlighter Jet ships,
fixes the reachable gap, and closes the underlying cause: the keyword lists in
the LSP and the TextMate grammars are **hand-maintained copies** that drift from
the single source of truth (`src/syntax.rs`, invariant **I7**). The drift is the
real defect; the substring symptom is one of its surface forms.

This is a **SMALL, well-scoped fix**, not the "substantial LSP improvements"
that bug-fixes.md mentions separately. Keep general LSP-quality work out of this
plan.

## Current state (verified)

Four code paths can color a `print` token. Three are already boundary-safe; one
table is stale; and all four duplicate keyword knowledge that belongs only in
`src/syntax.rs`.

### 1. LSP semantic tokens (server) — CORRECT, rule it out

`src/lsp/features.rs::semantic_token_type_for` (lines 263–345) and
`encode_semantic_tokens` (348–392) classify by **`TokKind`**, driven by the real
lexer. The lexer yields a single `TokKind::Ident("print")` token, classified
`st::VARIABLE` (features.rs:303–311). It never emits a `KwIn` span *inside*
`print`. The delta-encoding (`byte_offset_to_lsp`, `src/lsp/position.rs:24`) is
UTF-16-correct. **This path cannot produce the symptom** — do not touch it and do
not re-litigate the encoding.

### 2. `JET_KEYWORDS` / `is_keyword` (rename + completion) — STALE, but whole-word

`src/lsp/completion.rs:81` (`JET_KEYWORDS`) and `:89` (`JET_TYPES`) are consumed
by:

- `src/lsp/features.rs:184` `is_keyword` → `JET_KEYWORDS.contains(&name)` (slice
  `.contains`, **whole-word**, no substring risk), used by rename validation
  (features.rs:211, 221).
- `src/lsp/completion.rs:395, 409` completion list assembly.

No substring matching here, so this is **not** the highlight bug. But the table
is **demonstrably drifted** from `src/syntax.rs`:

| `JET_KEYWORDS` lists | Reality in `src/syntax.rs` |
|---|---|
| `switch` | `KW_SWITCH = "when"` (syntax.rs:181) — `switch` is not a keyword |
| `val`, `var` | retired to `FOREIGN_VAL`/`FOREIGN_VAR` teaching-errors (syntax.rs:37–38) — not keywords |
| `when` (also present) | correct, but listed *alongside* the wrong `switch` |
| `ref` | `KW_STORED = "ref"` (ok), but listed as a plain keyword |
| `import` | renamed to `use` by D-S16-USE (commit 380b6a5) |
| `value` | `LIT_VALUE = "value"` (a literal, syntax.rs:98), not a keyword |

Consequence today: rename **wrongly rejects** `switch`/`import` as "keywords"
(they aren't) and **wrongly allows** `when` only because it was also added. This
is a real correctness bug in `is_keyword`, just not the highlight one.

### 3. TextMate grammars — boundary-safe, but stale list (the symptom's home if `\b` is ever dropped)

Active grammar (registered by `editors/vscode/package.json:30–34`,
`scopeName: source.jet`): `editors/vscode/syntaxes/jet.tmLanguage.json`.
Standalone twin: `editors/jet.tmGrammar`.

Both spell keywords as alternation regexes **with `\b` … `\b`**, e.g.
`jet.tmLanguage.json:69`:

```
"match": "\\b(if|else|while|for|in|break|continue|switch|return|or|loop)\\b"
```

`\b` boundaries mean `in` will **not** match inside `print` here, and
`git log -p` on the file shows `\b` was present since the grammar's first commit
(02ffbc3). So the *current* TextMate grammar does **not** reproduce the bug.
But this regex-with-`\b` is exactly the construct that produces the reported
symptom the instant a `\b` is dropped or a keyword is added without one — and the
keyword list here is also **stale** (lists `switch`, `val`, `var`, `ref`,
`value`; missing the `when` rename and `use`). It is a latent substring trap plus
a second drifting copy of the keyword set.

### 4. Tree-sitter grammars (Zed) — node-based, boundary-safe

`editors/zed/languages/jet/highlights.scm` matches **named nodes**
(`(call_expr name: (identifier) @function.call)`), and the grammars
(`editors/tree-sitter/grammar.js`, `editors/zed/grammar-repo/grammar.js`) list
`"in"` as a real token (grammar.js:256) parsed by the tree-sitter lexer, which is
token-boundary-aware by construction. No substring risk. Both grammar headers
even say they are *"Generated from src/syntax.rs keyword/sigil constants"* —
i.e. the intended source-of-truth flow already exists in spirit here.

### Root cause

The literal `in`-inside-`print` symptom is **not reproducible against current
`master`**: the lexer-driven semantic-tokens path can't produce it, and every
shipped grammar is boundary-safe. The report reflects either a stale editor
install or an earlier grammar state.

The *underlying, still-live* defect is **source-of-truth drift (I7 violation in
spirit)**: keyword sets are hand-copied into `src/lsp/completion.rs`
(`JET_KEYWORDS`/`JET_TYPES`) and into both `.tmLanguage`/`.tmGrammar` files, and
they have drifted from `src/syntax.rs` (`switch` vs `when`, retired `val`/`var`,
`import` vs `use`, `value` as a keyword). That drift is what makes
keyword-highlighting and rename-validation wrong, and the regex-with-`\b` form in
the TextMate grammars is the latent mechanism by which the exact reported
substring symptom recurs.

## Proposed fix (small, across the pipeline)

Two independent, low-risk changes. Neither needs an owner syntax decision (no new
keyword/sigil; this is making existing tables agree with `src/syntax.rs`).

### Fix A — make the LSP keyword set track `src/syntax.rs` (correctness)

In `src/lsp/completion.rs`, replace the hand-typed `JET_KEYWORDS`/`JET_TYPES`
literals with values sourced from the `syntax::KW_*` / `TYPE_*` constants, so the
list cannot drift again. Minimal mechanical form:

- Build `JET_KEYWORDS` from the `syntax::KW_*` constants (e.g. `KW_FN`, `KW_PUB`,
  `KW_IF`, `KW_ELSE`, `KW_IN`, `KW_SWITCH` (= `"when"`), `KW_BREAK`,
  `KW_CONTINUE`, `KW_RETURN`, `KW_STRUCT`, `KW_ENUM`, `KW_IMPL`, `KW_CONST`,
  `KW_COMPTIME`, `KW_LOOP`, `KW_UNSAFE`, `KW_MUTATE`, `KW_MOVE`, `KW_VIEW`,
  `KW_STORED`, `KW_SELF`, …) rather than string literals. Drop `switch`, `val`,
  `var`, `import`, `value` (none are keywords).
- Build `JET_TYPES` from `TYPE_*` constants (`TYPE_INT`, `TYPE_FLOAT`,
  `TYPE_BOOL`, `TYPE_STRING`, `TYPE_CHAR`, `TYPE_LIST`, `TYPE_MAP`,
  `TYPE_SHARED`, …).
- `is_keyword` (features.rs:183) and completion (completion.rs:395/409) then work
  unchanged; rename stops mis-rejecting `switch`/`import` and stops treating
  `value` as un-nameable.

This is the canonical "single source of truth" fix the second bug-fixes.md bullet
asks for, applied to the LSP. The whole-word `.contains` matching is already
correct and stays.

### Fix B — harden + de-stale the TextMate grammars (closes the substring trap)

In `editors/vscode/syntaxes/jet.tmLanguage.json` (the active grammar) and the
`editors/jet.tmGrammar` twin:

- Update the keyword alternations to match `src/syntax.rs`: `switch` → `when`;
  remove retired `val`/`var`; ensure `use` is present; move `value` out of the
  keyword group (it is a literal). Keep `ref` only if `KW_STORED` is still `ref`.
- Confirm every keyword alternation keeps its `\b…\b` (or `(?<![A-Za-z0-9_])` /
  `(?![A-Za-z0-9_])`) boundaries, so no keyword can ever match inside an
  identifier. This is what directly forecloses the `in`-in-`print` symptom.

These two files are duplicate sources of truth for the same grammar; per the
"single source of truth" bug, the twin (`editors/jet.tmGrammar`) should either be
generated from / kept identical to the registered one, or deleted if unused.
Decide and note which in the PR. (The tree-sitter grammars already declare they
are generated from `src/syntax.rs`; the TextMate pair should follow the same
discipline.)

### Out of scope

- The lexer-driven semantic-tokens path (already correct).
- Broader "make the LSP flagship-quality" work from bug-fixes.md — separate
  effort, separate plan.

## Test / acceptance checklist

- [ ] **Unit (rename):** `is_keyword("switch")` is `false`, `is_keyword("when")`
      is `true`, `is_keyword("import")` is `false`, `is_keyword("use")` is `true`,
      `is_keyword("value")` is `false`. Add to the lsp test suite
      (`cargo test --test lsp`).
- [ ] **Unit (semantic tokens):** encode a source containing `print(in_count)`
      and assert the `print` and `in_count` spans are emitted as `VARIABLE`
      (and the literal `in` only highlights when it is a standalone `for x in xs`
      token). Confirms no substring keyword span.
- [ ] **Drift guard:** add a test that every entry in `JET_KEYWORDS` equals some
      `syntax::KW_*` constant (or derive the table from them so the test is
      structural). Fails if the table drifts again.
- [ ] **Grammar:** a small fixture run (or manual review) confirms `print`,
      `mainfn`, `inside` are *not* keyword-colored by
      `jet.tmLanguage.json`/`jet.tmGrammar`; `for x in xs` colors `in`.
- [ ] **No regressions:** `nix develop -c cargo test` full suite green; no
      existing ui snapshot changed (this touches LSP tables + editor grammars,
      not diagnostics).
- [ ] **Docs:** if `editors/jet.tmGrammar` is deleted or marked generated, note
      it; record the source-of-truth decision so the duplication doesn't return.
