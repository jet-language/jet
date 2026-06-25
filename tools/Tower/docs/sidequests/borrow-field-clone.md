# c150 — borrowed non-Copy param assigned to a field isn't cloned (E0507 / I2)

**Status:** planned, no owner decision needed (pure correctness — I2 violation).

## Goal

A method that assigns a borrowed (Read-convention) non-Copy parameter straight
into a field must compile. Today it emits a move out of a shared reference, so
rustc rejects the generated code with E0507 — an I2 violation (rustc speaks to
the user). It must instead clone the value on store, exactly as every other
borrowed-value store in the TIR already does.

## Current state / root cause (verified)

### Repro

`/tmp/.../repro_c150.jet`:
```jet
struct Ledger { rows: [Int] }
impl Ledger {
    fn put_back(~self, s: [Int]) {
        self.rows = s
    }
}
fn main() {
    l: Ledger := Ledger { rows: [1, 2] }
    l.put_back([3, 4])
    print(l.rows[0])
}
```

Actual — the ICE banner (I2) with rustc's E0507:
```
internal compiler error: the generated Rust did not compile. ...
error[E0507]: cannot move out of `*user_s` which is behind a shared reference
468 |         ((*self)).user_rows = (*user_s);
help: consider cloning the value ...
468 +         ((*self)).user_rows = user_s.clone();
```
Expected: builds and prints `3`. Taking the param by `^` (Move) is the
documented workaround, not the fix.

### Root cause

The store-lowering site is the `LValue::Field` arm of `Stmt::Assign` in
`Source/Codegen/TIR/lower.rs:862-870`:
```rust
LValue::Field { base, field, span } => {
    let field_expr = Expr::Field(base.clone(), field.clone(), *span);
    let place = emit_tir_expr(&lower_expr(&field_expr, cx, env), cx);
    TStmt::Assign { place, op: *op, value: lower_expr(value, cx, env) }
}
```
The RHS is lowered with a plain `lower_expr(value, …)`. For `s` — a `[Int]`
Read param — its env slot is the deref'd place `(*user_s)` (a borrow;
`LowerEnv::is_borrowed`, `Source/Codegen/TIR/lower.rs:89`), and `lower_expr`
emits exactly `(*user_s)`. Assigning that into `(*self).user_rows` *moves* out
of the shared `&` borrow ⇒ E0507. No clone decision is made at this store.

### The precedent to mirror

Borrowed-value stores elsewhere already clone:

- **Enum payload arg** — `lower_enum_arg` (`Source/Codegen/TIR/lower.rs:1758-1778`):
  ```rust
  let borrowed = matches!(e, Expr::Ident(name, _) if env.is_borrowed(name));
  let clone = payload_ty.is_some_and(|t| !t.is_scalar()) && borrowed;
  ```
  emitted as `({…}).clone()` via the `TEnumArg.clone` flag
  (`Source/Codegen/TIR/emit.rs:736`).
- **Match subject** — `LowerEnv::bare_borrow` clones the borrow itself
  (`Source/Codegen/TIR/lower.rs:93-94`, `(user_light).clone()`).
- **Destructure binds** — `(tmp).field.clone()` (`Source/Codegen/TIR/emit.rs:220,232`).

There is also a ready emit node: `TExprKind::Clone(recv)` →
`({recv}).clone()` (`Source/Codegen/TIR/emit.rs:810-811`).

This clone-on-store is a mechanical lowering decision (not a checking decision),
so doing it in the TIR is consistent with I3 and with the existing precedent —
the same way `lower_enum_arg` decides it.

## Fix (staged)

1. **Store-lowering site.** In the `LValue::Field` arm
   (`Source/Codegen/TIR/lower.rs:862`), compute the same clone predicate as
   `lower_enum_arg`:
   ```rust
   let borrowed = matches!(value, Expr::Ident(n, _) if env.is_borrowed(n));
   let mut v = lower_expr(value, cx, env);
   if borrowed && !v.ty.is_scalar() && op.is_none() {
       v = TExpr { ty: v.ty.clone(), kind: TExprKind::Clone(Box::new(v)) };
   }
   ```
   Use `TExprKind::Clone` so emit produces `user_s.clone()` (the `Clone` node's
   `emit_tir_expr(recv)` yields the bare place; confirm it renders the borrow,
   not a re-deref — mirror `bare_borrow` if the deref needs stripping).
   Gate on `op.is_none()`: a compound `+=`/`-=` reads-then-writes and must not
   clone the RHS.

2. **Cover `LValue::Local` too** (`Source/Codegen/TIR/lower.rs:838`) only if the
   same E0507 reproduces for `local = borrowed_param` of a non-Copy type; verify
   with a second repro before widening (the report isolates the bug to a field
   store, so keep scope minimal unless the local case also fails).

3. Verify the clone predicate matches `lower_enum_arg` exactly so behavior is
   uniform: `is_scalar()` false ⇒ non-Copy ⇒ clone; `borrowed` ⇒ Read/Infer
   deref'd slot only.

## Test

- **Example + golden (I5):** add `examples/features/<NN>_field_store_borrow.jet`
  (the `Ledger`/`put_back` repro, printing `3`) + `expected/<NN>_…out`. This
  builds the generated Rust, so it is the direct E0507 regression.
- **TIR integration:** a `build_and_run` case in `tests/tir.rs` mirroring the
  repro, asserting exit 0 and stdout `3`.
- (No new diagnostic — this is an ICE that should never have fired, so I4 adds
  nothing; the golden/build *is* the proof.)

## Risk / scope

- **No over-cloning of Copy types:** the `!v.ty.is_scalar()` guard skips
  `Int`/`Float`/`Bool`/`Char` (a scalar Read param is by-value anyway, and
  `is_borrowed` is false for it). Confirm a scalar-field assignment snapshot is
  unchanged.
- **No over-cloning of owned/Move params:** a `^s` param's slot is non-deref
  (by value) ⇒ `is_borrowed` is false ⇒ no clone. This is exactly the existing
  workaround, now made unnecessary; verify the `^` form still emits a plain move.
- **Compound assign:** the `op.is_none()` guard avoids cloning the RHS of
  `field += s`.
- Single, well-scoped lowering site; mirrors a proven helper. Run the full
  `cargo test` once at the end (golden snapshots are the main blast radius);
  targeted `--test tir` while iterating.

## Open Owner-Q

None. Pure correctness; no user-facing syntax or semantics change. The clone is
the obvious, only-correct lowering (rustc itself suggests `user_s.clone()`), and
it matches how every other borrowed store already lowers.
