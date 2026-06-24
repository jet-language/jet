# c119 — Split largest compiler modules by responsibility
**Decision:** none required — pure refactor, no behavior change, no new user-facing surface.
**Gate:** none.

---

## Why

The largest modules are maintenance bottlenecks. Any new sema pass or codegen feature has to
navigate files that mix unrelated responsibilities. The split is by stable responsibility
boundaries so future features touch fewer files and conflicts narrow.

---

## Measured sizes (verified 2026-06-24 via `wc -l Source/**/*.rs | sort -n | tail`)

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

Current responsibilities mixed in TIR.rs (verified names):
- TIR node definitions: `TFunc` (`:53`), `TStmt` (`:164`), `TExpr` (`:427`), `TExprKind`
  (`:432`) — these exist as stated.
- The subset-coverage predicate is the module-level **`pub(crate) fn n(body, cx) -> bool`**
  (called from `Codegen/mod.rs`). **There is no `tir_covers` / `tir_covers_test_body` /
  `tir_covers_error_conv`** — the writer invented those names; the predicate is `n` (plus its
  helpers).
- Expression lowering: `lower_expr` (`:6301`), all `TExprKind` arms.
- Statement lowering: `lower_stmts` (`:5034`), `lower_stmt` (`:5038`), `lower_forin_collection`
  (`:5496`), `lower_enum_match` (`:5865`)/`lower_enum_arg` (`:6070`). (No `lower_for`/
  `lower_match` by those names.)
- Item lowering and emit (16 `emit_*` fns) — currently interleaved with lowering.

Proposed split (`Source/Codegen/TIR/` subdirectory, new `mod.rs` re-exports):

| New file | Responsibility |
|----------|---------------|
| `TIR/Nodes.rs` | TIR node type definitions (`TFunc`, `TStmt`, `TExpr`, `TExprKind`, etc.) |
| `TIR/Coverage.rs` | the `n` coverage predicate and its helpers |
| `TIR/LowerExpr.rs` | Expression lowering (`lower_expr`, all `TExprKind` arms) |
| `TIR/LowerStmt.rs` | Statement lowering (`lower_stmts`, `lower_stmt`, `lower_forin_collection`, `lower_enum_match`) |
| `TIR/LowerItems.rs` | Item lowering (fn/struct/enum/trait-impl lowering) |
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
| `Sema/Infer/BinaryOps.rs` | `infer_binary` (`:2554`) + numeric coercion helpers |

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

**Correction:** Registration.rs does **not** contain `compile_bundle`/`check_bundle` — that
pipeline driver lives in `Source/Sema/Bundle.rs` (the `check`/`check_freestanding` entry over
`ProgramBundle`/`CompileMode`). Registration.rs's actual contents are the top-level entry
`check`/`check_with_mode` (`:82`/`:86`) plus a large family of `register_*` and `check_*`
helpers (`register_distinct`, `register_const`, `register_struct`, `register_enum`,
`register_type_methods`, `register_impl_methods`, `check_func_body`, `check_effect_boundaries`,
`eval_comptime_items`, …). The natural split is registration vs in-place body/effect checks,
not registration vs pipeline:

| New file | Responsibility |
|----------|---------------|
| `Sema/Register.rs` | `register_*` (struct/enum/const/distinct/methods into the registry) |
| `Sema/CheckBodies.rs` | `check`/`check_with_mode` entry, `check_func_body`, `check_effect_boundaries`, `eval_comptime_items` |

`Registration.rs` → `Register.rs` + `CheckBodies.rs`; `Sema/mod.rs` re-exports both. (The
pipeline already lives separately in `Bundle.rs`, so no `Pipeline.rs` is needed.)

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
| `Source/Sema/Registration.rs` | `Source/Sema/Register.rs` + `Source/Sema/CheckBodies.rs` |

---

## Decision verdict

No decision needed — pure refactor, no user-facing syntax or behavior change.
