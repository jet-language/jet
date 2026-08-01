# Core breadth audit (E3)

Audit ledger for cards #1117–#1119 and #117. Built modules only (D-STDLIBLEDGER1=C);
missing domains stay implicit.

## Compression (#1117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.compress.gzip` | shipped | compress/decompress `[U8]`; Prelude codecs |
| `core.compress.zstd` | shipped | compress/decompress `[U8]` |
| `core.archive` zip/tar | shipped | package Core (`corelib/core.archive`), no duplicated template |
| HTTP response compression middleware | non-goal this epoch | open in HTTP table; transport gzip decode is separate |
| Brotli / lz4 public Core modules | non-goal | not ratified; compose via FFI/`#Unsafe` if needed |

Evidence: `docs/reference/core-library.md` compression sections; focused codec
coverage in `tests/corelib.rs`.

## Linalg / math (#1118)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.math` scalars | shipped | width-generic ops |
| `core.linalg` helpers | shipped | Prelude `LinalgFns` |
| `core.compute` Tensor / ndarray / FFT / sparse | shipped | CPU oracle; GPU deferred to E6 |
| `core.compute` autodiff / ML / f32 tile | shipped | examples under `examples/features/tooling/compute_*.jet` |
| BLAS/LAPACK vendor binding | non-goal | expert `#Unsafe` / package |
| Full autograd graph beyond VJP/JVP helpers | non-goal this epoch | CPU `GradTriple` helpers only |

## DB drivers (#117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.db` connection + parameter binding | shipped | parameter-only queries (no string concat) |
| `Driver` trait + `DBConnection` impl | shipped | D-DBDRIVER1=A: `T: Driver` bounds; SQLite first backend; AOT + default `jet run` |
| ORM / query builder | non-goal | one mechanism: typed SQL + params |

Evidence: `docs/reference/core-library.md` DB section; `tests/corelib.rs` DB cases
(`db_checked_sql_params_feed_parameterized_execute`, `core_db_implements_driver_trait`).

## Rubric / parity closeout (#1119)

Examples and goldens under `examples/features/` are the executable rubric (I5).
I9 requires AOT and default `jet run` against those goldens for every applicable
surface. Diagnostics for Core misuse stay in `docs/spec/diagnostics.md` with UI
snapshots. Backend ownership/effects facts: `docs/reference/core-backend-facts.md`.
