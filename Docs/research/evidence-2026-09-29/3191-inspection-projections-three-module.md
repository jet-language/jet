# #3191 — inspection and impact projections vs checked facts (three-module graph)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14`, sha256 `00b1e35e25ed941cdecb248ec8ffbfde0c1d5b48b563177443436288af2ad3fd`.

## Fixture
`~/.cache/jet-test-scratch/Closer07/inspect3/` (`package.jet` allows IO, Log, Mem.Alloc). `safe-jet.sh run run.jet` prints `10` / `"app"`.

| file | contents | sha256 |
|---|---|---|
| `run.jet` | `use "service"`, `use core.encoding.json`, `run()` calls `service.quote` | `5e3a9871…` |
| `service.jet` | `use "model"`, `pub fn quote(prices) = model.total_of(prices) + 1` | `eef21327…` |
| `model.jet` | `pub struct Item`, `pub fn make`, `pub fn total_of` (calls `make`) | `16525aa0…` |

Edit variants (copies):
- `inspect3_body/` — representation-only body change in `total_of`: `item :: make(price); sum += item.price` → `sum += make(price).price`. model sha256 `047f19ce…`.
- `inspect3_sig/` — public signature change: `total_of(prices: [Int], bonus: Int)`. model sha256 `f9bcb05c…`. This breaks the `service.jet` caller.

## Method
`planes.sh <outdir>` runs, for each variant:
- `safe-jet.sh inspect {types,rights,claims,structure,build} run.jet --json`
- `inspect impact run.jet {total_of,make} --json`
- `inspect compiler check model.jet`
- `inspect graph run.jet --json`
- `inspect query build run.jet --json`
- the live/replay forms

It records input and binary hashes. Outputs: `inspect3/out/base2/`, `inspect3_body/out/`, `inspect3_sig/out/`.

LSP: `python3 ~/.cache/jet-test-scratch/Closer07/lsp/drive_inspect3.py ~/.cache/jet-test-scratch/Closer07/inspect3` sends references, prepareCallHierarchy, prepareTypeHierarchy, hover and definition over `safe-jet.sh self lsp`. Raw output: `~/.cache/jet-test-scratch/Closer07/result_inspect3.json`.

## Evidence and discrepancies
1. **Checker work vs semantic dependency (crit 1).**
   - Body edit: `impact total_of` is unchanged apart from a 1-byte span shift (`make` callee at 203→202). `structure` and `types` are byte-identical.
   - Signature edit: `impact total_of` returns `ok:false` with `E0104 total_of expects 2 arguments, got 1 (service.jet:4)` and no impact payload. So at the moment dependents break, the impact view yields no dependency result, only a failure.
   - `inspect compiler check model.jet` returns `ok:true` for the signature edit. It is scoped to the explicit file and states so: rows "module graph: not applicable (E2390)".
   - The two results are separately observable, but only the check side survives a breaking edit. Neither output claims runtime reachability; impact uses `upstream_callers`/`downstream_callees` with `depth_limit:3`.
2. **Identity disagreement across views (crit 2).** The same functions are named four ways:

   | view | name used |
   |---|---|
   | `inspect compiler check` | `main::make`, module `main` |
   | `inspect impact` | `definition_module: "model.jet"`, `module_path` per ref |
   | `inspect structure` | `closer07-inspect3.model` |
   | `inspect rights` | `run::run` |
   | LSP (references/definition/hover) | `fn:module:model::total_of` |

   Also:
   - The LSP derivation identity carries `source: sha256:0bd19d54…`, which is not the sha256 of `model.jet`'s bytes (`16525aa0…`).
   - `build: sig:3b17ecd15fe9afb6`, `run: ""`.
   - The CLI impact/structure outputs carry no source or build identity at all.
   - `inspect compiler check` → `semantic_index: null`.
3. **LSP vs CLI relationships (crit 3).**
   - `inspect impact run.jet total_of` finds the caller `quote` in `service.jet` (67..75).
   - LSP `textDocument/references` on `total_of` (includeDeclaration) returns only the declaration in `model.jet` 8:7, missing the `service.jet` use.
   - `prepareCallHierarchy` returns `[]` and `prepareTypeHierarchy` on `struct Item` returns `null`. These are empty, complete-looking results, not an explicit unsupported status.
   - `definition` from `service.jet` does resolve to `model.jet` 8:7.
4. **Empty complete-looking results (crit 4).**
   - `inspect build`, `inspect graph` and `inspect query build` all return `{"action":"inspect.build","ok":true,"build":null}`. They give no reason and say nothing about scope; `graph` even reuses the `inspect.build` action.
   - Live/replay handling is good: explicit typed errors. `--live 999999` → E2105 "No live Jet runtime is observable at process 999999"; `--replay nonexistent.artifact` → E2104 "Invalid replay name". A target combined with `--live`/`--replay` → E2104 "one scope".
   - `rights` labels itself well: `scope: explicit-file`, `evidence: checked_static`, `runtime_observation: not_run`, `provenance_scope: checked_semantic_index`.
   - `claims` → `ok:false`, `capability_relation.status: "unavailable"` with the reason ("hardening-manifest.json is absent").
5. **Origins (crit 5).**
   - `inspect types` lists `Vec3` operator rows ("Float * Vec3 -> Vec3 mirror of Vec3.Mul<Float>", …) for a program that has no `Vec3`. These are synthetic Core rows from `register_synthetic_operators` (`crates/jet-foundation/src/Traits.rs:2687-2730`, span 0..0), shown with no origin marker and labelled "explicit hook" like source rows.
   - The user's own `Item` has no row.
   - `structure` includes a Core liveness fact (`Core/math/math.jet` 4822..4823 "parameter is never read") among the user's facts. It keeps a source path but has no imported/Core origin field.
6. **Double document (crit 6).** On the signature-edit variant, `inspect structure --json` wrote two JSON documents to stdout: first `ok:true` with structure facts, then `ok:false` with the E0104 reports.
7. **Build graph (crit 7).** `inspect graph` and `query build` return `build:null` for this package, so nothing can be compared. The eleven-item Main card was not reviewed here.

## Verdict
FAIL. The projections do not agree on identity, LSP references and call hierarchy disagree with CLI impact, and there are empty complete-looking results (build/graph/query, callHierarchy, typeHierarchy) plus synthetic rows without origin. Criterion 8's named tests (`three_module_projections_match_checked_facts`, `impact_and_hierarchy_match_checked_graph`) do not exist. No repair was attempted: this closer's contract forbids compiler changes.
