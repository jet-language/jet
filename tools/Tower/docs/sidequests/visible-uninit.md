# Plan: Visible uninitialization (D-UNINIT1)

**Status: READY to implement — no open owner decision. D-UNINIT1 ratified 2026-06-21 (opt C, `#Uninit` marker); the prior codegen blocker (D-FIXARR1) is ratified AND implemented (verified 2026-06-25). Remaining work is pure implementation: MaybeUninit codegen + wire the parser dispatch + snapshots/example.**

**Verification (2026-06-25):**
- D-FIXARR1 stack-array lowering is **implemented**: `Type::FixedList { elem, len }` lowers to a real Rust array `[E; N]` in codegen (`Source/Codegen/Context.rs:378-380`), in FFI (`Source/FFI.rs:431-432`), and array literals/fan-out emit `[…]` (`Source/Codegen/TIR/emit.rs:1142-1149,1323`). `Vec<…>` now lowers only plain `Type::List` (`Context.rs:249`), not `[T#N]` — so the old "[T#N]→Vec" claim below is **stale**; the prerequisite is satisfied. `MaybeUninit<[T; N]>` is now sound.
- Sema write-before-read proof is **done and wired**: `check_uninit_binding` (`Source/Sema/CheckerCore.rs:2018`) is called from `check_binding` (`CheckerCore.rs:2101`); gate E0424 + POD E0423 there; read-hook E0420 (`Source/Sema/CheckerInfer/expr.rs:239-254`); compound-assign E0420 (`CheckerCore.rs:481-494`); `mut`-arg clears (`expr.rs:296,470`); if-merge intersection (`CheckerCore.rs:1495-1594`); loop restore (`CheckerCore.rs:1037,1059,1206,1257`). Flow-state field `uninit` (`Source/Sema/mod.rs:540`).
- Parser `uninit_binding` written with E0421/E0422 (`Source/Parser/Statements.rs:360-405`) but **still UNWIRED** in the `TokKind::Hash` dispatch (`Statements.rs:761-764`).
- Codegen for `b.uninit` is **not yet emitted** — only the comptime subset excludes it (`Source/Codegen/TIR/subset.rs:983-1036`). No `MaybeUninit` line is generated.

**Landed (green, all 42 test binaries pass):**
- AST `Binding.uninit` field (placeholder `Expr::Int(0,…)` init, so the 46 `b.init` walkers need no edits).
- `Syntax::ATTR_UNINIT`.
- Parser method `uninit_binding` (`#Uninit name: Type`, E0421 missing-type, E0422 has-initializer) —
  **written and `#[allow(dead_code)]`, intentionally NOT wired into the `TokKind::Hash` dispatch**
  until sema+codegen make the path safe (no mis-compiling/unsafe surface exists meanwhile).

**Sema dataflow DONE (green, inert until parser wired):** `Checker.uninit` flow-state;
`check_uninit_binding` (gate E0424 on `use core.mem`, POD-only E0423, declare, mark
uninit); read-hook E0420 in CheckerInfer (mirrors use-after-move); writes clear it
(plain `name = …`; `mut`-pass via `clear_uninit_mut_args`); compound `+=` is E0420;
branch-merge threaded through `check_if` (intersection: stays uninit if uninit on any
path; no-`else` keeps fall-through) and `check_branches` (conservative — switch doesn't
initialize); loops restore the pre-loop set (0-iteration path). Parser `uninit_binding`
(E0421/E0422) written, still UNWIRED.

## Prior blocker — RESOLVED (D-FIXARR1 implemented)

`MaybeUninit::uninit().assume_init()` (the ratified lowering) requires the binding to be
a **stack value** of a fixed size. This was originally blocked because `[T#N]` lowered to
a heap `Vec<T>` (an uninitialized `Vec` is UB; the safe `vec![0u8; N]` zero-fills, defeating
`#Uninit`). **That blocker is gone:** D-FIXARR1=B is ratified (2026-06-22, card c82) AND
implemented — `[T#N]` now lowers to a real stack array `[E; N]` (`Source/Codegen/Context.rs:380`),
so `MaybeUninit<[T; N]>` is sound and skips the fill. No prerequisite remains; this is now
straight implementation.

Scalars (`Int`/`U8`/…) work with MaybeUninit too, but a single uninitialized scalar has no
perf benefit — the feature exists for buffers — so a scalars-only `#Uninit` would be a
misleading partial. The buffer case `[U8#4096]` is exactly the `[T#N]` array that now works.

**Remaining work (no gate):** (1) MaybeUninit codegen in the `Stmt::Val` arm for `b.uninit`
(emit `let mut <name>: <T> = unsafe { core::mem::MaybeUninit::uninit().assume_init() };`),
(2) wire the parser dispatch (`Statements.rs:761-764` → call `uninit_binding`),
(3) diagnostics.md rows E0420–E0424 + ui snapshots, (4) golden buffer example + dataflow unit
tests. Sema is already complete and inert until the parser is wired.

## Ratified surface (option C — NOT the old `:= uninit` form)

```jet
use core.mem

fn fill(sock: Socket) {
    #Uninit buffer: [U8#4096]     // no initializer; skip the zero-fill
    sock.read(mut buffer)?        // fills it — counts as the initializing write
    process(buffer)               // ok: written before read
}
```

`#Uninit` is the marker sigil `#` (D-ATTR1) in a new position: immediately before a
local binding that has a **type annotation and no initializer**. Gated by `use core.mem`
(S58); outside that gate → teaching error pointing at the gate. Type annotation is
**required** (no value to infer from).

## Implementation design (exact)

### AST — DONE
`Binding.uninit: bool` (default false). For `#Uninit`, parser sets `uninit:true`,
`mutable:true`, `ty:Some(_)`, and `init = Expr::Unit` (a harmless placeholder so the
46 existing `b.init` walkers — purity, comptime, REPL — stay correct without edits).

### Parser (Source/Parser/Statements.rs)
In statement dispatch on `TokKind::Hash`, peek the marker ident: if it is
`Syntax::ATTR_UNINIT`, parse `#Uninit name: Type` → `Stmt::Val(Binding{ uninit:true,
mutable:true, name, ty:Some, ty_span, init: Expr::Unit(marker_span), .. })`. Errors:
no type annotation → E0421 ("`#Uninit` needs a type: `#Uninit buf: [N]U8`"); an
initializer present (`#Uninit x: T := e`) → E0422 ("`#Uninit` has no initializer").

### Gate (sema)
A `#Uninit` binding in a module without `use core.mem` → the existing core.mem gate
diagnostic (reuse the S58 path, like `Arena`). Check in `check_binding`.

### Sema dataflow — write-before-read (mirror the `moved` flow-state)
The checker already tracks `moved: HashMap<name,Span>` with branch-merge in `check_if`
(CheckerCore.rs:456, **union** = moved-in-any-branch). Definite-assignment is the
mirror: add `uninit: HashMap<String,Span>` = locals declared `#Uninit` and not yet
definitely written.
- `check_binding` with `b.uninit` → `uninit.insert(name, span)` (after gate check).
- **Write (removes from `uninit`)**: `Stmt::Assign{ target: Ident(name), op:None }`
  (check RHS first, then remove); a call arg that is a direct `Ident(name)` passed with
  `AccessConvention::Mutate` (the fill case — `mut buffer`).
- **Read (errors if still in `uninit`)**: any `Expr::Ident(name)` resolved as a read
  while `name ∈ uninit` → **E0420** ("read of possibly-uninitialized `name`"; note the
  `#Uninit` decl span; fix: write it first). Hook at the ident-resolution read site
  (CheckerCore — same place use-after-`moved` is reported). The mut-pass and assign-LHS
  positions must NOT count as reads (handle them before/instead of the generic read).
- **`if` merge (intersection of "initialized")**: in `check_if`, alongside the existing
  `moved` union, thread `uninit`: `before = uninit.clone()`; each branch starts from
  `before`; a name is **still uninit after** if it is still uninit in **any** branch
  (union of branch-remaining); with **no `else`**, the fall-through keeps `before`, so
  body inits don't count (result ⊇ before). (Initialized-after = initialized in every
  path.)
- **Loops (`while`/`for`)**: body may run 0 times → restore `uninit = before` after the
  loop (inits inside the body don't escape); reads inside the body are checked against
  the live set (sound: first-iteration read of a body-only init is flagged).
- `op Some` compound-assign (`buffer += …`) reads first → E0420 if uninit. Index/field
  writes are partial → do NOT clear `uninit` (sound conservative; the real fill is the
  `mut`-pass).

### Codegen (Source/Codegen/Statement.rs `Stmt::Val` arm)
`b.uninit` → restrict to no-Drop ("POD": primitives, fixed arrays of POD, structs of
POD); a Drop type → **E0423** at sema ("`#Uninit` needs a plain-data type; `T` has
cleanup"). Lower to:
`let mut <name>: <T> = unsafe { core::mem::MaybeUninit::uninit().assume_init() };`
The generated `unsafe` is licensed by the `use core.mem` tier (I1). Sema's
write-before-read proof makes the read sound; this skips the zero-fill (the perf goal).
All later uses are plain `T` — no `assume_init` juggling per use.

### Diagnostics (docs/spec/diagnostics.md) + tests
E0420 (read before write), E0421 (missing type), E0422 (initializer present), E0423
(Drop type). Each: `tests/ui` snapshot. Example: `examples/features/NN_uninit.jet`
(green path: declare, fill via `mut`, read) with golden output, golden-tested.
Unit tests for the dataflow (if-both-branches ok; if-one-branch E0420; loop-body init
doesn't escape; compound-assign E0420).

## Invariants
I1 (generated `unsafe` only under the `use core.mem` gate), I2 (rustc never speaks —
sema owns E0420), I3 (codegen dumb — just emits the MaybeUninit line), I4 (every E-code
has a snapshot), I5 (example is golden-tested).
