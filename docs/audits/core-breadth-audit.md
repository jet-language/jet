# Core breadth audit (E3)

Audit ledger for cards #1117–#1119 and #117. Built modules only (D-STDLIBLEDGER1=C);
missing domains stay implicit.

## Compression (#1117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.archive.gzip` | shipped | compress/decompress `[U8]`; Prelude codecs |
| `core.archive.zstd` | shipped | compress/decompress `[U8]` |
| `core.archive` zip/tar | ordinary-Jet package plus audited ABI kernel | reachable `archive.jet` source closure; only its internal byte-format ABI crosses into `corelib/core.archive/pkgs/archive/src/lib.rs` |
| HTTP response compression middleware | non-goal this epoch | open in HTTP table; transport gzip decode is separate |
| Brotli / lz4 public Core modules | non-goal | not ratified; compose via FFI/`#Unsafe` if needed |

Evidence: `docs/spec/reference/core-library.md` compression sections; focused codec
coverage in `tests/corelib.rs`.

## Linalg / math (#1118)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.math` scalars | shipped | width-generic ops |
| `core.compute` helpers | shipped | shared numeric Prelude |
| `core.compute` Tensor / ndarray / FFT / sparse | shipped | CPU oracle; explicit Metal F32 covers the declared kernel subset; unsupported Metal ops fail closed |
| `core.compute` autodiff / ML / f32 tile | shipped | CPU oracle plus Metal F32 elementwise, matmul, reduction, MSE, SGD, and derivative paths; examples under `examples/features/tooling/compute_*.jet` |
| BLAS/LAPACK vendor binding | non-goal | expert `#Unsafe` / package |
| Full autograd graph beyond VJP/JVP helpers | non-goal this epoch | CPU reverse tape and composable VJP/JVP are shipped; accelerator graphs remain outside this slice |

The ML example now exercises both F64 and F32 CPU paths end to end: inference,
scalar MSE, named reverse gradients, SGD, and checksummed serialization back
to a placement-preserving Tensor. `tests/compute_parity.rs` also covers shape,
profile, checksum, learning-rate, bounds, and storage failures.
Metal placement is explicit. The targeted backend check proves that F32
placement either reaches the Metal path on an Apple target or returns the
declared no-fallback error on an unavailable target; it does not treat CPU
execution as Metal success.

## DB drivers (#117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.db` connection + typed SQL binding | shipped | checked SQL carries template text and ordered `DBValue` bindings |
| `Driver` trait + `DBConnection` impl | shipped | D-DBDRIVER1=A: `T: Driver` bounds; SQLite first backend; AOT + default `jet run` |
| ORM / query builder | non-goal | one mechanism: typed SQL carrier; builders may produce the same value |

Evidence: `docs/spec/reference/core-library.md` DB section; `tests/corelib_parts/system.rs` DB cases
(`db_checked_sql_execute_uses_typed_sql`, `core_db_implements_driver_trait`).

## Rubric / parity closeout (#1119)

Examples and goldens under `examples/features/` are the executable rubric (I5).
I9 requires AOT and default `jet run` against those goldens for every applicable
surface. Diagnostics for Core misuse stay in `docs/spec/diagnostics.md` with UI
snapshots. Backend ownership/effects facts: `docs/spec/reference/core-backend-facts.md`.
