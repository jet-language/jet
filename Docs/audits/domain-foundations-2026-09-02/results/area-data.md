# Area data probe

## 1. What I built

I built a typed data-analysis package that reads a generated 100,000-row CSV, materializes a core table, filters and aggregates it, computes a rolling window and summary statistics, fits a typed OLS result, joins grouped summaries, labels a two-dimensional array, and writes SVG plus a JSON receipt. I also exercised the notebook protocol with dependency invalidation and isolated the missing Parquet, PNG, default-tier join, and Tensor-in-struct paths. The complete package source checks cleanly, but its single combined `jet run` did not finish within 60, 180, or 300 seconds; the component runs below are the usable evidence.

Files:
- `pkg/package.jet` — package authority.
- `pkg/analysis.jet` — complete requested package source (100k CSV path restored).
- `pkg/analysis.jetnb` — two reactive Jet cells.
- `pkg/csv_load.jet`, `pkg/data_ops_probe.jet`, `pkg/regression_probe.jet`, `pkg/manual_join_probe.jet`, `pkg/tensor_basic.jet`, `pkg/labeled_probe.jet`, `pkg/plot_receipt_probe.jet` — focused executable probes.
- `pkg/parquet_attempt.jet`, `pkg/png_attempt.jet`, `pkg/tensor_struct_probe.jet` — negative probes.
- `fixture-100k.csv` — 100,000 rows, 2,948,458 bytes.

## 2. What worked

- `scripts/agent/jet-env jet check .../pkg/analysis.jet` -> `ok`, diagnostics `0`.
- `scripts/agent/jet-env jet run .../pkg/csv_load.jet` -> `rows=100000 columns=5` (`real 0m10.062s`).
- `scripts/agent/jet-env jet run .../pkg/data_ops_probe.jet` -> `rows=100000 filtered=48450 groups=10 sums=10 pivot=20 rolling=48450 count=48450` (`real 0m12.534s`).
- `scripts/agent/jet-env jet run .../pkg/combo_probe.jet` -> `rows=100000 f=48450 g=10 p=20 w=48450 s=48450 j=10 r=3.5000030030030054` (`real 0m11.893s`).
- `scripts/agent/jet-env jet run .../pkg/regression_probe.jet` -> `slope=3.5000030030030054 intercept=-0.0002000000001117587 r2=0.9996083219166306` (`real 0m10.755s`).
- `scripts/agent/jet-env jet run .../pkg/manual_join_probe.jet` -> `joined=2 Ada Grace`.
- `scripts/agent/jet-env jet run .../pkg/tensor_basic.jet` -> `rank:2`, `shape:[2, 3]`, `device:CPU`, `cell:4.0`.
- `scripts/agent/jet-env jet run .../pkg/labeled_probe.jet` -> `south/margin=6.0`.
- `scripts/agent/jet-env jet run .../pkg/plot_receipt_probe.jet` -> `rows=3 groups=2 svg=524`; verified `output.svg` and `receipt.json` were written.
- `scripts/agent/jet-env jet run examples/features/tooling/data_analysis.jet` -> CSV/filter/sort/group/stats/text/SVG/status example exits `0`.
- `printf 'exec first data-1\nexec first data-2\nstate\n' | scripts/agent/jet-env jet notebook --protocol --headless .../pkg/analysis.jetnb` -> cell turns `1` and `2`, text `2` then `5`, cache entries `2`.
- After `edit data-1 x := 4`, `state` reported `cache_entries:0`; rerunning cells reported text `(no text/plain)` then `7`, and state returned `cache_entries:2`. Descendant invalidation works.

## 3. Gaps, ordered

### area-data-G01 — Parquet/columnar file reader

- Tags: `impossible`
- Primitive capability: read typed Parquet/Arrow columnar files into the core table/stream model.
- Evidence: `scripts/agent/jet-env jet check .../pkg/parquet_attempt.jet` exits `1`: `Error [E1004]: core.data has no item parquet`; the diagnostic fix list contains `csv`, `json`, `csv_reader`, and table/stat/plot items, but no Parquet reader.
- Areas: data analysis, backend services, AI/ML apps.
- Severity: `blocks`
- Workaround: convert input to CSV/JSON outside Jet; implementing a columnar codec is library-sized machinery.

### area-data-G02 — Raster plot export

- Tags: `impossible`, `boilerplate`
- Primitive capability: export a chart directly as PNG bytes/file.
- Evidence: `scripts/agent/jet-env jet check .../pkg/png_attempt.jet` exits `1`: `Error [E1004]: core.data has no item bar_png`; the only suggestion is `bar_svg`.
- Areas: data analysis, GUI apps, web apps, reporting.
- Severity: `blocks`
- Workaround: `data.bar_svg` works and writes SVG; a library author must provide or implement a raster encoder for PNG.

### area-data-G03 — Default-tier typed table join

- Tags: `defect`
- Primitive capability: default evaluator execution for the documented typed `core.data.inner_join` operation.
- Evidence: `scripts/agent/jet-env jet run examples/features/tooling/data_pipeline.jet` exits `1` with `Error [E0956]: core.data.inner_join() isn't supported by the current evaluator yet`; the diagnostic says the canonical TIR evaluator does not cover this construct and suggests `jet build`.
- Areas: data analysis, backend services, web apps, AI/ML apps.
- Severity: `blocks`
- Workaround: use a user-written nested-loop join (`pkg/manual_join_probe.jet` succeeds for two rows), but it is quadratic and not a general table primitive.

### area-data-G04 — Tensor handles in user records

- Tags: `defect`
- Primitive capability: store a `core.compute.Tensor` in a user-defined struct, needed for a labeled tensor/table value object.
- Evidence: `scripts/agent/jet-env jet run .../pkg/tensor_struct_probe.jet` exits `101` with `internal compiler error: whole-program TIR lowering produced no program for the dev interpreter`; `jet build .../pkg/tensor_struct_probe.jet` independently exits `101` with `compiler bug (I2/R7)` at `TensorBox{values: values}`. Plain Tensor operations and a labeled nested-list array both run.
- Areas: data analysis, numerical computing, AI/ML apps, GUI apps.
- Severity: `hurts`
- Workaround: keep labels and Tensor in separate bindings, or use nested `[[Float]]` plus user-written coordinate lookup.

## 4. Friction

- The safe numeric spelling for Float accumulators is explicit `Float{0.0}`; bare `0.0` infers Decimal and caused E0108/E0131 until corrected.
- Manual OLS and coordinate lookup are straightforward but require user-owned structs and loops; no standard model-fitting or labeled-array object was needed to make the focused examples run.
- The 100k CSV path is whole-file UTF-8 loading (`files.read` then `data.csv`), not a demonstrated Parquet/columnar or out-of-core path.
- Pandas and Polars were not installed in this environment, so no same-input incumbent timing is reported.
- The combined `analysis.jet` run timed out without a completed receipt; component commands are the reliable run evidence.

## 5. Defects

- E0956 default-evaluator rejection of the documented `inner_join` path is a cross-tier defect, not a missing data concept.
- Tensor-in-struct produces exit `101` compiler bugs in both interpreter and AOT build paths.

## 6. Battery notes

- `csv_load_100k`: parse 100k typed rows, table count, and schema count; `libraryOnly: false`.
- `table_ops_100k`: filter, group mean/sum, pivot, rolling mean, describe; `libraryOnly: false`.
- `typed_ols`: typed regression coefficients and $R^2$ result; `libraryOnly: true`.
- `manual_group_join`: grouped summary to owner join fallback; `libraryOnly: true`.
- `labeled_array`: coordinate labels resolve `south/margin` to `6.0`; `libraryOnly: true`.
- `plot_receipt`: SVG file plus JSON receipt file; `libraryOnly: false`.
- `notebook_reactive`: dependency invalidation and descendant rerun; `libraryOnly: false`.

## 7. Verdict

Jet has a credible typed in-memory analysis floor: CSV, tables, filtering, grouping, pivoting, windows, statistics, SVG, and notebook invalidation work. A library author can write typed OLS and labeled nested arrays today. Parquet and PNG are absent core surfaces, while the documented join path is unavailable in the default evaluator. Tensor handles cannot be placed in user records without compiler failure. Data analysis is therefore buildable for bounded CSV/SVG workflows, but not yet a complete columnar/raster/default-tier table-analysis platform.
