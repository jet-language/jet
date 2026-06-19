# Named-args follow-ups (D-NARG-D2, D-NARG-D4)

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c60

## What's ratified / the goal

Two ratified follow-ups to the named-args feature (D-NARG1, shipped
2026-06-19). Both are pure implementation work — no new user-facing syntax.

- **D-NARG-D2** — a default value may reference an *earlier* parameter:
  `fn box(w: Int, h: Int = w)`. Calling `box(5)` fills `h` with the
  argument supplied for `w` (so `h == 5`). Today default-fill treats each
  default as self-contained; it clones the default expression unchanged.
- **D-NARG-D4** — call-site label mismatches (transposed or unknown labels)
  get their *own* teaching diagnostic instead of being folded into E0104
  (the generic arity error). D-NARG1 currently emits E0104 for both.

## Current state to build on (cite files)

- `Source/Sema/mod.rs` — `FnSig.param_info: Vec<(String,bool)>` and
  `FnSig.defaults: Vec<Option<Expr>>` (lines 32–36); the same pair on
  `MethodSig` (49–52). `func_to_method_sig` (119) populates them, dropping
  `self`. Defaults are stored as raw `Expr` AST.
- **Two fill sites, structurally duplicated:**
  - `Source/Sema/CheckerItems.rs` `check_method_args` (43) — methods. Label
    check at 61–86 (emits E0104), default-fill at 88–111, arity error at
    113–131.
  - `Source/Sema/CheckerInfer.rs` ~2696 (label check) / ~2726 (default-fill)
    / ~2751 (arity) — free functions. Same logic, copied.
- **How a default is filled today** (both sites): when `args.len() <
  expected`, push `default_expr.clone()` as a synthetic `CallArg` at the
  call site. For a self-contained default like `false` or `100.0` this is
  correct. For `h: Int = w` it clones `Ident("w")` into the *caller's*
  scope, where `w` is some unrelated local or undefined.
- `Source/Comptime/Purity.rs` + interpreter walk the **post-sema AST**, so
  whatever the fill writes into the call is what comptime and codegen see
  (the `tests/comptime_diff.rs` differential battery enforces agreement).
- Diagnostic registry: E0101–E0124 are used; **E0125 is free** (verified by
  `rg "E0125" Source docs tests` — no hits) and sits naturally beside E0104.

## Proposed implementation (worked Jet example)

```jet
struct Rect {
    width: Int
    height: Int
}

impl Rect {
    // D-NARG-D2: `height` defaults to whatever `width` was given.
    fn square(width: Int, height: Int = width) -> Rect {
        return Rect{width: width, height: height}
    }
}

fn main() {
    s :: Rect.square(5)          // height fills from width -> 5
    print("{s.width}x{s.height}") // 5x5

    r :: Rect.square(5, 3)       // explicit height
    print("{r.width}x{r.height}") // 5x3
}
```

Expected output:

```
5x5
5x3
```

### Why a bare clone breaks (the crux)

`Rect.square(5)` with a clone-fill would expand to `square(5, width)` at the
call site. There is no `width` in `main`'s scope → codegen emits
`Rect::square(5, user_width)` → **rustc rejects generated code → I2/ICE**.
Filling an earlier-param ref is therefore not "extend the loop"; the default
must *resolve `width` to the argument that was supplied for parameter
`width`*, in the callee's parameter scope, not the caller's.

### Resolution mechanism (recommended: temp-bind, option B below)

Sema desugars the *call* so the earlier argument is named once and the
default refers to that name. The call

```
Rect.square(5)
```

lowers (in the checked AST) to a block-call:

```
{ a0 :: 5; Rect.square(a0, a0) }   // h's default `width` -> a0
```

Codegen stays dumb (I3): it lowers the already-desugared block straight to
Rust. No earlier argument is evaluated twice, even when it has side effects
(`Rect.square(make_w())` calls `make_w` once). When the referenced earlier
arg is already a literal, ident, or field access (no side effect, no
re-eval cost), the fill may inline it directly and skip the temp — a size
optimization, not a correctness requirement.

### Default validity rules (checked at definition time, in Registration)

- A default expression may reference only parameters **declared before it**
  and module-level constants/comptime bindings already in scope. (`w` before
  `h` is fine; `h: Int = total` where `total` is a later param is not.)
- Referencing a **later** parameter → new diagnostic (proposed E0126; or reuse
  E0107 — owner picks in card D-NARG-DIAG below).
- Referencing an **unknown** name → existing **E0107** (unknown name).
- Type of the default must assign to the parameter's declared type — already
  covered by the type pass once the default is in place.

### D-NARG-D4 — the new label-mismatch code

Claim **E0125** (sema). Replace the E0104 emission at the *label-mismatch*
sites only (`CheckerItems.rs` ~67 and `CheckerInfer.rs` ~2702). The E0104 at
the arity sites (`...len() != expected`) stays E0104 — that is genuinely a
"wrong number of arguments" error. One code covers both label sub-cases:

- **transposed** — the label names a real parameter, but a different one
  than sits at this position;
- **unknown** — the label names no parameter of the callee at all.

Proposed E0125 text (house voice, `Source/Sema/Diagnostics.rs` or inline):

```
Error [E0125]: label `height:` doesn't match the parameter `width` here
  --> tests/ui/method_label_mismatch.jet:14:23
     |
  14 |     r :: Rect.square(height: 5, width: 3)
     |                      ^^^^^^^
 Why: labels are checked documentation — each names the parameter at its own
      position, and arguments stay in the order they're declared
 Fix: write `width:` here, or drop the label
```

Unknown-label variant (same code, different what/fix):

```
Error [E0125]: `square` has no parameter named `depth`
 Why: a label must name the parameter at its position; `square` takes `width`, `height`
 Fix: use one of `square`'s parameter names, or drop the label
```

## Implementation sketch — file-level pipeline touchpoints

**parser** — no change. Labels and defaults already parse (D-NARG1). Default
expressions are already stored as `Expr`.

**sema (definition side)** — `Source/Sema/Registration.rs` (where `FnSig` /
`MethodSig` are built): add a **default-scope validity pass**. For each
default expression, walk its idents; each must be an earlier param name or a
known module constant. Earlier-param ref → record the source param index on
the default so the fill site knows what to bind. Later-param ref → E0126 (or
E0107). Unknown name → E0107. This runs once per definition, not per call.

**sema (call side)** — both fill sites (`CheckerItems.rs` ~88,
`CheckerInfer.rs` ~2726): when filling a default that references earlier
params, emit the *resolved* form. Recommended: hoist supplied args that a
default references into synthetic temp bindings and substitute the temp into
the default before pushing it as a `CallArg`; wrap the call in a block-expr
carrying those temps. Factor the shared fill+label logic into one helper
(e.g. `fill_defaults_and_check_labels`) called from both sites — kills the
current copy-paste and guarantees methods and free functions behave
identically. The label-mismatch branch in that helper emits **E0125**.

**codegen** — `Source/Codegen` lowers the desugared block-call verbatim. No
safety/scope decision (I3/R1). If the fill is done right, codegen never sees
an unresolved earlier-param ident.

**comptime** — none directly, but the differential battery must keep passing:
because the fill mutates the post-sema AST the interpreter reads, a comptime
call that uses a ref-default (`comptime X = box(5)`) must produce the same
value as the compiled run. Add a diff case (below) so a regression here is
loud.

**diagnostics** — register E0125 in `docs/spec/diagnostics.md` (and E0126 if
chosen); add a what/why/fix row in the same voice; new `tests/ui` snapshot.

## Test plan — ui snapshots + example(s)

- **Example (I5):** extend `examples/features/63_named_args.jet` (or a new
  `64_default_refs.jet`) with the `Rect.square` earlier-param-default case
  above + `examples/features/expected/*.out`. Golden test enforces it
  front-end-passes, contains no `unsafe`, and prints `5x5` / `5x3`.
- **ui snapshot — E0125 transposed:** `tests/ui/method_label_mismatch.jet`
  already exists for the old E0104 path; **re-point it to E0125** and
  re-bless (`UPDATE_EXPECT=1`). Add a free-function variant
  `tests/ui/fn_label_mismatch.jet`.
- **ui snapshot — E0125 unknown label:** `tests/ui/label_unknown.jet`.
- **ui snapshot — E0126/E0107 later-param ref:**
  `tests/ui/default_forward_ref.jet` (`fn f(a: Int = b, b: Int)`).
- **comptime diff:** add a `comptime` binding that calls a function relying on
  an earlier-param default to `tests/comptime_diff.rs`, asserting the
  interpreter and compiled run agree.
- **Arity unchanged:** confirm an existing E0104 arity snapshot still says
  E0104 (the carve-out must not steal the arity case).

## Risks & invariant check

- **I2/ICE (the live risk):** a clone-fill of an earlier-param ref emits Rust
  that references an undefined local. The temp-bind desugar removes this; the
  test plan's golden + diff cases are the guard. **This is the reason
  D-NARG-D2 is real work, not a one-liner.**
- **I3 (dumb codegen):** all resolution happens in sema; codegen lowers a
  fully-resolved block-call. Honored.
- **I4:** E0125 (and E0126 if used) ship with `docs/spec/diagnostics.md` rows
  + ui snapshots, or they don't exist.
- **I8 (ratchet):** no new feature surface — both are ratified refinements of
  an existing one.
- **Double-evaluation:** option A (raw substitution) would call a
  side-effecting earlier arg twice; option B (temp-bind) is recommended
  precisely to avoid it.
- **Two fill sites drifting:** unify into one helper so methods and free
  functions can't diverge again.

## Implementation note — engineering fork (no owner decision)

**Fill mechanism.** Pure backend, no user surface — implementer's call, not a
ballot item.
- **(A)** Substitute the earlier *argument expression* into the default.
  Simple; one place to change. **Breaks** on side-effecting earlier args
  (double-eval) and is more prone to the I2/ICE failure.
- **(B)** Temp-bind supplied args, default references the temp, wrap the call
  in a block-call. Correct under side effects, no double-eval; a bit more
  codegen-side plumbing for the block-call form.
- **Going with B.** Matches the owner's "hard work on the backend so the
  frontend feels magic." Inline-when-trivial (literal/ident/field) keeps output
  clean.

## Owner decision — diagnostic blessing only

Both decisions (D-NARG-D2, D-NARG-D4) are **ratified** — no user-facing syntax
is unresolved. The only owner-facing item is **product copy**: D-NARG-D4 mints
a new diagnostic code with new error text, and the later-param-ref case is a
code-id fork. Diagnostic text is user-facing copy (I4), so it gets an owner
nod. Card below.

### D-NARG-DIAG — diagnostic codes/text for the named-args follow-ups (rec A)

D-NARG-D4 splits the call-site label-mismatch error out of the generic arity
code (E0104). That needs a new sema code + its house-voice text. Separately,
referencing a *later* parameter in a default needs a code. This card blesses
both — implementation can't ship the snapshots (I4) until the text is settled.

- **Option A — mint E0125 for label mismatch + E0126 for later-param ref.**
  Two purpose-built codes; each teaches its own rule. E0125 covers both the
  transposed and unknown-label sub-cases (one code, two what/fix variants).

    ```jet
    // E0125 (transposed): label names a real param, wrong position
    r :: Rect.square(height: 5, width: 3)
    // Error [E0125]: label `height:` doesn't match the parameter `width` here
    //  Why: labels are checked documentation — each names the parameter at its
    //       own position, and arguments stay in the order they're declared
    //  Fix: write `width:` here, or drop the label

    // E0126 (later-param ref in a default)
    fn f(a: Int = b, b: Int) -> Int { return a }
    // Error [E0126]: a default can only use a parameter declared before it
    //  Why: defaults fill left to right; `b` isn't bound yet when `a` defaults
    //  Fix: reorder so `b` comes before `a`, or use a constant default
    ```

  Cost: two new code ids. Benefit: each error teaches its specific rule;
  later-param-ref gets a reorder hint E0107 can't give.

- **Option B — mint E0125 for label mismatch, reuse E0107 for later-param ref.**
  One new code; the forward-reference reuses the existing "unknown name" code
  (defensible — `b` genuinely isn't in scope yet at that point).

    ```jet
    fn f(a: Int = b, b: Int) -> Int { return a }
    // Error [E0107]: unknown name `b`
    //  Why: that name isn't in scope here
    //  Fix: define `b`, or check the spelling
    ```

  Cost: the generic E0107 text can't teach the left-to-right default rule, so
  the user gets a weaker fix. Benefit: one fewer code id to register.

**Recommendation: A.** E0125 is needed either way (D-NARG-D4 is ratified).
For the forward-ref, E0126's reorder hint is the teaching win — E0107's
generic "unknown name" sends the user looking for a missing definition when
the real issue is parameter order. Both codes are free (`rg "E0125|E0126"
Source docs tests` → no hits) and sit naturally after E0124. If the owner
prefers to hold the code-id count, B is the fallback (E0125 + E0107 reuse).
