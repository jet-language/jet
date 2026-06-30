# Pre/post increment operators (`++x`, `x++`, `--x`, `x--`)

**Card:** Tower #158 (`c5rmib6`) · **Epoch 3** · **Status:** *done* (D-INCR1=A ratified 2026-06-30)
**Ballot:** **D-INCR1** — ratified Option A (full C-style on mutable integer lvalues).

## 1. What this feature is

C-family languages expose four unary update operators:

| Form | Effect on `x` | Value of expression |
|------|---------------|---------------------|
| `++x` / `--x` (prefix) | add/subtract 1 | new value |
| `x++` / `x--` (postfix) | add/subtract 1 | old value |

Jet already has the canonical update path via **S17 compound assignment**: `x += 1`, `x -= 1`, `n += 1` (see `examples/features/loop_forms.jet`, `83_self_mutation.jet`). Sized integers inherit trap-on-overflow from **D-NUMOPS2**; compound `+=` on integers already routes through those rules.

**Codebase scan (2026-06-30):** no `++`/`--` tokens in `crates/jet-foundation/src/Syntax.rs` or `Source/` parser/sema. Feature is greenfield.

## 2. Spec conflicts to resolve

| Source | Tension |
|--------|---------|
| **I8** (philosophy #4) | Exactly one mechanical path per operation. `+= 1` / `-= 1` already increment/decrement. Adding `++`/`--` is a second spelling unless it fully replaces compound increment (it does not — S17 covers `+= 2`, bit-ops, floats, etc.). |
| **S17** (ratified) | Full compound-assignment set; LHS must be `var` or `mut` parameter. Any `++`/`--` design should reuse the same mutability/lvalue rules, not invent a third LHS policy. |
| **philosophy non-goals** | "Operator overloading" is a v1 non-goal — `++`/`--` are not overloading, but they are extra operator surface. |
| **philosophy owner direction (2026-06-12)** | Hybrid identity: adopt familiar C-family ergonomics when they do not fight safety or one-path. This card is the explicit owner ask for that familiarity. |
| **D-LOOP-SURFACE-REOPEN (ratified B)** | Counted-loop headers may use `i += n` afterthoughts. If `++` ships, loop headers are a high-visibility site — ballot must say whether `i++` is legal there. |
| **Lexer** | `--` is not a Jet operator today. Tokenizer must distinguish `x--` from `x - -y` (subtraction chain). `++` has no current conflict. |

Rank tradeoffs only on: safety, beginner experience, one-path, long-term correctness — never implementation effort.

## 3. DECISION BALLOT — D-INCR1

See `tools/Tower/tower.json` decision **D-INCR1** for the owner-facing ballot (gist, story, inWild, comparisons, worked options, recommendation).

### Option summary

| Key | Summary |
|-----|---------|
| **A** | Ship full C-style `++`/`--` (pre + post) on mutable integer lvalues (`var`, `mut` param, `self.field`, index) |
| **B** | Reject forever — `x += 1` / `x -= 1` remain the only increment/decrement spellings (**recommended**) |
| **C** | Postfix only (`x++`, `x--`) — statement or expression returning `Void` / no usable value |
| **D** | Prefix only (`++x`, `--x`) — usable as expression (returns new value) |
| **E** | Statement-position only — both `++x`/`x++` allowed as standalone statements, never inside larger expressions |

**Rec: B.** S17 already provides a clear, uniform update operator family; `++`/`--` duplicate that job and reintroduce the prefix/postfix value distinction beginners stumble on. Rust, Swift, and Zig all omit them. Option A is the direct answer if the owner prioritizes C/C++ transliteration ergonomics over I8 on this one site.

## 4. Build order (only after D-INCR1 ratified ≠ B)

1. **`crates/jet-foundation/src/Syntax.rs`** — register `++`/`--` tokens + decision id if ratified.
2. **Lexer** — add `PlusPlus` / `MinusMinus` tokens; `x--` disambiguation rule (no space between operand and `--`).
3. **Parser** — prefix unary in `expr_unary`; postfix as new postfix layer (above field/index access or per ratified precedence table). Reject forms declined by ballot (e.g. postfix if D only).
4. **AST** — `Expr::PrefixInc` / `Expr::PostfixInc` (or desugar early to compound assign + temp).
5. **Sema** — reuse S17 LHS rules: `var`, `mut` binding, `self.field` on `mut self`, index assign sites. Types: ratified integer widths only (default proposal: `Int` + all sized ints; reject `Float`). Overflow: same as `+= 1` (D-NUMOPS2 trap). Expression typing: prefix → same integer type; postfix → ratified return type (old value vs void per option).
6. **Codegen** — lower to equivalent `+= 1`/`-= 1` + temp for postfix old-value semantics; no new runtime helpers.
7. **Diagnostics** (I4) — at minimum:
   - `E-INCR-IMMUT` — operand not mutable
   - `E-INCR-TYPE` — type does not support increment (e.g. Float if excluded)
   - `E-INCR-EXPR` — postfix/prefix used in expression context if ballot forbids (Option C/E)
   - Teaching error if user writes `++` and ballot is B: point at `+= 1`
8. **Tests** — `tests/ui/` fixtures per diagnostic; golden example `examples/features/NNN_increment.jet` if user-visible.
9. **Docs** — `docs/spec/spec.md` + `syntax-decisions.md` ratified entry; amend S17 cross-reference if needed.

## 5. Acceptance

- D-INCR1 ratified and recorded in `syntax-decisions.md`
- If A/C/D/E: parser → sema → codegen wired; overflow/mutability match `+= 1`
- If B: teaching diagnostic on `++`/`--` tokens (lexer may still recognize to emit helpful error)
- Every new diagnostic: code in `diagnostics.md` + `tests/ui` snapshot
- `nix develop -c cargo test` green
- No implementation starts before owner ratifies
