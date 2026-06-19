# Sidequest: convert attributes from `@` to `#`

## Goal

Evaluate and (if the owner approves) migrate Jet's attribute marker sigil from
`@` to `#` — `@unsafe` → `#unsafe`, `@audit("…")` → `#audit("…")`,
`@Serialize` → `#Serialize`, `@[a, b]` → `#[a, b]`. This is a pervasive
user-facing syntax change: it touches I7 (`src/syntax.rs` decision IDs), the
lexer/parser, fmt, the teaching-error that today steers users *away* from `#`,
plus every example and ui snapshot. It also **reverses two ratified decisions**,
both of which rejected the Rust-style `#[…]` spelling — so the headline question
the owner must answer is not "is `#` nicer" but "do we resurrect the spelling we
already rejected, and accept `#`'s comment-char baggage." Agent surfaces the
decision; owner picks. No code until ratified.

The two decisions to reverse are distinct rows:

| Row | What it decided | What reversing it means |
|---|---|---|
| **S55** (derive policy) | "Rejected: … Rust `#[derive(…)]` attributes." | re-allows the `#[…]`-derived spelling for built-in trait markers (`@Serialize`/`@Comparable`). |
| **S82** (`@` marker syntax) | "`@` not `#` — … Rejected: `#[…]` Rust-style attributes." | swaps the marker sigil itself from `@` to `#` across all attribute forms. |

S82 is the general marker-sigil choice; S55 is the derive-specific
`#[derive(…)]` rejection. The owner must amend **both**, not one.

## Current state (verified)

### The `@` sigil is overloaded — five distinct jobs

`@` is not just "the attribute prefix." A migration must decide **per use**
whether it moves to `#` or stays `@`:

| Use | Decision ID | Where (symbol) | Example |
|---|---|---|---|
| Attribute prefix | S82 (`syntax::ATTR_PREFIX`) | parser, fmt | `@unsafe`, `@Serialize`, `@audit("…")` |
| Attribute list delimiters | S82 (`syntax::ATTR_LIST_OPEN`/`CLOSE`) | parser | `@[a, b]` |
| Loop label | D-LABEL1 | parser, fmt (`fmt_stmt` label arms in src/fmt/stmts.rs) | `@outer loop { … }`, `break @outer` |
| `os` host selector | U16 (`syntax::OS_HOST_SELECTOR`) | cmd_pkg / cmd_supply | `jetpack os switch ./config.jet@web-1` |
| Provider/target ref sep | U6 (`syntax::REF_PROVIDER_AT`) | manifest/fetch | `github@owner/repo/rev`, `nixpkgs@…` |

D-LABEL1 reuses the S82 `@` marker sigil, and the parser disambiguates
label-vs-attribute by the following keyword. fmt emits both `@unsafe`
(src/fmt/items.rs — `fmt_func`, the `is_unsafe` arm) and the labels
`@outer`/`break @outer`/`continue @outer` (src/fmt/stmts.rs — `fmt_stmt` loop,
`BreakLabel`/`ContinueLabel` arms) side by side. **The loop-label entanglement is
the trap:** if attributes move to `#` but labels stay `@`, Jet source carries two
marker sigils and fmt prints both. The two ref/host `@` uses (U6, U16) live in
CLI/manifest strings, not Jet source — they reuse `@` deliberately and are out of
scope for a *source-syntax* sigil swap unless the owner wants total consistency.

### `#` is already a live `Hash` token (the collision)

`#` is not free. The lexer already emits `TokKind::Hash` for a bare `#`
(src/lexer/scan.rs — the `'#'` one-char arm). It has two existing jobs, both
**S76**:

- `[T#N]` fixed-size list, parsed in src/parser/types.rs (the `TokKind::Hash`
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
`TokKind::At` arms in src/parser/items.rs (item dispatch, `at_unsafe_fn`,
`at_c_module`, `const_def`'s attr loop), in src/parser/stmts.rs (statement
`@`-marker / `@unsafe`-block arms), and in src/parser/modules.rs (the
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

- `@static` / `@inline` const attrs: parsed in src/parser/items.rs (`const_def`'s
  `while … TokKind::At` attr loop → `ConstAttr::ForceStatic`/`ForceInline`),
  emitted in src/fmt/items.rs (`fmt_const`, the `ConstAttr` match — `@static `/
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

- `src/syntax.rs`: 4 constants (`ATTR_PREFIX`, `ATTR_LIST_OPEN`,
  `ATTR_LIST_CLOSE`, `FOREIGN_HASH_ATTR`) + decision-ID comments on S82 / S55 /
  S58 / S59 / D-LABEL1.
- `src/parser/{items,stmts,modules,types}.rs`, `src/fmt/{items,stmts}.rs`:
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

## Decision points (owner approval required BEFORE coding)

All three must land before any code; (2) and (3) can ride on (1).

**Decision 1 — `#` vs `@` at all** (the S55 + S82 reversal, plus `#`'s
comment-char baggage).

```jet
// Before (today, @):        // After (Option #):
@unsafe                      #unsafe
fn raw() { … }               fn raw() { … }

@Serialize                   #Serialize
struct Point { … }           struct Point { … }
```

**Decision 2 — bare `#Name` vs Rust-literal `#[…]`** for the multi-marker list
form.

```jet
// Before (today, @[…]):     // After 2a (bare):   // After 2b (Rust-literal):
@[Serialize, Comparable]     #[Serialize, …]       #[derive(Serialize, …)]
struct Point { … }           struct Point { … }    struct Point { … }
```

**Decision 3 — loop labels and the CLI ref/host `@`.** If attributes move but
labels stay, two sigils coexist in source:

```jet
// Move attrs only (mixed):  // Move both (uniform):
#unsafe                      #unsafe
@outer loop {                #outer loop {
    break @outer                 break #outer
}                            }
// ref/host @ (U6/U16) lives in CLI strings, not source — likely stays @:
//   github@owner/repo/rev   jetpack os switch ./config.jet@web-1
```

## Proposed implementation (only after ratification)

Follow the workflow loop: failing test first → spec → parser → sema → codegen
→ fmt → diagnostics → examples → green → docs.

1. **Decision record.** Add a ratified row (e.g. `S82-amend` or a new `D-ATTR`
   id) to syntax-decisions.md with the chosen option; flip **both** rejection
   notes — S55's "Rejected: … Rust `#[derive(…)]`" and S82's "Rejected: `#[…]`
   Rust-style attributes" — to record the reversal and its rationale. Update
   `tests/decisions.rs` surface list if the id changes.
2. **Failing tests first.** Write the new teaching-error ui snapshot (`@unsafe`
   → "use `#unsafe`", mirroring today's `#[`→`@[` error) and convert one
   example (e.g. 48_lowlevel.jet) as the canary before touching the parser.
3. **syntax.rs.** `ATTR_PREFIX = "#"`; `FOREIGN_HASH_ATTR` becomes
   `FOREIGN_AT_ATTR = "@"` (rejected spelling now `@`). `ATTR_LIST_OPEN`/`CLOSE`
   unchanged (`[`/`]`). If labels move, `OS_HOST_SELECTOR`/`REF_PROVIDER_AT`
   stay `@` unless decision (3) says otherwise.
4. **Lexer.** No new token needed (`Hash` exists). Only add the shebang
   carve-out *if* a `#!` shebang is planned (see open questions) — skip a `#!`
   on source line 1 before tokenizing. Otherwise lexer is untouched.
5. **Parser.** Replace `TokKind::At` with `TokKind::Hash` in every prefix
   attribute dispatch (the `TokKind::At` arms in src/parser/items.rs,
   src/parser/stmts.rs, src/parser/modules.rs, and `const_def`'s attr loop). Keep
   `Hash`'s `[T#N]` branch (src/parser/types.rs) intact — it is bracket-interior,
   never prefix, so no ambiguity. If labels move, port the
   following-keyword disambiguation (label vs `#unsafe`/`#audit`/`#Marker`).
   Add the `@`-rejected teaching error.
6. **Sema.** No semantic change — attributes mean the same thing; only the
   token feeding sema changes. Verify S58 gate checks (E3101/E3103/L3101) and
   S59 C-binding checks read the new token.
7. **Codegen.** None (I3) — attributes already lower the same way.
8. **fmt.** Swap every `@`-emitting `write` to `#`: src/fmt/items.rs (`fmt_func`
   `@unsafe`, `fmt_const` `@static`/`@inline`) and src/fmt/stmts.rs (`@audit`,
   `@unsafe {` block, and the label arms if decision (3) moves labels). `jet fmt`
   then canonicalizes any surviving `@unsafe` to `#unsafe`.
9. **Diagnostics.** Update every attribute-bearing row in diagnostics.md
   (E3101/E3103/L3101/E3202/E3203/E3205-E3208/E3301/E1802/E2201, the
   const-attr `E`, and the new rejected-spelling error). Re-bless ui snapshots.
10. **Examples + tests.** Convert all ~24 example attribute lines and re-bless
    the ~18 ui/ui_lint snapshots and golden/showcase/cffi/repl expectations
    (`UPDATE_EXPECT=1`). Update spec.md, architecture.md, roadmap.md, and the
    docs/plans attribute references.

## Acceptance checklist

- [ ] New/amended decision row ratified in syntax-decisions.md; S82 reversal
      noted; `tests/decisions.rs` surface list consistent.
- [ ] syntax.rs: `ATTR_PREFIX = "#"`, rejected-spelling constant flipped to
      `@`; decisions (loop label, ref/host) reflected.
- [ ] New teaching error: `@unsafe`/`@[` rejected, teaches `#unsafe`/`#[`, with
      a ui snapshot (no snapshot = error doesn't exist, I4).
- [ ] Parser: every prefix dispatch reads `Hash`; `[T#N]` and `name#ver` still
      parse (positional disambiguation holds); regression test for all three.
- [ ] fmt emits `#`-form for attributes (and labels, per decision); `jet fmt`
      round-trips a converted example.
- [ ] All ~24 example attribute lines converted; golden tests green (I5).
- [ ] All ~18 ui/ui_lint snapshots + cffi/showcase/repl expectations re-blessed.
- [ ] diagnostics.md attribute rows updated; rendered output matches snapshots.
- [ ] spec.md / architecture.md / roadmap.md / docs/plans references updated.
- [ ] `cargo test` fully green; no `@` attribute survives in src/docs/examples
      except the deliberately-kept ref/host `@` (if decision (3) keeps them).
- [ ] I1-I8 unbent (esp. I3 codegen unchanged, I4 snapshot exists, I7 every
      typeable sigil in syntax.rs with an id).
