# Core backend facts

Published ownership, effect, failure, blocking, platform, and backend facts for
compiler-known Core modules (R10 / #1134). Accelerated backends must
differentially conform to these facts against the CPU/oracle path.

| Module | Ownership | Effects | Failure | Blocking | Platform | Backend |
|--------|-----------|---------|---------|----------|----------|---------|
| Kernel (`JetStd` brace chain) | owned values | pure unless noted | typed `Result` / panic | sync | all native | Prelude template |
| `core.files` / `core.path` | path/handle owned | `IO` | `IOError` | may block | host FS | Prelude Top |
| `core.net` / `core.tls` | stream owned | `Net` | `NetError` | may block | Linux proven; macOS/Windows E9 | Prelude + OS |
| `core.http.*` | request/response owned | `Net` | `HTTPError` | may block | native | Prelude |
| `core.data` | table/series owned | pure / bridge | `DataError` | sync | all | Prelude |
| `core.compute` | `Tensor` owned | pure (CPU oracle) | `ComputeError` | sync | all | Prelude CPU; GPU E6 |
| `core.services` | tree/endpoint owned | tasks/channels | `ServiceError` | sync mailboxes | all | Prelude over taskgroups |
| `core.archive` | bytes owned | pure | string/`Result` | sync | all | `corelib/core.archive` package (no duplicated template) |

## Differential conformance

1. AOT emit calls Prelude `jet_*` symbols only.
2. JIT hosts marshal into the same symbols, or deopt to the TIR evaluator which
   calls the same symbols via Lite includes (`ComputeLite`, …).
3. Cache identity includes the R10 emission fingerprint comment
   (`jet-corelib-r10`) so a broader Top-module set cannot reuse a narrower
   artifact.

## Offline delivery

Pinned toolchains and content-addressed Core builds remain the delivery path.
`core.archive` proves the package boundary: one source tree consumed by
CoreProvider without a copied fallback template (architecture R10).

Hostile closure checks:

- Missing or mismatched R10 fingerprint → rebuild (no silent reuse of a
  narrower Top-module set).
- A package Core module must resolve through CoreProvider; a second embedded
  copy of the same source is forbidden.
- Offline builds use the pinned Jet toolchain identity recorded in the store;
  host drift that changes that identity invalidates the cache.

AOT and default `jet run` (Cranelift / deopt) share the same Prelude symbols for
every module in the table above (I9).
