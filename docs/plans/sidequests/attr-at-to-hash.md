# Sidequest: convert attributes from `@` to `#`

**Status:** ratified 2026-06-19 (D-ATTR1 = B, D-ATTR2 = A, D-ATTR3 = B) — ready to implement on owner's word.

## Goal

Migrate Jet's attribute/marker sigil from `@` to `#` — `@unsafe` → `#unsafe`,
`@audit("…")` → `#audit("…")`, `@Serialize` → `#Serialize`, `@[a, b]` →
`#[a, b]`. Loop labels keep `@` (D-ATTR3 = B): two marker sigils coexist in
source. This is a pervasive user-facing syntax change: it touches I7
(`Source/Syntax.rs` decision IDs), the lexer/parser, fmt, the teaching-error that
today steers users *away* from `#`, plus every example and ui snapshot. It
reverses two prior decisions (S55, S82) that rejected the `#[…]` spelling.

The two decisions reversed are:

| Row | What it decided | How ratification resolves it |
|---|---|---|
| **S55** (derive policy) | "Rejected: … Rust `#[derive(…)]` attributes." | RESOLVED (D-ATTR2 = A): bare `#[Serialize, Comparable]` list form adopted; Rust-literal `#[derive(…)]` remains rejected. |
| **S82** (`@` marker syntax) | "`@` not `#` — … Rejected: `#[…]` Rust-style attributes." | RESOLVED (D-ATTR1 = B): sigil swaps to `#` for attributes/markers; S82 is amended, not abandoned. |

## Current state (verified)

### The `@` sigil is overloaded — five distinct jobs

`@` is not just "the attribute prefix." A migration must decide **per use**
whether it moves to `#` or stays `@`:

| Use | Decision ID | Where (symbol) | Example |
|---|---|---|---|
| Attribute prefix | S82 (`syntax::ATTR_PREFIX`) | parser, fmt | `@unsafe`, `@Serialize`, `@audit("…")` |
| Attribute list delimiters | S82 (`syntax::ATTR_LIST_OPEN`/`CLOSE`) | parser | `@[a, b]` |
| Loop label | D-LABEL1 | parser, fmt (`fmt_stmt` label arms in Source/Formatter/Statements.rs) | `@outer loop { … }`, `break @outer` |
| `os` host selector | U16 (`syntax::OS_HOST_SELECTOR`) | cmd_pkg / cmd_supply | `jetpack os switch ./config.jet@web-1` |
| Provider/target ref sep | U6 (`syntax::REF_PROVIDER_AT`) | manifest/fetch | `github@owner/repo/rev`, `nixpkgs@…` |

D-LABEL1 keeps `@` (D-ATTR3 = B, ratified). The parser disambiguates
label-vs-attribute by the following keyword; once attributes become `#`, there is
no ambiguity at all — `#` in prefix position is always an attribute, `@` in
prefix position before a loop/block keyword is always a label. fmt will emit
`#unsafe` / `#audit(…)` / `#Serialize` alongside `@outer loop` / `break @outer`
— two marker sigils in the same source file. This mixed-sigil end state was the
"trap" the plan flagged; the owner chose it knowingly. The two ref/host `@` uses
(U6, U16) live in CLI/manifest strings, not Jet source — they are out of scope.

### `#` is already a live `Hash` token (the collision)

`#` is not free. The lexer already emits `TokKind::Hash` for a bare `#`
(Source/Lexer/Scan.rs — the `'#'` one-char arm). It has two existing jobs, both
**S76**:

- `[T#N]` fixed-size list, parsed in Source/Parser/Types.rs (the `TokKind::Hash`
  branch inside a bracket type → `E0963` if no integer follows).
  `TYPE_FIXED_SIZE_SEP = "#"` (`syntax::TYPE_FIXED_SIZE_SEP`).
- `name#ver` package version-pin (`pkg#1.2.0`). Same constant; resolved by
  **position** at the ref/manifest string level (not the Jet lexer `Hash`
  token).

Feasibility hinges on **positional disambiguation**: a prefix `#Name` /
`#[…]` at item/statement start cannot collide with `[T#N]` (always
*bracket-interior*, after a type) or `name#ver` (always *infix*, between two
name parts, and only inside ref strings). The parser would consume `Hash`
instead of `At` in prefix position exactly where it consumes `At` today — the
`TokKind::At` arms in Source/Parser/Items.rs (item dispatch, `at_unsafe_fn`,
`at_c_module`, `const_def`'s attr loop), in Source/Parser/Statements.rs (statement
`@`-marker / `@unsafe`-block arms), and in Source/Parser/Modules.rs (the
`KwConst | At` const-item arm). **The lexer barely changes** — both `At` and
`Hash` tokens already exist; the work is parser dispatch + fmt output + the
teaching-error flip.

### The teaching error points the *wrong way* for this change

`FOREIGN_HASH_ATTR = "#["` (`syntax::FOREIGN_HASH_ATTR`) is **defined but not yet
consumed** — the constant reserves the rejected `#[…]` spelling, but no parser
arm references it today (grep finds only the definition). It is the placeholder
for an unbuilt teaching error that would reject Rust-style `#[…]` and teach
`@[…]`. S55 (the derive-policy block) records: *"Rejected: … Rust `#[derive(…)]`
attributes,"* and S82 records: *"Rejected: `#[…]` Rust-style attributes."* Moving
to `#` resurrects exactly that spelling and inverts the not-yet-written error: it
must instead reject `@unsafe` and teach `#unsafe`.

### Const attributes and aspirational forms

- `@static` / `@inline` const attrs: parsed in Source/Parser/Items.rs (`const_def`'s
  `while … TokKind::At` attr loop → `ConstAttr::ForceStatic`/`ForceInline`),
  emitted in Source/Formatter/Items.rs (`fmt_const`, the `ConstAttr` match — `@static `/
  `@inline `). The unknown-attr error in that parser loop hard-codes the `@` in
  its message text.
- `@bindgen` / `@extern module` (S59, `syntax::ATTR_BINDGEN`/`ATTR_EXTERN_MODULE`):
  C-binding module attributes. Many diagnostics name them (`E3203`,
  `E3205`-`E3208`).
- `@embed("file")` appears in diagnostics.md:530 (E3301) and `@embed`/`@embed`
  text, but the real builtin is `embed_file` (S26) — this is inconsistent doc
  copy, fold it into the sweep but note it's not a parsed attribute.
- `@test` (S43), `@Comparable` / `@Serialize` (S55 — note S55 **retired** the
  in-body `derive Trait;` form in favor of the prefix `@Trait` attribute).

### Footprint (counted)

- `Source/Syntax.rs`: 4 constants (`ATTR_PREFIX`, `ATTR_LIST_OPEN`,
  `ATTR_LIST_CLOSE`, `FOREIGN_HASH_ATTR`) + decision-ID comments on S82 / S55 /
  S58 / S59 / D-LABEL1.
- `Source/Parser/{items,stmts,modules,types}.rs`, `Source/Formatter/{items,stmts}.rs`:
  dispatch + emit sites above (the parser/lexer/fmt single files were split into
  these dirs).
- Docs: ~31 `@` mentions in spec.md; S82/S55/S58/S59/D-LABEL1 blocks in
  syntax-decisions.md; diagnostics.md attribute-bearing rows; architecture.md;
  roadmap.md; ~14 plan files under docs/plans/.
- Examples: ~24 attribute lines (e.g. examples/features/48_lowlevel.jet,
  examples/showcase/lowlevel.jet, examples/capstone/logbook/).
- Tests: ~18 ui / ui_lint snapshots (cffi_*, lowlevel_*, deref_forbidden,
  unsafe_missing_audit, …) plus golden/showcase/cffi/repl harness `.txt`
  expectations.
- `tests/decisions.rs:65`: S82 listed in `SURFACE_IN_SYNTAX_RS` — stays
  enforced; if a new ID supersedes S82, update this list.

## Decisions (resolved 2026-06-19)

**D-ATTR1 = B — attributes/markers move to `#`.** RESOLVED.

```jet
// Before (today, @):        // After (ratified, #):
@unsafe                      #unsafe
fn raw() { … }               fn raw() { … }

@Serialize                   #Serialize
struct Point { … }           struct Point { … }
```

**D-ATTR2 = A — multi-marker list is bare `#[…]`.** RESOLVED. Rust-literal
`#[derive(…)]` remains rejected (S55).

```jet
// Before (today, @[…]):     // After (ratified, bare #[…]):
@[Serialize, Comparable]     #[Serialize, Comparable]
struct Point { … }           struct Point { … }
```

**D-ATTR3 = B — loop labels stay `@`.** RESOLVED. Two marker sigils coexist;
this is the "trap" the plan flagged, chosen knowingly by the owner.

```jet
// Ratified end state (mixed sigils):
#unsafe
@outer loop {
    break @outer
}
// ref/host @ (U6/U16) in CLI strings is out of scope and stays @:
//   github@owner/repo/rev   jetpack os switch ./config.jet@web-1
```

## Proposed implementation

Follow the workflow loop: failing test first → spec → parser → sema → codegen
→ fmt → diagnostics → examples → green → docs.

1. **Decision record.** D-ATTR1/D-ATTR2/D-ATTR3 are recorded in
   syntax-decisions.md (ratified 2026-06-19). Flip S55's "Rejected: … Rust
   `#[derive(…)]`" and S82's "Rejected: `#[…]` Rust-style attributes" to note
   the reversal and its rationale. Update `tests/decisions.rs` surface list for
   the new IDs (D-ATTR1/D-ATTR2/D-ATTR3 replace S55/S82 in
   `SURFACE_IN_SYNTAX_RS` if the implementation uses those IDs).
2. **Failing tests first.** Write the new teaching-error ui snapshot (`@unsafe`
   → "use `#unsafe`", mirroring today's `#[`→`@[` error) and convert one
   example (e.g. 48_lowlevel.jet) as the canary before touching the parser.
3. **syntax.rs.** `ATTR_PREFIX = "#"`; `FOREIGN_HASH_ATTR` becomes
   `FOREIGN_AT_ATTR = "@"` (rejected spelling is now `@`). `ATTR_LIST_OPEN`/`CLOSE`
   unchanged (`[`/`]`). Labels keep `@` (D-ATTR3 = B), so `OS_HOST_SELECTOR`/
   `REF_PROVIDER_AT` also stay `@` — no label or ref/host constant changes.
4. **Lexer.** No new token needed (`Hash` exists). Only add the shebang
   carve-out *if* a `#!` shebang is planned (see open questions) — skip a `#!`
   on source line 1 before tokenizing. Otherwise lexer is untouched.
5. **Parser.** Replace `TokKind::At` with `TokKind::Hash` in every prefix
   attribute dispatch (the `TokKind::At` arms in Source/Parser/Items.rs,
   Source/Parser/Statements.rs, Source/Parser/Modules.rs, and `const_def`'s attr loop). Keep
   `Hash`'s `[T#N]` branch (Source/Parser/Types.rs) intact — it is bracket-interior,
   never prefix, so no ambiguity. Labels stay `@` (D-ATTR3 = B), so the existing
   following-keyword disambiguation (label-vs-attribute by keyword after `@`)
   **disappears** — `@` always means label now, `#` always means attribute.
   Add the `@unsafe` / `@[` rejected-spelling teaching error.
6. **Sema.** No semantic change — attributes mean the same thing; only the
   token feeding sema changes. Verify S58 gate checks (E3101/E3103/L3101) and
   S59 C-binding checks read the new token.
7. **Codegen.** None (I3) — attributes already lower the same way.
8. **fmt.** Swap attribute `@`-emitting writes to `#`: Source/Formatter/Items.rs
   (`fmt_func` `@unsafe`, `fmt_const` `@static`/`@inline`) and Source/Formatter/Statements.rs
   (`@audit`, `@unsafe {` block). Label arms (`@outer`, `break @outer`,
   `continue @outer`) in Source/Formatter/Statements.rs stay `@` — D-ATTR3 = B. `jet fmt`
   then canonicalizes any `@unsafe` / `@audit` / `@Marker` in user source to
   the `#` form.
9. **Diagnostics.** Update every attribute-bearing row in diagnostics.md
   (E3101/E3103/L3101/E3202/E3203/E3205-E3208/E3301/E1802/E2201, the
   const-attr `E`, and the new rejected-spelling error). Re-bless ui snapshots.
10. **Examples + tests.** Convert all ~24 example attribute lines and re-bless
    the ~18 ui/ui_lint snapshots and golden/showcase/cffi/repl expectations
    (`UPDATE_EXPECT=1`). Update spec.md, architecture.md, roadmap.md, and the
    docs/plans attribute references.

## Acceptance checklist

- [ ] D-ATTR1/D-ATTR2/D-ATTR3 rows in syntax-decisions.md; S55/S82 reversal
      noted; `tests/decisions.rs` surface list consistent with new IDs.
- [ ] syntax.rs: `ATTR_PREFIX = "#"`, rejected-spelling constant flipped to
      `FOREIGN_AT_ATTR = "@"`; label constants (`D-LABEL1`) unchanged.
- [ ] New teaching error: `@unsafe`/`@[` rejected, teaches `#unsafe`/`#[`, with
      a ui snapshot (no snapshot = error doesn't exist, I4).
- [ ] Parser: every attribute prefix dispatch reads `Hash`; `@` in prefix
      position is unambiguously a label (no following-keyword disambiguation
      needed anymore); `[T#N]` and `name#ver` still parse; regression test for all.
- [ ] fmt emits `#`-form for attributes; label arms (`@outer`, `break @outer`,
      `continue @outer`) stay `@`; `jet fmt` round-trips a converted example.
- [ ] All ~24 example attribute lines converted to `#`; golden tests green (I5).
- [ ] All ~18 ui/ui_lint snapshots + cffi/showcase/repl expectations re-blessed.
- [ ] diagnostics.md attribute rows updated; rendered output matches snapshots.
- [ ] spec.md / architecture.md / roadmap.md / docs/plans references updated.
- [ ] `cargo test` fully green; no `@attribute` survives in src/docs/examples;
      `@label` and `@` ref/host forms unchanged.
- [ ] I1–I8 unbent (esp. I3 codegen unchanged, I4 snapshot exists, I7 every
      typeable sigil in syntax.rs with an id).
