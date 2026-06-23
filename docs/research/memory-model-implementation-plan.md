# Memory/access-capability model — implementation plan

Readiness map for `docs/prompt-memory-model-final.md`. Pairs the prompt's nine phases
with the **actual** compiler code (audited 2026-06-23, file:line below), and splits work
into **BUILD NOW** (non-gated) vs **WAITS ON GATE** (blocked on an owner decision). The
spelling `T`/`~T`/`^T`/`&T`/`*T` is ratified (D-CAP7); three semantic decisions gate the
rest: **D-CAP8** (unmarked-`T` default), **D-CAP9** (`&`/`*` expr grammar + `*T` vs
`Ptr<T>`), **D-CAP10** (overloads). Cards are live in
`tools/Tower/docs/ballots/decision-ballots.md`.

## 1. Repository audit (Phase 1 — done)

The capability spine already exists; this is a migration + extension, not a greenfield
build.

| Concept | Where it lives today |
|---|---|
| `AccessConvention {Read, Mutate, Move}` | `Source/AST.rs:7-15`; on `Param.convention` (`AST.rs:738`), `CallArg.convention` (`AST.rs:1224`), receiver via `MethodSig.self_conv` (`Sema/mod.rs:45,159`) |
| Single producer of conventions | `parse_access_prefix()` `Source/Parser/Expressions.rs:1569-1633`; unmarked → `Read` at **:1631** |
| Convention consumers (the diagnostic engine) | `Sema/CheckerItems.rs:216-340`, `Sema/CheckerCoreLib.rs:152-170`, `Sema/CheckerInfer.rs:3382-3520`; `Sema/Registration.rs` seeds `mutable`/`param_conv` |
| Capability keywords `mut`/`take`/`view`/`ref` | spellings `Source/Syntax.rs:111-121`; lexed `Lexer/mod.rs:46-49`; parsed via `parse_access_prefix` + `Items.rs` (`view` return :708, `ref` field :1525) |
| Type parsing (sigil insertion point) | `type_inner` top `Source/Parser/Types.rs:260`; existing `Type::Shared` `AST.rs:31` (the `&T` analog) produced from `Shared<…>` `Types.rs:467` |
| Unary-prefix expr parsing | `expr_unary_inner` `Source/Parser/Expressions.rs:463-514`; `*`→Deref at **:507** |
| Ownership diagnostics | E0201 `CheckerItems.rs:248`; E0202 `:201,280`; E0205 `CheckerCore.rs:757`; E0206 `CheckerInfer.rs:38`; E0208 `CheckerInfer.rs:388`; E0120 `CheckerCore.rs:834`; E0111 `CheckerItems.rs:303` |
| Escape/region machinery (`&T` composes here) | E0631/E0632 `Sema/CheckerOwnership.rs:177-260`; `region{}` `Parser/Statements.rs:708`; arena views `CheckerOwnership.rs:196-219`; E1102 spawn-sendability `:575` |
| Raw / unsafe | `Ptr<T>` `Syntax.rs:173` (S58); `#Unsafe` block `Parser/Statements.rs:8-57`, fn-attr `Items.rs:736`; raw reject = E0208 |
| `api = stable | explicit` | parsed `Manifest.rs:436`; **no signature-emission yet** |
| Overloading | **none** — second same-name def = E0105 `Registration.rs:1353`; single-name map `:158` |

**Two audit corrections to the prompt** (the prompt's "verified current" notes are partly
wrong):
- `~` is **not lexed at all** — there is no `Tilde` token (`Lexer/Scan.rs` has no `'~'`
  arm). The `~T`/`~x` sigil is on a clean glyph; S83's `~~` trait-attach is spec-only, not
  implemented. So `~`/`^`/`&` prefixes are **position-disambiguated and free**.
- The prompt says "S58 already uses `&x`=address-of and `*p`=deref." Half true: `*p` deref
  is real (E0208), but **address-of is `mem.address_of(x)`** (`Syntax.rs:182`), a call, not
  a `&x` sigil. The only genuine expression-position collision is **`*x` (deref vs
  raw-of)** — this is the heart of D-CAP9.

## 2. The gate — what each open decision blocks

- **D-CAP8 (c125)** changes the meaning of `parse_access_prefix` default (`Expressions.rs:1631`)
  and re-points/removes the E0202/E0205 triggers. **Blocks:** Phase 4 inference resolution,
  the default semantics, and any example whose unmarked param mutates.
- **D-CAP9 (c127)** decides `*x` (deref vs raw-of), the deref spelling, and `*T` vs
  `Ptr<T>`. **Blocks:** all expression-position `*` parsing (`expr_unary_inner:507`,
  E0208) and the `*T` type form. (`~x`/`^x`/`&x` are NOT blocked — they're free.)
- **D-CAP10 (c128)** decides whether capability overloads exist. **Blocks:** the overload
  resolution section + its ambiguity diagnostic. Recommendation A collapses it to single-fn
  call-site disambiguation (no engine change).

## 3. BUILD NOW — non-gated work (safe to implement before the owner rules)

Ordered; each step is independently testable. None of these changes the unmarked-`T`
default, expression-position `*`, or overloading.

1. **Lex `~` / `~~`.** Add `Tilde`/`TildeTilde` tokens (`Lexer/Tokens.rs`, `Lexer/Scan.rs`).
   `~` was free, so this is purely additive.
2. **`AccessCapability` enum.** Grow `AccessConvention {Read,Mutate,Move}` →
   `{Infer, Read, Write, Move, Share, Raw}` (`AST.rs:7`). Rename `Mutate`→`Write` across the
   ~40 match sites; add `Infer`/`Share`/`Raw` variants. Keep `Infer` *unused as a default*
   until D-CAP8 (gate); for now unmarked still resolves `Read` so behavior is unchanged —
   this is a mechanical type widening with a green test suite.
3. **Type-position sigils.** Parse `~T`/`^T`/`&T` at `type_inner` top (`Types.rs:260`) into
   the capability on the param/field. Map `&T` onto the existing `Type::Shared` path. (`*T`
   waits on D-CAP9.)
4. **Receiver sigils.** `~self`/`^self`/`&self` already flow through `parse_access_prefix`
   then the `KwSelf` check (`Items.rs:1073-1086`) — wire the sigil branch so they parse
   without the `mut self` keyword.
5. **Call-site sigils for explicit params.** `~x`/`^x`/`&x` in argument position
   (`parse_access_prefix` at `Expressions.rs:1542`) — free glyphs, position-disambiguated.
6. **Keyword → sigil migration (D-CAP7).** Turn `mut`/`view`/`take` into S14 teaching errors
   pointing at the sigils (`Lexer`/`Parser`); update every message that hardcodes
   `KW_MUTATE`/`KW_MOVE`/`KW_VIEW` to the sigil text. Snapshot-bless.
7. **`#Unsafe("reason")` merge (D-UNSAFE2, already ratified, not yet built).** Make `#Unsafe`
   take the reason argument and retire the separate `#Audit` (`Parser/Statements.rs:10-30`).
   Independent of the sigils; unblocks the `*T` examples later.
8. **Diagnostic re-wording to capability vocab.** Re-voice E0201/E0202/E0205/E0206/E0120 to
   the prompt's capability language (move/edit/share, not borrow jargon) — same triggers for
   now. New snapshots.

## 4. WAITS ON GATE — sequenced after the owner rules

- **After D-CAP8:** flip `Expressions.rs:1631` default to `Infer`; build the Phase-4
  deterministic constraint solver (`Sema`, new module) that elevates `Infer →
  Read/Write/Move/Share` over a **sorted** fixed point; re-point E0202/E0205; add the
  differential determinism test (same source+flags ⇒ same resolved signatures). If the owner
  picks **C**, add signature-freeze at `api: explicit` boundaries (Phase: Public API).
- **After D-CAP9:** implement `*x` per the ruling (rec: `*x` = raw-of only, dereference →
  postfix `p.*` per Odin/Jai prior art, `*T` canonical / `Ptr<T>` deprecated alias); update
  E0208 into a teaching error pointing at `p.*`; parse `*T` in type position. (`.read()`/
  `.write()` stay for explicit/volatile ops.)
- **After D-CAP10:** if **A** (rec), turn the doc's "ambiguous overload" diagnostic into a
  single-fn "call needs/forbids stronger capability" diagnostic — no registration change. If
  **B**, re-key `Registration.rs:158` maps by (name, capability) + rank rules.

## 5. Phase → code crosswalk (prompt Phases 2-9)

| Prompt phase | Concrete site | Gate |
|---|---|---|
| 2 Parsing (type pos) | `Types.rs:260` | none (`*T` waits D-CAP9) |
| 2 Parsing (expr pos) | `Expressions.rs:463-514`, lexer `~` | `*x` waits D-CAP9 |
| 3 AST/HIR | `AST.rs:7` enum widen | none |
| 4 Constraint solving | new `Sema/Capability.rs`; consumers `CheckerItems.rs:216` | **D-CAP8** |
| 5 Type checking | `CheckerInfer.rs:3382-3520`, `CheckerOwnership.rs` | D-CAP8 |
| 6 Lowering/IR | `Codegen/TIR.rs:388`, `Traits.rs:480-491` | none (post-resolution) |
| 7 Codegen | `Source/Codegen/` | none |
| 8 Optimization | passing-strategy choice, post-resolution | none (see §7) |
| 9 Tests | `tests/ownership.rs`, `tests/capabilities.rs`, `tests/ui/*`, `tests/golden.rs` | per-feature |

## 6. Migration notes (deliverable 6)

- `mut x`/`mut self` → `~x`/`~self`; `take x`/`take self` → `^x`/`^self`; `view` return →
  `&`; default-read → bare `T`. Retired keywords become S14 teaching errors (not silent
  removals).
- D-CAP1 word vocabulary (`view`/`edit`/`take`/`share`) → sigils; `copy` stays a verb (no
  sigil — D-CAP7 closed the set at five).
- `Ptr<T>` → `*T` (canonical) per D-CAP9 rec; `Ptr<T>` kept as a deprecated alias that
  teaches `*T` until removed.
- `#Audit("…")` → argument of `#Unsafe("…")` (D-UNSAFE2).

## 7. Determinism guarantee (deliverable 8)

Capability resolution is a **deterministic constraint fixed point**, independent of
optimization:
- Unmarked params start at `Infer`; constraints come only from semantic uses (field read →
  ≥Read; field assign → Write; call to `^` param → Move; escape/retain → Share; `*x` → Raw +
  unsafe). No performance input ever raises a capability.
- Iterate to a fixed point over a **sorted** symbol/constraint order (no HashMap iteration
  order in the solver) so the same source + compiler version + target/flags yields identical
  resolved signatures — enforced by a differential test (Phase 9).
- Conflicts (use-after-move, write-while-read, short-lived share) are errors, not silent
  downgrades.

## 8. Capability vs optimization separation (deliverable 9)

Two strictly separated layers (prompt's "Critical Determinism Rule"):
- **Semantic capability** (this plan) decides *what is permitted* — resolved before any
  optimization, frozen into the public signature.
- **Passing strategy** (optimizer) decides *how* — by-value/register vs readonly-view vs
  exclusive-view vs move-elision vs region handle. It may change representation but **never**
  whether the caller loses ownership, whether mutation/escape is allowed, whether raw access
  occurs, or the public capability signature. The optimizer reads only deterministic inputs
  (type size, copy/move traits, drop, escape/alias analysis, ABI, opt level, explicit PGO
  artifact). No optimizer feedback edge into the capability solver.

## 9. Unresolved conflicts / blockers (deliverable 7)

1. **Unmarked-`T` default** — D-CAP8 (owner). Plus sub-question: does a *caller* write the
   sigil for an *inferred* param, or stay bare? (Owner Q on the card; rec: require it.)
2. **`*x` deref vs raw-of**, deref spelling, `*T` vs `Ptr<T>` — D-CAP9 (owner).
3. **Overloads vs S14** — D-CAP10 (owner); rec collapses to single-fn disambiguation.
4. **api signature emission** doesn't exist yet (`Manifest.rs:436` parses the field only) —
   build with the realize/publish pipeline once D-CAP8 = C.
5. **`copy` residual** — D-CAP7 left duplication as a verb with no sigil; confirm the
   call-site spelling when the move examples that need a copy land.
