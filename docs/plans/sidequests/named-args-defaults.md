# Sidequest: Named function inputs & default values (S61)

**Status:** ratified 2026-06-19 (D-NARG1 = A, D-NARG2 = A) — ready to implement on owner's word.

## Goal

Finish S61 — optional call-site argument labels (`f(x: 1, y: 2)`) and trailing
parameter defaults (`fn f(x: Int = 0)`) — so the feature works end-to-end for
**free functions and methods**, survives `jet fmt` round-trips, and ships with
examples + ui snapshots. The core syntax is already **ratified** (S61: labels
are `name: value`, positional order is fixed, labels never reorder, only
trailing params take defaults). Most of the free-function path is built; this
sidequest closes the gaps and surfaces the few decisions S61 did not settle.

## Current state (verified by reading the code)

S61 is ~70% implemented, not greenfield. What exists:

- **AST** — `ast::Param.default: Option<Box<Expr>>` (ast.rs:589) and
  `ast::CallArg.label: Option<(String, Span)>` (ast.rs:949) already carry the
  data, with S61 doc comments. (ast.rs was not part of the module split.)
- **Parser** — `param()` (Source/Parser/Items.rs) parses a trailing `= expr` default
  via `expr_no_struct_lit()`. `call_arg()` (Source/Parser/Expressions.rs) parses a `name:`
  label, disambiguating `ident :` from `::` (Rust path) by peeking two tokens.
- **Sema (free functions)** — `FuncSig` carries `param_info: Vec<(String, bool)>`
  and `defaults: Vec<Option<Expr>>` (Source/Sema/mod.rs — `FuncSig`), populated in
  `func_to_sig` (Source/Sema/mod.rs). `check_call` (Source/Sema/CheckerInfer.rs):
  - validates each label against the param name at that position, emitting
    **E0104** on mismatch (the `sig.param_info` label loop);
  - fills omitted trailing params from `defaults` by pushing synthetic
    `CallArg`s, respecting the required/trailing split
    (`take_while(|d| d.is_none())`);
  - then runs the existing arity check.
- **Fmt (params)** — `fmt_param` emits ` = default` (Source/Formatter/Items.rs).
- **Codegen** — no special path needed: sema injects default exprs into
  `call.args` and labels are check-only, so codegen emits plain positional args.
- **Example** — `examples/showcase/library.jet` declares `fn greet(name: String,
  loud: Bool = false)` and calls `greet("world")` / `greet("world", true)`
  (defaults + positional only; **no call-site label is exercised anywhere**).

Gaps (the actual work):

1. **Methods get nothing (ratified to fix — D-NARG1 = A).** `MethodSig`
   (Source/Sema/mod.rs) has no `param_info`/`defaults`; `check_method_args`
   (Source/Sema/CheckerItems.rs) does plain positional arity + type checks and
   ignores `arg.label` entirely. So `obj.m(x: 1)` and method defaults are
   silently unsupported (label is parsed, then dropped). Same for the
   static-method and enum-variant-constructor call paths that route through
   `check_method_args`. D-NARG1 = A ratifies closing this gap.
2. **Fmt label-drop is FIXED.** `fmt_call_args` (Source/Formatter/Expressions.rs) now emits
   `arg.label` — when `arg.label` is `Some`, it writes `name: ` before the expr
   (canonical `name: value` spacing). So `f(x: 1)` round-trips correctly; the
   earlier label-loss bug no longer exists. No fmt work remains here — D-NARG2 = A
   (preserve-as-written) is ratified.
3. **Default injection is span-poor.** Synthetic default `CallArg`s use
   `call.name_span` (the default-fill loop in Source/Sema/CheckerInfer.rs); a type
   error inside a default expression points at the call name, not the `fn`'s
   default. Minor, but worth a span.
4. **L2401 covers free functions, not methods.** The advisory lint (positional
   `Bool` param on a `pub` fn) is documented in diagnostics.md:427 and **is fully
   implemented for free functions** — it fires via `Diagnostic::lint` in
   Source/Sema/Registration.rs and Source/Sema/Bundle.rs (the `f.is_pub` param loop),
   with a ui snapshot at `tests/ui/l2401_positional_bool.{jet,stderr}`. The gap is
   that the registration loop iterates free `fn`s only, not type methods — so
   `pub` methods with a positional `Bool` param get no lint. This folds into gap 1
   (the method path gets nothing).
5. **No call-site-label coverage.** No example or ui snapshot exercises a
   correct `name:` label or the E0104 mismatch path, so per I4/I5 those
   behaviors "don't exist" yet.
6. **Validation holes** in the existing path (decisions below): a label on an
   arg *beyond* the param count, a default expr that references another
   parameter, a non-const default at comptime, and default + label interaction
   are unspecified.

## Proposed approach (workflow loop order)

Write the failing ui fixture / example first, then parser → sema → codegen →
fmt → docs, per CLAUDE.md.

### Parser
Largely done. Add only what the decisions below require, e.g. reject a
leading/standalone-comma label edge or a `mut`/`take` prefix *before* a label
if the owner wants a fixed `label: conv expr` vs `conv label: expr` order.
Keep `expr_no_struct_lit` for defaults (a `{` after `=` would otherwise read as
a struct literal vs a block).

### Sema
1. **Lift labels + defaults to methods.** Add `param_info` and `defaults` to
   `MethodSig` (Source/Sema/mod.rs); populate in `func_to_method_sig`
   (Source/Sema/mod.rs). In `check_method_args` (Source/Sema/CheckerItems.rs), run the
   same label-match (offset by `self`) and default-fill logic `check_call`
   (Source/Sema/CheckerInfer.rs) already has. Factor the shared logic into one
   helper (`apply_labels_and_defaults(args, param_info, defaults, self_offset)`)
   so free-fn and method paths can't drift.
2. **Tighten validation** per the ratified decisions: label index ≥ param count
   → dedicated diagnostic (not a generic arity message); default expr must be a
   self-contained expression that does not reference other params (decision D2).
3. **Span the synthetic default** at the param's `default` expr span, not the
   call name.
4. **Extend L2401 to methods.** Already fires on free `pub fn`s in
   Source/Sema/Registration.rs / Source/Sema/Bundle.rs; extend the same param loop to
   `pub` method declarations with a positional `Bool` param that has no default
   (callee side, once per decl).

### Codegen
No change expected — verify with a golden example that a defaulted/labeled call
lowers to the right positional Rust call. (R1: codegen stays dumb.)

### Fmt
Label emission already works: `fmt_call_args` (Source/Formatter/Expressions.rs) writes `name: `
before the expr when `arg.label` is `Some` (S44 spacing — space after `:`, none
before). fmt **preserves** user labels as-written and never adds or strips them
(D-NARG2 = A, ratified). No fmt work remains for the free-function path.

### Diagnostics
- Reuse **E0104** for arity, or split label-mismatch into its own code (D4).
- Add a ui snapshot for: correct label, label mismatch, label past arity,
  omitting a defaulted trailing param, omitting a *required* param when a later
  one has a default (must still error).
- L2401 free-fn fixture already exists (`tests/ui/l2401_positional_bool.*`); add
  a method-path fixture when the lint is extended to methods.

### Examples / tests
- Extend `examples/showcase/library.jet` (or a new
  `examples/features/NN_named_args.jet` + `expected/NN_named_args.out`) to call
  with a label and to omit a defaulted arg, so golden tests run the compiled
  output.
- Add ui fixtures under `tests/ui/` for each diagnostic above.

## Decisions (D-NARG1/D-NARG2 resolved 2026-06-19; D2/D4/D5 still open)

S61 already ratified the big questions (label spelling `name:`, fixed order,
labels-never-reorder, trailing defaults). D-NARG1 and D-NARG2 are now also
resolved. D2/D4/D5 remain open.

- **D-NARG1 = A — Methods/constructors in scope.** RESOLVED. Named args and
  defaults apply to methods and constructors, not just free functions.

  ```jet
  // Before (today): label parsed then dropped, default unsupported on the method.
  fn draw(self, filled: Bool = false) { … }   // default never fills
  rect.draw(filled: true)                      // label silently ignored
  // D-NARG1 = A: both behave like a free fn.
  ```

- **D2 — May a default reference earlier params?** (`fn f(x: Int, y: Int = x)`).
  OPEN. Recommend no in v1 — defaults are self-contained; reject param refs with
  a teaching error.

  ```jet
  fn box(w: Int, h: Int = 1)   { … }   // OK: self-contained default
  fn box(w: Int, h: Int = w)   { … }   // rejected in v1 (refs earlier param)
  ```

- **D-NARG2 = A — fmt preserves call-site labels as written.** RESOLVED. fmt
  never adds or strips labels; canonicalization deferred to the LSP quick-fix
  (S14/M6).

  ```jet
  greet("world", loud: true)   // preserve: stays as written
  greet("world", true)         // preserve: stays unlabeled (no auto-add)
  ```

- **D4 — Own diagnostic code for label-mismatch?** OPEN. Currently folded into
  E0104. Recommend a dedicated code (clearer "transposed argument" teaching) —
  owner copy, surfaced not decided.

  ```jet
  fn move_to(x: Int, y: Int) { … }
  move_to(y: 1, x: 2)   // today: E0104 (arity-flavoured); D4: dedicated "label
                        //        doesn't match parameter at this position" code
  ```

- **D5 — Interaction with multiple-constructor-shapes and future overloads.**
  OPEN. S61 assumes one signature per name; named args do not enable overload
  resolution by label. Resolve constructor-shapes design first.

  ```jet
  // S61 does NOT make these two distinct overloads:
  fn open(path: String) { … }
  fn open(fd: Int)      { … }      // still a duplicate-name error, not an overload
  ```

## Test / acceptance checklist

- [ ] `fn f(x: Int = 0)` parses; `f()` fills the default; `f(5)` overrides.
- [ ] Required-then-default ordering enforced: a default may not precede a
      required param at a call (omitting the required one still E0104).
- [ ] `f(x: 1, y: 2)` checks labels against positions; mismatch → E0104 (or D4
      code) pointing at the label span.
- [ ] Method calls `obj.m(label: v)` and method defaults work identically to
      free functions.
- [ ] `jet fmt` round-trips a labeled call without dropping or reordering labels.
- [ ] Codegen output for a defaulted/labeled call compiles and prints the
      expected value (golden example + `expected/*.out`).
- [x] L2401 fires on a free `pub fn` with a positional non-default `Bool` param
      (ui snapshot `tests/ui/l2401_positional_bool.*` exists).
- [ ] L2401 also fires on a `pub` method with the same shape, with a ui snapshot.
- [ ] Default expr that references another param is rejected (if D2 = no).
- [ ] ui snapshots exist for every new/changed diagnostic (I4).
- [ ] `nix develop -c cargo test` green; no new `unsafe` in generated code (I1).

## Blast radius

`Source/Sema/{mod,checker_items,checker_infer,registration,bundle}.rs` (MethodSig +
check_method_args + shared label/default helper, L2401-on-methods),
`Source/Parser/{items,exprs}.rs` (only if a decision adds grammar),
`docs/spec/diagnostics.md` + `tests/ui/` (snapshots),
`examples/showcase/library.jet` or a new `examples/features/` pair +
`expected/`. AST, codegen, fmt label emission, and the free-function sema path
(including L2401 for free fns) are already in place.
