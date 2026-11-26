# Layer rules design review (2026-11-26)

Adversarial design review of P22 (*Layer rules*,
`Docs/Proposals/2026-11-24-greenfield-language.md:575`) and C47
(`Docs/Audits/2026-11-24-greenfield-language-review.md:700`).

**Verdict: holds** (with P22's draft list syntax unified onto Jet's existing
lowest-first `layers: { ... }` record form).

## 1. Where `layers`, package module imports, and dependency direction checks live today

Every place in the repository that already reads `layers`, resolves package
module imports, or enforces dependency direction:

| Area | Location | Current role |
|---|---|---|
| Core manifest | `Core/package.jet:7-12` | Declares Core's 4-layer table (`prelude`, `data`, `system`, `services`), ordered lowest layer first. |
| Layer model | `Compiler/JetFoundation/Source/Rings/ModuleLayers.jet` | Defines `ModuleLayer` (`name`, `modules`) and longest-prefix lookup `module_layer_of`. |
| Package manifest model | `Jetpack/PackageModel/Source/Manifest/PackageParse.jet:247,301,628-647` | Already parses `layers: { layer: ["mod", ...] }`, rejecting empty/duplicate module entries across layers. |
| Package facts & inline carrier | `Jetpack/PackageModel/Source/Manifest/PackageFacts.jet:470-472`, `InlinePackage.jet:24` | Stores `PackageFacts.layers: [ModuleLayer]` and lists `"layers"` in `PKG_MANIFEST_BLOCKS`. |
| Core-only layer check | `Compiler/JetDriver/Source/Driver/CoreLayers.jet` | Scans Core source tokens for `core.<area>` upward references and reports `E1012` (`Jet.UpwardCoreLayerReference`). Only runs on `jet check Core`. |
| General import boundary check | `Compiler/JetDriver/Source/Driver/Boundaries.jet` | Checks `boundaries.deny` over `resolved_imports` (`SemaRegistrationImportEdge`), computing canonical module names via `jet_driver_boundary_module_name`, emitting `E0619`/`L0619` and `StructureFactKind.ImportEdge`. |
| Package identity & loader | `Compiler/JetDriver/Source/Driver/Identity.jet`, `Compiler/JetDriver/Source/Driver/Loader.jet` | Reads `package.jet` fields into `JetDriverPackageInfo` (previously ignoring `layers:` via `else -> {}`), builds `resolved_imports`, and invokes `jet_driver_check_import_boundaries` before namespace binding. |
| Import edge representation | `Compiler/JetSema/Source/Sema/Registration/` | Produces `SemaRegistrationImportEdge` records for every `use` edge across loaded modules and packages. |
| Workspace check dispatch | `Compiler/JetCLI/Source/CLI/Entry.jet:1364-1425` | `jet_cli_workspace_check_entries` makes `jet check Compiler` check the workspace's top-level roots (`JetCLI`, `JetDevServer`), whose dependency closures load every compiler member package. |
| Diagnostics & spec | `Compiler/Prelude/Diagnostics.jet`, `Docs/Spec/spec.md:7400-7422`, `Docs/Spec/Reference/jetpack-epoch5.md:76-95` | `E1012` covers Core's token scan; `E0619` covers explicit `boundaries.deny`. Neither enforced general package `layers:`. |

## 2. Adversarial attacks on P22 and C47

### Attack 1 (P22): Syntax collision with shipped `Core/package.jet`
- **Attack.** P22 drafted `layers: [ui, app, domain, storage]` (a flat list,
  top layer first). Jet already ships `layers: { prelude: [...], data: [...], system: [...], services: [...] }`
  in `Core/package.jet`, `PackageParse.jet`, `PackageFacts.jet`, `ModuleLayers.jet`,
  and `Docs/Spec/spec.md` (a record mapping each layer name to its modules,
  ordered **lowest layer first**). Supporting a second top-first list form
  would split `package.jet` into two incompatible spellings of the same
  concept and make opposite ordering assumptions on the same key name.
- **Resolution.** Unify onto the existing lowest-first `layers: { ... }` record
  form. Each layer maps a name to one or more module or package prefixes (for
  single-module layers, `storage: ["storage"]` is one line; for multi-module
  layers and workspaces, grouping multiple modules/packages into one layer is
  native). This reuses `pkg_parse_layers`, `pkg_format_source`, and
  `ModuleLayer` directly with zero grammar changes and full bootstrap safety.

### Attack 2 (P22): Conflating Core's `E1012` token scan with general import edges
- **Attack.** `CoreLayers.jet` emits `E1012` (`Jet.UpwardCoreLayerReference`)
  by scanning raw source tokens for `core.<area>` names (including qualified
  uses without a `use` statement), and its message specifically talks about
  Core modules and `Core/package.jet`. Reusing `E1012` for user packages would
  mislabel user modules as Core modules and break existing Core UI snapshots.
- **Resolution.** Keep `CoreLayers.jet` and `E1012` intact for `jet check Core`,
  and register a dedicated diagnostic `E0646` (`Jet.ImportPointsUpALayer`) for
  package import-edge layer violations naming both modules (`from`, `to`) and
  both layers (`from_layer`, `to_layer`).

### Attack 3 (C47a): Callbacks that point up a layer
- **Attack.** A lower layer (e.g. `storage`) often needs to notify or delegate
  to a higher layer (e.g. `domain` or `ui`) without statically depending on it.
- **Resolution.** Holds as C47 noted: the lower layer declares a trait (or
  function type) and accepts a value implementing it; the higher layer imports
  the lower layer and implements the trait. Every static `use` points downward.
  Proved in `Examples/Modules/Packages/LayerRules/` and
  `Compiler/JetDriver/Tests/PackageLayers.jet`.

### Attack 4 (C47b): Test exemption vs inline `#Test` loophole
- **Attack.** Exempting any file that contains a `#Test` block would let a
  production module bypass layer rules merely by appending one inline `#Test`.
  Conversely, refusing upward imports in dedicated test files would block
  integration and architecture tests.
- **Resolution.** Exempt only dedicated test files: paths under `Tests/` or
  `tests/`, files named `test.jet` / `*_test.jet` / `*_tests.jet` / `*.test.jet`
  / `*Test.jet` / `*Tests.jet`, or modules whose top-level items consist
  exclusively of `#Test` blocks (with no script body). Production modules with
  inline `#Test` blocks remain fully subject to layer rules.

### Attack 5: Multi-package workspaces (`Compiler/`)
- **Attack.** In a workspace like `Compiler/`, modules live in separate member
  packages (`jet_foundation`, `jet_lexer`, `jet_parser`, `jet_sema`, `jet_ir`,
  `jet_driver`, `jet_cli`, etc.), and `use jet_foundation.[...]` emits a
  synthetic zero-width `.Module` edge paired with an `.Unqualified` edge.
  Checking only same-package imports would miss cross-package architectural
  layering, and checking zero-width spans would produce degenerate carets.
- **Resolution.**
  1. Match each loaded module against `from_package.layers` first, then against
     the entry package's `packages[0].layers` (governs cross-package dependency
     layers without duplicating `E0646` on one edge).
  2. Match by longest listed prefix across full boundary name, package-relative
     module name, `Source.`-stripped relative name, and package name.
  3. Recover the written import span from the matching `.Unqualified` edge when
     a `.Module` edge has zero width.
  4. Audited all 24 `Compiler/*/package.jet` dependency graphs; every edge in
     the compiler obeys a clean 7-layer DAG (lowest first):
     - `foundation`: `jet_foundation`, `jet_wasm`, `jet_queries`, `jet_runtime`, `jet_prelude`, `jet_testkit`
     - `syntax`: `jet_lexer`, `jet_parser`, `jet_backend`, `jet_debug`, `jet_repl`, `jet_migrations`
     - `sema`: `jet_sema`
     - `codegen`: `jet_ir`, `jet_web`, `jet_plugin`, `jet_eval`
     - `driver`: `jet_driver`
     - `tools`: `jet_semindex`, `jet_impact`, `jet_canvas`, `jet_lsp`, `jet_devserver`
     - `cli`: `jet_cli`

### Attack 6: Stopping on the first violation
- **Attack.** Reporting only the first upward import forces a slow edit-check
  loop when refactoring a layered package.
- **Resolution.** `jet_driver_check_package_layers` iterates every edge in
  `resolved_imports` across the loaded package graph and pushes every `E0646`
  diagnostic in one pass before load stops.

### Attack 7: Duplicate layer assignment & unlisted modules
- **Attack.** Listing one module in two layers creates an ambiguous ordering;
  forcing every entry file or third-party dependency into a layer causes
  boilerplate friction.
- **Resolution.** `pkg_parse_layers` rejects duplicate module entries across
  layers at manifest parse time. Unlisted modules (such as `run.jet` or
  external packages outside the declared layers) are unconstrained.

### Attack 8: Runtime cost and tier parity
- **Attack.** Layer rules must not emit runtime checks or diverge across
  optimization tiers.
- **Resolution.** `jet_driver_check_package_layers` runs during load before
  sema/IR lowering and records only `StructureFactKind.ImportEdge` facts; zero
  runtime representation is generated. `Examples/Modules/Packages/LayerRules/run.jet`
  produces identical golden output at `-O0`, `-O1`, and `-O2`.

## 3. Exact cutover list

| Surface | Files | Change |
|---|---|---|
| Diagnostic registry | `Compiler/Prelude/Diagnostics.jet`, `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`, `Docs/Site/Dist/e/E0646/index.html`, `Docs/Site/Dist/e/Jet.ImportPointsUpALayer/index.html`, `Tests/diagnostics_coverage_baseline.txt`, `Tests/diagnostics_census.tsv` | Register `E0646` (`Jet.ImportPointsUpALayer`) and regenerate derived diagnostic tables and web pages. |
| Driver manifest & layer checker | `Compiler/JetDriver/Source/Driver/Identity.jet`, `Compiler/JetDriver/Source/Driver/Loader.jet`, `Compiler/JetDriver/Source/Driver/PackageLayers.jet` | Parse `layers:` into `JetDriverPackageInfo.layers` via `pkg_parse_layers` and enforce downward-only imports on `resolved_imports` in one pass with test exemption and `StructureFactKind.ImportEdge` emission. |
| Compiler package layers | `Compiler/package.jet`, all 24 `Compiler/*/package.jet` | Declare the verified 7-layer compiler stack across the compiler workspace and every compiler package. |
| Unit tests | `Compiler/JetDriver/Tests/PackageLayers.jet` | Verify upward refusal + exact rendered `E0646`, multi-violation one-pass reporting, downward/same-layer/trait-inversion acceptance, dedicated test file exemption, cross-package dependency layers, and duplicate-module rejection. |
| UI & case tests | `Tests/UI/import_points_up_a_layer/*`, `Tests/Cases/Layers/check_reports_every_layer_violation_in_one_pass.case` (+ fixture), `Tests/Cases/Layers/compiler_packages_declare_layers_and_check_clean.case` (+ fixture) | Lock `E0646` UI rendering, one-pass multi-violation `jet check`, and clean `jet check Compiler` with all compiler manifests declaring `layers:`. |
| Golden example | `Examples/Modules/Packages/LayerRules/*`, `Examples/Modules/Packages/LayerRules.out` | Executable layered package demonstrating downward imports and trait-based dependency inversion across `-O0`, `-O1`, `-O2`. |
| Spec, reference, grammars | `Docs/Spec/spec.md`, `Docs/Spec/Reference/jetpack-epoch5.md`, `Tools/Editors/*` (verified up to date) | Document general package `layers:`, `E0646`, one-pass checking, test exemption, and dependency inversion; verify editor grammars pass generation check cleanly. |

## 4. Proof summary

- `jet test Compiler --profile ui --filter import_points_up_a_layer`
- `jet test Compiler --profile cases --filter Tests/Cases/Layers/check_reports_every_layer_violation_in_one_pass`
- `jet test Compiler --profile golden --filter LayerRules`
- `jet test Compiler --profile cases --filter Tests/Cases/Layers/compiler_packages_declare_layers_and_check_clean`
- `jet test Compiler/JetDriver/Tests/PackageLayers.jet --filter package_layers_`
- `jet run Tools/Agent/DiagnosticRows.jet -- --check` and `jet run Tools/Agent/DiagnosticCensus.jet -- --check`
- `jet test Tests/Guards/PackageManifests/package_manifests.jet` and `jet test Tests/Guards/grammar_highlight.jet`
