# Core breadth audit (E3)

Audit ledger for cards #1117–#1119 and #117. Built modules only (D-STDLIBLEDGER1=C);
missing domains stay implicit.

## Compression (#1117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.compress.gzip` | shipped | compress/decompress `[U8]` |
| `core.compress.zstd` | shipped | compress/decompress `[U8]` |
| `core.archive` zip/tar | shipped | package Core (`corelib/core.archive`), no duplicated template |
| HTTP response compression middleware | non-goal this epoch | open in HTTP table; transport gzip decode is separate |
| Brotli / lz4 public Core modules | non-goal | not ratified; compose via FFI/`#Unsafe` if needed |

## Linalg / math (#1118)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.math` scalars | shipped | width-generic ops |
| `core.linalg` helpers | shipped | Prelude `LinalgFns` |
| `core.compute` Tensor / ndarray / FFT / sparse | shipped | CPU oracle; GPU deferred |
| BLAS/LAPACK vendor binding | non-goal | expert `#Unsafe` / package |
| Autograd beyond VJP/JVP helpers | partial | `value_and_grad_mul` / `GradTriple` CPU only |

## DB drivers (#117)

| Surface | Status | Notes |
|---------|--------|-------|
| `core.db` connection + parameter binding | shipped | parameter-only queries (no string concat) |
| Driver conformance suite | in progress | focused tests under `tests/` for SQL string + params |
| ORM / query builder | non-goal | one mechanism: typed SQL + params |

## Rubric / parity closeout (#1119)

Examples and goldens under `examples/features/` are the executable rubric (I5).
I9 requires AOT and default `jet run` against those goldens for every applicable
surface. Diagnostics for Core misuse stay in `docs/spec/diagnostics.md` with UI
snapshots.
