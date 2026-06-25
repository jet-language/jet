# Comptime `$` splice marker + `comptime { }` execution block (c162)

**Status:** Plan — not started. Ratified D-CTMARKER1=C (2026-06-25). Implementation gated on c155 (S56) for the `$` splice consumer; the `comptime { }` block can land standalone.

## Goal

Add two ratified comptime-surface pieces:

1. `$name` — a **splice marker**: marks a compile-time value being woven into runtime/generated code. Metaprogramming splice site ONLY. Not a second spelling of `comptime` (no `$if`/`$loop`); I8 holds.
2. `comptime { … }` — a keyword-spelled **build-time statement block**, Jet's gated equivalent of Jai's free-standing `#run { … }`, consistent with the existing `comptime if`. Fills the one gap: today Jet has comptime bindings + `comptime if` but no comptime block.

Both run inside the D-CTCORE1 pure-Core whitelist + D-CTEFFECT1 reproducible/`#Impure` tiers. Codegen erases both (build-time only; no runtime code), `$name` weaves the computed value at its splice site.

## Current state (verified, file:line)

- **`$` is UNUSED in the lexer.** No `Dollar`/`$` `TokKind` exists (`Source/Lexer/Tokens.rs:17-133` — the full `TokKind` enum; no `$`). The scanner's punctuation `match` has no `'$'` arm (`Source/Lexer/Scan.rs:117-206`); `$` falls to the `other =>` arm and raises **E0001** unexpected-char (`Source/Lexer/Scan.rs:207-209`). So `$` is free to claim. The keyword table (`keyword()` consumed at `Scan.rs:204`) does not bind it either.
- **Existing comptime surface:**
  - Keyword token `KwComptime` (`Source/Lexer/Tokens.rs:47`), spelled by `Syntax::KW_COMPTIME = "comptime"` (`Source/Syntax.rs:580`), reserved-word set at `Source/Syntax.rs:1361`.
  - Bindings: `comptime NAME = expr;` parsed at `Source/Parser/Statements.rs:475-483` (dispatch) → `comptime_binding()` `Statements.rs:2003+`; AST flag `Binding.is_comptime` (local `Source/AST.rs:1600-1602`) and `Const.is_comptime` (item `Source/AST.rs:1295-1297`).
  - `comptime if`: parsed at `Source/Parser/Statements.rs:1969+` (dispatch by lookahead at `Statements.rs:475-481`); AST node `Stmt::ComptimeIf { cond, then_body, else_body, selected_then, .. }` (`Source/AST.rs:1430-1444`).
  - Evaluator lives in `Source/Comptime/` (`Interpreter.rs`, `Methods.rs`, `Purity.rs`, `Value.rs`, `Builtins.rs`, `mod.rs`). Items are evaluated pre-body in sema: `eval_comptime_items` / `comptime_context_from_items` in `Source/Sema/Registration.rs`.
- **`comptime if` lowering = erasure.** Sema picks the arm and sets `selected_then` (`Source/Sema/CheckerCore.rs:1023` dispatch, `:1610+` `check_comptime_if`). Codegen emits ONLY the chosen branch's statements inline on the same env — no `if`, no runtime conditional — and emits nothing when unresolved (I3): `Source/Codegen/TIR/lower.rs:1000-1013` (`TStmt::Inline(...)`). Formatter re-emits `comptime`-led: `Source/Formatter/Statements.rs:188-189`, span/else handling `Source/Formatter/mod.rs:299,619`.
- **D-CTCORE1 whitelist location:** `Source/Comptime/Methods.rs:362+` (header comment `:362`; module-alias dispatch `:321`; `eval` of a whitelisted pure Core call `:386+`; `core.math` `:400`, `core.string` `:435`; unknown/un-whitelisted → teaching diagnostic `:485+`). Purity walk in `Source/Comptime/Purity.rs`.
- **D-CTEFFECT1 (`#Impure` tiers) is NOT implemented yet.** It is its own card **c157** (`board.json:542`, stage `backlog`, decision `D-CTEFFECT1`), "Ratified — not yet implemented" (`docs/spec/syntax-decisions.md:2941`). No `#Impure` gate / `--allow-impure` exists in `Source/`. The current comptime wall is pure-only: I/O Core calls → **E0958** (`docs/spec/diagnostics.md:300`), impure reach → **E0951** (`:293`).
- **S56 reflection/derive surface does NOT exist yet.** It is card **c155** (`board.json:510`, stage `backlog`, decision `D-METADEPTH1`). Built-in derives (`#[Codable]`, S55) ship, but the user-authored-derive + reflection surface that *produces* the values `$name` splices is unbuilt. `$name` has no consumer until c155 lands.

## Decision (ratified — do not re-decide)

D-CTMARKER1=C (`docs/spec/syntax-decisions.md:2949`):
- `$` reserved for the metaprogramming **splice site only** — `$name` = a compile-time value woven into runtime/generated code (the reflection/derive surface, D-METADEPTH1=A).
- `comptime` keyword keeps declaring bindings + `comptime if`; **no** `$if`/`comptime if` duplicate (I8).
- Owner Q4 confirmed: add a keyword-spelled **`comptime { … }` statement block** (gated equivalent of Jai's `#run { … }`).
- Both run inside D-CTCORE1 whitelist + D-CTEFFECT1 tiers (Tier 0 pure always-on; Tier 1 hashed-reproducible `@embed`/`find`/`fetch(url,sha256:)`; Tier 2 ambient behind `#Impure(reason)` + `--allow-impure`).
- Per D-METADEPTH1=A + D-CTCODEGEN1=A: the block executes build-time work; it is **not** a macro / AST injector. Any generated source re-enters lexer→parser→sema.

## Implementation (staged)

### Stage A — `comptime { }` block (standalone; no c155/c157 dependency for the pure path)

1. **Parser.** In the `KwComptime` dispatch (`Source/Parser/Statements.rs:475`), extend the existing two-token lookahead (today: `if` → `comptime_if_stmt`, else → binding) with a third arm: `{` → `comptime_block_stmt`. Both braces required (mirror `comptime if`'s brace rule, `Statements.rs:1966`).
2. **AST.** Add `Stmt::ComptimeBlock { body: Vec<Stmt>, span }` beside `ComptimeIf` (`Source/AST.rs:1435`). Add to the span match (`AST.rs:1519`).
3. **Sema.** Add a `check_comptime_block` beside `check_comptime_if` (`Source/Sema/CheckerCore.rs:1023`/`1610`). Run the body through the existing comptime evaluator (`Source/Comptime/`) under the D-CTCORE1 whitelist + purity walk (`Purity.rs`). Pure path only in Stage A: any effectful op → existing **E0951**/**E0958**. (Tier-1/Tier-2 effect tiers wire in Stage C.) Decide binding-leak semantics per Open Owner-Q.
4. **Codegen (erasure).** Add `Stmt::ComptimeBlock { .. } => TStmt::Inline(vec![])` (or emit nothing) in `Source/Codegen/TIR/lower.rs` beside the `ComptimeIf` arm (`:1000`). The block runs at build; it leaves NO runtime Rust. I3 holds — codegen emits nothing.
5. **Formatter.** Add a `ComptimeBlock` arm in `Source/Formatter/Statements.rs` (beside `:189`) and the span/recursion arms in `Source/Formatter/mod.rs` (beside `:299,619`). Required by the new-syntax rule: emission + a `fmt` STABILITY test (idempotence alone misses dropped tokens).
6. **Diagnostics.** No new code needed for the pure path (reuses E0951/E0958/E0956). Add a `comptime { }`-position parse error only if a placement is illegal per the Open Owner-Q answer.
7. **Example + golden (I5).** `examples/features/NN_comptime_block.jet` exercising a pure build-time block (e.g. a whitelisted compute + a `comptime` binding it sets up), with `expected/NN_comptime_block.out`. Confirm generated Rust contains no leftover block.
8. **Tests.** `tests/comptime.rs` (or extend existing): parse, sema-erasure, golden, fmt-stability. UI fixture for any new diagnostic.

### Stage B — `$name` splice expression (sequence AFTER c155 / S56)

1. **Lexer.** Add `TokKind::Dollar` to `Source/Lexer/Tokens.rs:17` enum + `describe` (`:163`). Add a `'$'` arm in `Source/Lexer/Scan.rs:117` (single-char, length 1; no compound). Record the decision id in the doc-comment.
2. **Syntax.rs (I7).** Add `pub const SIGIL_SPLICE: &str = "$";` with the `D-CTMARKER1` id, beside the other sigils.
3. **Parser.** Parse `$name` as a prefix splice in expression position → `Expr::Splice { name, span }` (new AST variant). Grammar shape (`$ident` vs richer) per Open Owner-Q; recommend minimal `$ident` matching c155's surface.
4. **Sema.** A `$name` splice is legal ONLY inside a generated/comptime context tied to the S56 reflection/derive surface (c155). It resolves `name` to a comptime value produced by that surface and weaves it in. Outside such a context → new diagnostic (Stage D). Type = the comptime value's type.
5. **Codegen.** `$name` weaves the computed compile-time value at the splice site (emit the constant); the splice marker itself disappears.

### Stage C — effect tiers (sequence WITH/AFTER c157 / D-CTEFFECT1)

Once c157 ships the `#Impure` gate + `--allow-impure` + Tier-1 hashed-reproducible effects, extend `check_comptime_block` (and binding eval) so Tier-1 effects are allowed and recorded in `.jet/lock`, and Tier-2 ambient effects require `#Impure("reason")` + `--allow-impure`. Reuse c157's diagnostic for "effectful op without `#Impure`" — do not invent a parallel code.

### Stage D — splice-misuse diagnostics (with Stage B)

- New code (propose **E0959**, confirm next-free in `docs/spec/diagnostics.md`): "`$name` used outside a splice site" — what/why/fix + `tests/ui` snapshot (I4). Fix line points at `comptime`/the derive surface.
- The "effectful op without `#Impure`" diagnostic is **c157's** (D-CTEFFECT1), not new here.

### Docs (every stage)

- `docs/spec/spec.md`: document `comptime { }` (Stage A) next to `comptime if`/comptime bindings; document `$name` splice (Stage B) with the S56 surface.
- `docs/spec/syntax-decisions.md`: flip D-CTMARKER1's "not yet implemented" status (`:2949`) to implemented as each stage lands; add the splice sigil to the sigil tables.
- `docs/spec/diagnostics.md`: register E0959 (Stage D).

## Sequencing / gates

- **Stage A (`comptime { }`) is independent** — depends only on the existing comptime evaluator + D-CTCORE1 whitelist. Build it first.
- **Stage B (`$name`) is gated on c155 (S56).** `$name` has no value to splice until the reflection/derive surface produces one. Lex/parse could land earlier behind a "no consumer yet" sema error, but the meaningful path waits on c155 — sequence B after c155.
- **Stage C (effect tiers) is gated on c157 (D-CTEFFECT1).** Until c157, `comptime { }` is pure-only (E0951/E0958 on effects). No `#Impure` gate exists yet.
- No invariant carve-outs: I1 (effects gated, expert opt-in via `#Impure`), I2 (sema rejects, rustc never sees bad code), I3 (codegen erases), I4 (E0959 + snapshot), I5 (example + golden), I6 (evaluator is std-only), I7 (`$` in Syntax.rs with D-CTMARKER1), I8 (block is the one new mechanism; no `$if` fork).

## Open Owner-Q

The ratified text settles the sigil (`$name`), the keyword block form (`comptime { }`), and the tier model. Two execution-semantics details are not pinned and block Stage A sema:

**Q1 — `comptime { }` placement.** Where may the block appear?
- (a) Function-body statement only (matches "statement block"; mirrors `comptime if`).
- (b) Module/item level only (Jai `#run {}` is top-level; fits build-time effects like a one-shot `@embed` scan).
- (c) Both.
- *Recommendation:* (c) both — `comptime if` is already a statement, and a build-time block is equally useful at module scope; consistent with the comptime binding which is both item and local. (Do not pick.)

**Q2 — does `comptime { }` introduce bindings visible afterward?**
- (a) No value, no leak: pure build-time side effects (Tier-1/Tier-2 work, `comptime` bindings declared inside stay inside). Names that should outlive the block use the existing `comptime NAME = …` binding. Smallest surface; preserves "one canonical mechanism for compute-a-value" (I8).
- (b) Bindings leak to the enclosing scope (matches how `comptime if`'s chosen-arm `let`s leak today, `lower.rs:996-999`).
- *Recommendation:* (a) — the block's job is build-time *effects/work*; value-binding already has its canonical form (`comptime NAME = …`). Leak (b) creates a second way to bind a comptime value (I8 pressure). (Do not pick.)

(Stage B's `$name` grammar — bare `$ident` vs `$ident.path`/`$(expr)` — is deferred to c155, where the reflection/derive surface defines what a splice can name; not an independent owner question.)
