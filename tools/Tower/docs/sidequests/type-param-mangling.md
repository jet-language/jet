# c148 — multi-character type-param names mangle wrong (ICE)

**Status:** planned, no owner decision needed (pure correctness — I2/R7 bug).

## Goal

A generic type or function whose type parameter has a multi-character name
(`Kind`, `Elem`, `Acc`) must behave exactly like a single-letter one (`T`,
`K`). Today only single-letter params work; a multi-char param is treated as a
concrete type and either breaks inference in sema or ICEs in codegen.

## Current state / root cause (verified)

Root cause is a single function:

`Source/Generics.rs:117`
```rust
pub fn is_type_var_name(name: &str) -> bool {
    name.len() == 1 && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}
```

It is a context-free heuristic: "one uppercase letter ⇒ type variable". A
declared param named `Kind` returns `false`, so every consumer treats it as a
concrete type and mangles it to `user_Kind`. The fix is to consult the
**declared type-param set** (which is available at every call site) instead of
the letter shape.

### Repro A — inference path (sema)

`/tmp/.../repro_c148.jet`:
```jet
struct Pair<Kind> { first: Kind  second: Kind }
fn make_pair<Kind>(a: Kind, b: Kind) -> Pair<Kind> {
    return Pair<Kind> {first: a, second: b}
}
fn main() {
    p: Pair<Int> @= make_pair(1, 2)
    print(p.first)
}
```
Actual: `E0904 can't figure out what 'Kind' should be`, then two `E0112`
("wants `Kind` for argument 1, but this is Int"), then `E0108`. Expected:
clean build printing `1`. Swapping `Kind`→`K` everywhere builds and runs.

### Repro B — codegen path (ICE), inference removed

`/tmp/.../repro_c148b.jet`:
```jet
struct Pair<Kind> { first: Kind  second: Kind }
fn main() {
    p: Pair<Int> @= Pair<Int> {first: 1, second: 2}
    print(p.first)
}
```
Actual: `internal compiler error: codegen reached a construct the typed IR
does not cover (main) — compiler bug (I2/R7)` (`Source/Codegen/Items.rs:999`).
Expected: prints `1`. The single-letter `K` version of B prints `1`.

### Verified call sites of the heuristic

Sema (breaks inference — repro A):
- `Source/Generics.rs:90-91` — `unify_types`: a `Type::Named(param)` only unifies
  against a concrete type when `is_type_var_name(param)`. `Kind` fails to unify
  ⇒ `infer_fn_subst` (`Source/Traits.rs:390-434`, called from
  `Source/Sema/CheckerInfer/calls.rs:2002`) returns `Err("Kind")` ⇒ E0904/E0112.
  `infer_fn_subst` already has the declared set (`type_params: &[TypeParam]`)
  in hand but never passes it down.
- `Source/Generics.rs:131` — `collect_free`/`free_type_params`.
- `Source/Sema/Diagnostics.rs:175`, `Source/Sema/CheckerOwnership.rs:451`.

Codegen / TIR (cause the ICE — repro B):
- `Source/Codegen/Context.rs:262-266` — `rust_type`: a `Type::Named(name)` that
  is `is_type_var_name && !type_names.contains` stays a bare `name`; otherwise it
  falls through to `Source/Codegen/Context.rs:318` → `user_{name}`. So nested
  `Kind` (`[Kind]`, `Option<Kind>`, `Map<K,Kind>`) mangles to `user_Kind`.
  (`Source/Codegen/Context.rs:193-197` `struct_field_rust` already special-cases
  a **top-level** field type against `s.type_params` — that is the precedent to
  generalize; it just doesn't recurse into compound field types.)
- `Source/Codegen/Context.rs:422`, `:787`, `:821` — same heuristic in
  `rust_param_type` and helpers.
- `Source/Codegen/TIR/subset.rs:398` `is_type_var_param_ty` (used by
  `field_ty_covered` at `:904`), `:665` `enum_is_covered_inner`, `:836`
  `ty_mentions_type_var` (used by `struct_is_generic` at `:827`), `:780`/`:850`
  `is_type_var` guards. With `Kind`, `struct_is_generic("Pair")` returns
  `false` (Pair looks concrete) and `field_ty_covered(Kind)` returns `false`
  (`Kind` is not a known struct), so `tir_covers(main)` is `false` ⇒ the
  `Items.rs:999` panic.

The declared set is available at every one of these sites: `StructDef.type_params`
for a struct field, `Func.type_params` for a function, `type_params` already
threaded into `infer_fn_subst`. Nothing needs new plumbing from the parser.

## Fix (staged)

1. **Sema inference.** Thread the declared param set into unification. Add a
   variant `unify_types_with(expected, found, params: &HashSet<String>, subst)`
   that treats `Type::Named(n)` as a type variable when `params.contains(n)`
   (keep `is_type_var_name` as the fallback only for the no-context callers, or
   drop it there too). Route `infer_fn_subst` (`Source/Traits.rs:401-415`) through
   it using its existing `type_params`. This clears repro A's E0904/E0112/E0108.

2. **Codegen `rust_type`.** Give `Cx` the in-scope type-param names for the item
   currently being emitted (a `RefCell<HashSet<String>>` set at the top of
   `emit_func`/`emit_method`/struct emission, mirroring how `current_fn` is
   already a `RefCell` on `Cx` — `Source/Codegen/Context.rs:91`). In `rust_type`'s
   `Type::Named` arm, render the bare name when it is in that set (covers nested
   positions), before the `user_{name}` fallback. Generalize
   `struct_field_rust` to recurse compound field types through the same check
   rather than only matching a top-level `Type::Named`.

3. **TIR subset detection.** Replace the bare `is_type_var_name` calls in
   `subset.rs` (`is_type_var_param_ty`, `ty_mentions_type_var`, the `is_type_var`
   guards) with checks against the declared param set for the struct/fn under
   examination, so `struct_is_generic`/`field_ty_covered` recognize a multi-char
   param exactly as they do `T`. This makes `tir_covers` route repro B through
   the existing generic-struct path instead of ICEing.

4. Keep the change mechanical: the single-letter path and existing snapshots
   must stay byte-identical (a name like `T` is still in `type_params`, so its
   classification is unchanged). Re-bless only if a *new* example is added.

## Test

- **Example + golden (I5):** add `examples/features/<NN>_generic_multichar.jet`
  (a `Pair<Kind>` with a `make_pair<Kind>` and a multi-char param used in a
  nested field, e.g. `items: [Elem]`) plus `expected/<NN>_…out`. This is the
  durable regression and exercises both the inference and codegen paths.
- **TIR integration:** add a `build_and_run` case in `tests/tir.rs` mirroring
  repro B (multi-char generic struct constructed + field read prints `1`), and a
  `covers(...)` assertion that a multi-char generic struct/fn is admitted, next
  to the single-letter cases at `Source/Codegen/TIR/mod.rs:2421`.
- **Sema unit:** a `unify_types_with` test asserting `Pair<Kind>` unifies with
  `Pair<Int>` and binds `Kind=Int`.

## Risk / scope

- No new diagnostic: this removes spurious errors; no I4 snapshot to add. Verify
  no existing `tests/ui` snapshot depended on the wrong-behavior errors
  (none expected — the heuristic predates multi-char params being used).
- Over-broad classification risk: a declared **concrete** type whose name is a
  single uppercase letter (`struct P`) must still read as concrete. The set-based
  check is *narrower* than today only when the name is genuinely a declared param,
  and the existing `!cx.struct_fields.contains(name)` guards (`subset.rs:780,850`)
  already encode that precedence — preserve them.
- Touches sema + codegen + TIR; gate with the full `cargo test` once, targeted
  `--test tir` / `--test corelib` while iterating.

## Open Owner-Q

None. Pure correctness fix; no user-facing syntax or semantics change (multi-char
param names are already accepted by the parser — they just miscompile).
