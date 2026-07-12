# Product split — slice 4: ModuleEval plan model

## Goal

Card #367 slice 4. Extract the computed-module evaluator (`ModuleEval`) and its
typed plan outputs into a shared, pure "plan model" crate that sits *below* both
realization engines, so `jetpack`'s env-runtime and the JetOS realization no
longer share the plan model by living in the same crate. This is the layering
D-PRODUCT-SPLIT1=C wants beneath the binaries: one canonical plan model, two
independent realizers.

Not a mechanical file move — a prior builder correctly refused that. This plan is
built from the real dependency structure below.

## Dependency reality

`ModuleEval` (7 files, 3321 lines) is already pure. It parses an
`env.jet`/`config.jet` module surface, runs it through the M9.5 comptime
interpreter, feeds §6 merge, and emits typed plans. Non-test couplings:

- Compiler frontend, via jetpack's `jet_codegen` re-export: `crate::AST`,
  `crate::Comptime`, `crate::Parser`, `crate::Lexer`, `crate::Diagnostics`,
  `crate::Sema`, `crate::Syntax`.
- `super::Merge` (§6 structural merge) — pure, `use std::collections::BTreeMap`
  only, zero `super::`/`crate::` couplings.
- `super::RefSpec`, `super::PackageManifest` — **already in `jet-pkg-model`**
  (slice 1).
- `super::Recipe::BuildRecipe` — only the **struct** (the `Build(BuildRecipe)`
  plan variant). The engine functions in `Recipe.rs` (`validate`, fetch via
  `Command::new`, `std::fs::write` trust files) are *not* touched by ModuleEval.
- `check_build_io` — lives inside `ModuleEval/Eval.rs`; pure static analysis over
  `Expr`, no engine.

`ModuleEval` touches **no** `Provider`, `ProviderGraph`, `Store` realize,
`WorkspaceLock`, `Trust`, `Secrets`, or `Toolchain`. It is plan model, not
engine.

Plan types (`EnvPlan`, `SystemPlan`, `ImagePlan`, `FleetPlan`, `HostPlan`,
`VmTestPlan`, `ServicePlan`, `DevServicePlan`, `AdapterPlan`, `EvaluatedModule`)
are consumed by:

- **env-runtime** (realization, engine): `CLI/run_enter_dev.rs` →
  `ModuleEval::evaluate_env` → `EnvPlan` → `Shell::enter`/`run_command` (spawns
  the shell), `Overlay`, `EnvFile`, `Services`.
- **JetOS realization** (engine): all 29 `JetOS/*.rs` submodules consume
  `System/Image/Fleet/Host/VmTest` plans to realize the store, generations,
  activation, and VMs.
- **Canvas** (slice 5 overlap): `Source/Canvas/project_scan.rs`,
  `project_transactions.rs`.
- Tests: `tests/fleet.rs`, `tests/image.rs`, plus in-crate `#[cfg(test)]`.

Both realizers depend *down* on the same plan types. That shared dependency is
exactly what a lower crate expresses cleanly.

## Boundary

Three layers, acyclic (`jet-comptime`/`jet-sema`/`jet-parser` do not depend on
`jet-pkg-model`, confirmed — no cycle):

- **L1 model (pure data)** — `jet-pkg-model`. Add: `Merge` (whole, pure) and the
  `BuildRecipe` **struct** (data only). Engine functions stay in jetpack's
  `Recipe.rs`, which now imports `BuildRecipe` from `jet-pkg-model`. Same
  data-down / engine-up pattern as slices 1 and 3.
- **L2 plan model (pure eval)** — **new crate `jet-env-model`**. Holds
  `ModuleEval/*` and the plan `Types`. Deps: `jet-pkg-model` +
  `jet-codegen` (the frontend funnel jetpack already uses) + `jet-foundation`.
  No provider/store/network/shell.
- **L3 realizers (engine, IO)** — `jetpack` env-runtime
  (`run_enter_dev`/`Shell`/`Overlay`/`EnvFile`/`Services`) and JetOS realization
  (`JetOS/*`), each depending on `jet-env-model`. No lateral env↔OS coupling.

Crate name `jet-env-model` (source is `env.jet`; sibling to `jet-pkg-model`).
Internal crate name — not user-facing, not a ballot. Alt: `jet-plan-model`.

Teaching/provenance shims: none newly required. Slice 4 is internal layering;
every user-visible command stays in its current binary, so D-PRODUCT-SPLIT1=C's
shim clause (`jet os …` routing message) is untouched. A shim becomes owed only
if a command surface moves between binaries — see the gate below.

## Migration order (each step keeps `cargo build --workspace` green)

**Step 1 — sink pure prerequisites into `jet-pkg-model`.** Move `Merge.rs`
whole. Split `Recipe.rs`: `BuildRecipe` struct → `jet-pkg-model`, engine fns stay
in jetpack and `use jet_pkg_model::BuildRecipe`. jetpack re-exports `Merge` and
`BuildRecipe` under historical paths (`crate::Merge`, `crate::Recipe::BuildRecipe`)
so ModuleEval and all callers are unchanged.
Tests: `cargo check -p jet-pkg-model`, `cargo test -p jetpack --lib`,
`cargo test --test pkg`.

**Step 2 — create `jet-env-model`; move ModuleEval + plan Types in.** Rewrite
`crate::{AST,Comptime,Parser,Lexer,Diagnostics,Sema,Syntax}` → `jet_codegen::…`,
`super::super::{Merge,RefSpec,PackageManifest}` → `jet_pkg_model::…`,
`BuildRecipe` → `jet_pkg_model`. Add the crate to workspace members
(+ default-members if a bare build must surface it). jetpack re-exports the plan
types and `evaluate_*`/`is_module_surface` under the legacy `crate::ModuleEval`
path so `JetOS/*`, `CLI/*`, `Services.rs` are unchanged this step.
Tests: `cargo check -p jet-env-model` (isolation), `cargo test -p jet-env-model`
(the in-crate ModuleEval suite), `cargo test --test fleet --test image`.

**Step 3 — make the shared dependency explicit; drop the shim.** Repoint jetpack
env-runtime + `JetOS/*` + `Services.rs` from `crate::ModuleEval` to
`jet_env_model::…`; remove the jetpack re-export. This is the actual split: both
realizers now name the shared plan model directly.
Tests: `cargo test --test jetpack_engine --test jetpack_jetos`,
`cargo test --test fleet --test image`.

**Step 4 — repoint Canvas + driver plan-type imports** (slice-5 seam; slice 4
does the import repoint only). `Source/Canvas/project_scan.rs` +
`project_transactions.rs` consume plan types via `jet_env_model`.
Tests: `cargo test --test canvas`.

**Step 5 — guards + docs.** Extend `tests/workspace_crates.rs`: assert
`jet-env-model` carries no provider/store/network dep and that env-runtime +
JetOS realization depend on it (not the reverse). Add the `jet-env-model` row +
the L1/L2/L3 layering note to `docs/spec/architecture.md`.
Tests: `cargo test --test workspace_crates`. Full suite: parent runs
`scripts/agent/verify-full.sh`.

## Ballot-worthy / owner-facing gates

Neither is resolved here — flagged for the parent/owner.

1. **Scope: does the JetOS realization engine physically relocate to
   `crates/jetos` in this card, or stay in `jetpack` behind the `jetos` binary
   until a later card?** Today `jetos`'s `main.rs` calls `jetpack::run(["os",…])`
   and all `JetOS/*` lives in jetpack. Slice 4 as planned extracts only the plan
   model and leaves the OS engine in jetpack. Under D-PRODUCT-SPLIT1=C ("jetos
   owns OS workflows") the engine's crate home is provenance-relevant, so
   confirm whether that relocation is in slice-4 scope or a distinct slice/card.
2. **Command surface stability.** Slice 4 moves no user-visible command between
   binaries by design. If any step is later expanded to move an env/`realize`/os
   verb across the `jet`/`jetpack`/`jetos` boundary, that triggers
   D-PRODUCT-SPLIT1=C's teaching/provenance shim and is an owner ballot — do not
   move a command silently.

(The `jet-env-model` crate name itself is internal, not user-facing — no ballot.)
