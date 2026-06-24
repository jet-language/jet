# c119 — Split largest compiler modules by responsibility
**Decision:** none required — pure refactor, no behavior change, no new user-facing surface.
**Gate:** none.

---

## Why

The largest modules are maintenance bottlenecks. Any new sema pass or codegen feature has to
navigate files that mix unrelated responsibilities. The split is by stable responsibility
boundaries so future features touch fewer files and conflicts narrow.

---

## Measured sizes (as of 2026-06-22)

| File | Lines |
|------|-------|
| `Source/Codegen/TIR.rs` | 12,124 |
| `Source/Sema/CheckerInfer.rs` | 3,639 |
| `Source/Sema/CheckerCore.rs` | 2,420 |
| `Source/Sema/CheckerCoreLib.rs` | 2,079 |
| `Source/Parser/Items.rs` | 2,044 |
| `Source/Parser/Statements.rs` | 1,806 |
| `Source/Parser/Expressions.rs` | 1,713 |
| `Source/Sema/Registration.rs` | 1,682 |
| `Source/AST.rs` | 1,575 |
| `Source/REPL.rs` | 1,530 |
| `Source/Sema/Bundle.rs` | 1,495 |
| `Source/Sema/CheckerItems.rs` | 1,439 |
| `Source/Loader.rs` | 1,127 |

Priority order: TIR.rs first (12K lines, 10× the second); then CheckerInfer.rs; then Parser
cluster; then Registration.rs.

---

## Split plan

### 1. `Source/Codegen/TIR.rs` → 6 modules

Current responsibilities mixed in TIR.rs:
- TIR node definitions (structs/enums: `TFunc`, `TStmt`, `TExpr`, etc.)
- `tir_covers` / `tir_covers_test_body` — subset-coverage predicates
- Expression lowering (`lower_expr`, `lower_call`, all TExprKind arms)
- Statement lowering (`lower_stmt`, `lower_for`, match/when lowering)
- Function/item lowering (`lower_fn`, `lower_struct`, `lower_enum`, `lower_trait_impl`)
- Emit (Rust source string construction) — currently interleaved with lowering

Proposed split (`Source/Codegen/` subdirectory, new `mod.rs` re-exports):

| New file | Responsibility |
|----------|---------------|
| `TIR/Nodes.rs` | All TIR node type definitions (TFunc, TStmt, TExpr, TExprKind, etc.) |
| `TIR/Coverage.rs` | `tir_covers`, `tir_covers_test_body`, `tir_covers_error_conv` |
| `TIR/LowerExpr.rs` | Expression lowering (`lower_expr`, all TExprKind arms) |
| `TIR/LowerStmt.rs` | Statement lowering (`lower_stmt`, `lower_for`, `lower_match`) |
| `TIR/LowerItems.rs` | Item lowering (`lower_fn`, `lower_struct`, `lower_enum`, `lower_trait_impl`) |
| `TIR/Emit.rs` | Rust string emission (all `emit_*` fns) |

The existing `Source/Codegen/TIR.rs` becomes `Source/Codegen/TIR/mod.rs` re-exporting all
pub items. `Source/Codegen/mod.rs`'s `use crate::Codegen::TIR::*` import is unchanged —
callers see no difference.

**Safety:** zero behavior change. Move code exactly; no renaming of pub items. Run
`nix develop -c cargo test` after each file move to confirm green.

### 2. `Source/Sema/CheckerInfer.rs` → 3 modules

Current responsibilities:
- Type inference core (`infer`, `infer_inner`, all Expr arms)
- Method-call resolution (`infer_method_call`, `finish_builtin_method`)
- Numeric / binary op inference (`infer_binary`, `infer_numeric_op`)
- Collection inference (`infer_list_lit`, `infer_map_lit`, `infer_index`, `infer_slice`)
- Fallibility / `?` / `??` handling (`infer_try`, `infer_or_fallback`, `infer_fallible_stmt`)

Proposed split:

| New file | Responsibility |
|----------|---------------|
| `Sema/Infer/Core.rs` | `infer`, `infer_inner`, `infer_name_or`, scalar/bool/string arms |
| `Sema/Infer/Methods.rs` | `infer_method_call`, `finish_builtin_method`, `field_type` |
| `Sema/Infer/Fallible.rs` | `infer_try`, `infer_or_fallback`, `infer_fallible_stmt`, `??` |
| `Sema/Infer/Collections.rs` | `infer_list_lit`, `infer_map_lit`, `infer_index`, `infer_slice`, `infer_tuple_lit`, `infer_fan_out` |
| `Sema/Infer/BinaryOps.rs` | `infer_binary`, `infer_binary_inner`, numeric coercion |

`Source/Sema/CheckerInfer.rs` becomes `Source/Sema/Infer/mod.rs`.

### 3. `Source/Parser/` cluster — Items.rs, Statements.rs, Expressions.rs

These are already modular enough; each is one responsibility. The only split worth doing:

`Source/Parser/Items.rs` (2,044 lines): split struct/enum/trait/impl parsing from
fn/module/test parsing.

| New file | Responsibility |
|----------|---------------|
| `Parser/ItemsTypes.rs` | struct, enum, trait, impl blocks |
| `Parser/ItemsFns.rs` | fn, test_def, module declarations |

`Items.rs` becomes `ItemsTypes.rs` + `ItemsFns.rs`; `Parser/mod.rs` re-exports both.

### 4. `Source/Sema/Registration.rs` → 2 modules

Current: registers items into the sema registry, AND drives the compilation pipeline
(`compile_bundle`, `check_bundle`).

| New file | Responsibility |
|----------|---------------|
| `Sema/Register.rs` | Item registration (struct/enum/trait/fn/test into the registry) |
| `Sema/Pipeline.rs` | `compile_bundle`, `check_bundle`, `CompileMode`, ordering |

`Registration.rs` → `Register.rs` + `Pipeline.rs`; `Sema/mod.rs` re-exports both.

### 5. `Source/AST.rs` — leave as-is for now

AST.rs (1,575 lines) is all type definitions — coherent, flat. It will need splitting as
node count grows, but the current size is manageable and it has no logic to separate. Defer.

---

## Execution order

1. TIR.rs split (highest leverage; move in 6 PRs, one per file, test after each).
2. CheckerInfer.rs split.
3. Parser/Items.rs split.
4. Registration.rs split.

Each step:
- Create new file(s).
- Move code exactly (no renames, no logic changes).
- Update `mod.rs` / re-exports.
- `nix develop -c cargo build && nix develop -c cargo test` — must be green.
- Commit with message `refactor: split <OldFile> into <NewFiles> (c119)`.

---

## Files touched

All moves are within `Source/`. No `docs/`, `tests/`, or `examples/` changes except that
tests must stay green throughout.

| Before | After |
|--------|-------|
| `Source/Codegen/TIR.rs` | `Source/Codegen/TIR/{mod,Nodes,Coverage,LowerExpr,LowerStmt,LowerItems,Emit}.rs` |
| `Source/Sema/CheckerInfer.rs` | `Source/Sema/Infer/{mod,Core,Methods,Fallible,Collections,BinaryOps}.rs` |
| `Source/Parser/Items.rs` | `Source/Parser/ItemsTypes.rs` + `Source/Parser/ItemsFns.rs` |
| `Source/Sema/Registration.rs` | `Source/Sema/Register.rs` + `Source/Sema/Pipeline.rs` |

---

## Decision verdict

No decision needed — pure refactor, no user-facing syntax or behavior change.
