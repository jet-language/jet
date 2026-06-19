# Module-split refactor — narrow the giant files

**Status:** plan, awaiting owner sign-off. No code moved yet.
**Goal:** break the few oversized source files into navigable submodule
directories of ~200–600 lines each, grouped by concern, with **zero feature
change**. Produced by a 15-agent read-only audit; adversarially reviewed.

## The one rule everything hangs on

This is a **pure module-move**. Every relocated item is a verbatim cut/paste of
its body. The only edits allowed are (a) a visibility keyword on a moved item
(`private → pub(crate)/pub(super)`), (b) added per-file `use` lines, and (c)
`pub use` re-exports + `mod` declarations to keep public paths resolving.

**The gate:** `nix develop -c cargo test` passes after every phase with **zero
`UPDATE_EXPECT`**. A pure move changes not one byte of diagnostic text,
generated Rust, or fmt output. If any `tests/ui` or golden snapshot diffs, the
move was impure — **revert that phase, never rebless.** Baseline is green today
(cargo test exit 0); that is the "before" we must reproduce exactly.

## Why this is safe (and where the risk actually is)

The audit found the whole codebase has `pub(crate)=0`, `pub(super)=1`. Because
Rust siblings can't see each other's privates, **every existing cross-file
reference already goes through `pub`** — so moving code *between* today's files
is trivial. The entire risk is the opposite: splitting one big file turns its
*intra-file* privates into *inter-file* references that won't compile until
bumped to `pub(crate)`/`pub(super)`. This is a **privacy-bump refactor**, and
the compiler catches every missed bump as `E0603/E0616/E0624` at build time —
loud, never silent, never a behavior change.

Confirmed non-hazards: no `macro_rules!` anywhere in `src/`; no
`file!()/line!()/column!()/module_path!()` (so no move can shift a generated
string); all wildcard `use X::*` are either inside `#[cfg(test)]` mods or
function-local (they travel with their owner). Public API surface to preserve:
**135 `jet::<mod>::<item>` paths** referenced from `tests/`, `main.rs`, `bin/`.

## Scope

**In:** the oversized files below. **Out (this pass):** the shared hubs
`ast.rs` (17 importers), `diag.rs` (25 + lib.rs re-export aliases), `syntax.rs`
(26 + the I7 keyword registry), `exit_codes.rs` — splitting these is a separate,
higher-risk operation. Orphan files `cli_spec.rs` and `diagjson.rs` are declared
nowhere and left untouched (in-progress features). Pre-existing dead-code
warnings are baseline and are not touched.

## Target tree (after all phases)

**Grouping (owner-ratified): flat top-level dirs.** No `frontend/`/`pkg/`
grouping — every split file becomes its own top-level dir at the same path it
had. This means **`lib.rs` needs zero changes**: `pub mod lexer;` etc. already
resolve a dir-with-`mod.rs` exactly as they resolved the file. `jet::parser::`,
`jet::lexer::`, `jet::fmt::`, `jet::publish::` all preserve for free, which
deletes the entire `mod frontend;`/`pub use frontend::{…}` fix class.

```
src/
├── lib.rs              # UNCHANGED — every split dir keeps its module's path
├── main.rs             # [[bin]] root: fn main + OutputMode/BuildProfile + shared helpers + `mod cmd_*`
├── cmd_compile.rs      # check/build/run/test/fmt/fix/new + rustc bridge
├── cmd_pkg.rs          # add/remove/fetch/update/store/gc
├── cmd_supply.rs       # publish/vendor/audit/sbom
├── cmd_dev_tools.rs    # dev/repl/doctor/explain/completions/bind/eval/emit/bench
│
├── lexer/{mod,tokens,terminators,scan,strings}.rs   # jet::lexer:: preserved free
├── parser/{mod,items,stmts,exprs,types,modules}.rs  # jet::parser:: preserved free
├── fmt/{mod,items,stmts,exprs}.rs                   # jet::fmt:: preserved free
├── publish/{mod,semver,api,diff,resolve,advisory,sbom,vendor,registry}.rs  # jet::publish:: free
│
├── sema/               # path preserved free (top-level dir)
│   ├── mod.rs          #   Checker decl + shared types + pub-use entry points
│   ├── ffi.rs  registration.rs  bundle.rs
│   ├── checker_core.rs  checker_infer.rs  checker_stdlib.rs
│   ├── checker_ownership.rs  checker_items.rs
│   └── diagnostics.rs  captures.rs  purity.rs
│
├── codegen/            # path preserved free
│   ├── mod.rs          #   PRELUDE + emit* drivers + pub-use
│   ├── cx.rs  tuples.rs  items.rs  stmt.rs  expr.rs  util.rs  cmodule.rs  imports.rs
│
├── lsp/                # path preserved free
│   ├── mod.rs json.rs position.rs symboldb.rs completion.rs features.rs server.rs check.rs
│
├── comptime/           # path preserved free
│   ├── mod.rs value.rs interp.rs methods.rs builtins.rs diag.rs purity.rs
│
└── jetpack/            # already a subdir; only the two big files split
    ├── modeval/{mod,types,diagnostics,eval,system,source}.rs
    └── packmanifest/{mod,helpers,parse_blocks,discovery,convert,edit}.rs
```

Two split strategies are used:
- **Submodule dir + `pub use`** (free-function files): public entry points must
  be re-exported from `mod.rs` — a `pub fn` in a *private* submodule is **not**
  reachable as `jet::mod::fn`. Used for sema's free fns, codegen, lsp, comptime,
  publish, modeval, packmanifest.
- **Impl-split across sibling files** (one big `impl Struct`): the struct decl
  stays in the parent `mod.rs`; sibling files hold method clusters. Every field
  and cross-sibling method a moved method touches bumps to `pub(super)`. Used for
  sema's `Checker`, codegen's `Cx`, `Parser`, `Fmt`, comptime's `Interp`.

## Execution order (serial — one file per phase, build+test green between each)

Lowest blast radius first; the keystone `sema.rs` last, after its consumers
(codegen, lsp) are already stable. Each phase = one commit.

| # | File → target | Why here / key gotcha |
|---|---|---|
| 1 | `publish.rs → publish/` | **Harness validation on a tiny leaf.** One real bump: `iso8601 → pub(crate)` (inline test calls it cross-file). Top-level dir → `jet::publish::` preserved free, no lib.rs change. Proves the dir-split + pub-use + zero-rebless loop before touching anything big. |
| 2 | `main.rs → cmd_*.rs` | Binary-internal, **zero external reachability** (nothing imports `jet::main`). No re-exports. Bump ~30 cross-cmd fns **plus the reverse-direction residents** `report_problems/flag_value/find_project_entry/resolve_source_path` and types `OutputMode/BuildProfile` to `pub(crate)`; cmd files `use crate::{…}`. Keep both `cfg`-gated `is_executable` defs together. |
| 3 | `codegen.rs → codegen/` | Imported by nothing but lib.rs (`emit_bundle*`) — can't break other modules. Bump `Cx` + all ~30 fields (incl. `current_fn: RefCell`, written in items, read in expr) + `Slot` to `pub(crate)`. **I3 guard:** type-classification (cloneable/comparable/box-edge) is layout computation — it stays in `codegen/cx.rs`, it is *not* sema. Land cx → tuples/util → cmodule/imports → items → expr+stmt. |
| 4 | `fmt.rs → fmt/` | Impl-split. `Fmt/Prec/Comment` + writer primitives stay in mod.rs. ~8 `pub(super)` bumps (`fmt_item/fmt_import/fmt_block_stmts/fmt_cond/fmt_value_block/fmt_expr/fmt_type/fmt_pattern`). Top-level dir → `jet::fmt::`/`format_source` preserved free. |
| 5 | `comptime.rs → comptime/` | Impl-split `Interp` across interp.rs+methods.rs. **FIX: keep `CtKey` `pub`** (it's embedded in `pub enum CtValue`; narrowing → E0446; it's already pub with no external users). mod.rs must `pub use` `CtValue/DevSink/walk_calls/REPL_FUEL_BUDGET` **and** `evaluate/evaluate_owned/run_main_with_fuel/run_repl_step` (external callers in sema/modeval/repl). |
| 6 | `lexer.rs → lexer/` | Submodule-dir; `Lexer`+fields+`keyword` stay in mod.rs (children see parent privates free). ~4 `pub(super)` sibling bumps (`at/pos`, `string/triple_string`). Top-level dir → `jet::lexer::` preserved free. |
| 7 | `parser.rs → parser/` | **Seam map's "zero bumps" was wrong** — sibling impl-split files are distinct child modules, so cross-sibling `self.method()` needs `pub(super)` (`self.expr` 41×, `type_` 22×, `block_stmts` 20×). Keep `Parser`+cursor primitives in mod.rs; bump the grammar entry points. Top-level dir → `jet::parser::{parse,parse_for_fmt,parse_for_check}` preserved free. |
| 8 | `jetpack/modeval.rs → modeval/` | Free-functions, already in jetpack (no lib.rs change). Bump 16 diag constructors + 5 driver fns to `pub(super)`. Name the submodule `system.rs` (sibling `jetos.rs` exists). mod.rs must retain the type `use`s the inline test's `use super::*` resolves against. |
| 9 | `jetpack/packmanifest.rs → packmanifest/` | Free-function grouping. Keep all type defs + `impl PackManifest` + `parse` in mod.rs; `pub use` discovery/convert/edit entry points; bump structural helpers + block parsers to `pub(super)`. |
| 10 | `lsp.rs → lsp/` | Split **before** sema so sema has one fewer consumer to coordinate; lsp only needs `sema::{CompileMode,check_bundle}`. **FIX: glob re-export** `pub use symboldb::*; completion::*; features::*;` in mod.rs (the retained inline test's `use super::*` calls `build_symbol_db/compute_*` unqualified — `pub(crate)` alone won't make them name-visible). Carry `#[allow(dead_code)]` attrs with their items. Do **not** reflow raw `r#"…"#` JSON literals (transcript tests are byte-exact). |
| 11 | `sema.rs → sema/` | **Keystone, last.** ~11.2k-line `impl Checker` across `checker_*.rs` siblings. Dominant risk = the impl-split visibility cascade: **blanket-bump `Checker` struct + all ~40 fields + all ~88 methods to `pub(crate)` in one pass** (a miss yields E0624/E0616). Shared private types (`TypeRegistry/TypeDef/MethodSig/LocalInfo/ModuleState/Send*`) move to mod.rs as `pub(crate)`; `FuncSig` stays `pub`. mod.rs `pub use` the 14 entry points (`check_bundle*`, `CompileMode`, `FuncSig`, `e3202/e3301/e3302/e3303/e3401/e3402/e3403`, `check_pure_fn`). Do not move checking toward codegen (I3). |

## Standing fixes folded in (from adversarial review, verdict GO-WITH-FIXES)

1. *(Moot under flat-dir grouping — every split dir keeps its module path, so
   `lib.rs` is untouched and no `frontend`/`pkg` re-exports are needed.)*
2. *(Moot — see 1.)*
3. `comptime`: `CtKey` stays `pub`; re-export `evaluate*`/`run_main_with_fuel`/
   `run_repl_step`.
4. `lsp`: glob re-exports for the inline test.
5. `main.rs`: bump the reverse-direction resident helpers + shared types.
6. The "exactly N `pub(super)` bumps" counts (fmt 8, lexer 4, parser ~20–40,
   sema ~88) are partition-dependent estimates — **trust the compiler over the
   numbers.** Undercounting is a loud build error, not a silent break.

## Definition of done (per phase, and overall)

- File split per its row; items moved verbatim; only visibility/`use`/re-export
  edits made.
- `nix develop -c cargo build` clean (warnings unchanged from baseline modulo
  where dead code now lives).
- `nix develop -c cargo test` green with **no `UPDATE_EXPECT`**.
- One commit per phase, message names the file and the bump count.
- `git diff` of the move shows only relocation + visibility keywords — no logic.
