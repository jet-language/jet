# Core backend facts

Published ownership, effect, failure, blocking, platform, and backend facts for
compiler-known Core modules (R10 / #1134). Accelerated backends must
differentially conform to these facts against the CPU/oracle path. This page
does not claim that current compiler-owned rows use ordinary-Jet behavior
source. `core.archive` uses an explicit Rust ABI bridge: `archive.jet` declares
the package boundary, while `src/lib.rs` implements behavior. This does not
satisfy D-CORE-SOURCE-AUTHORITY1=A. Card #1133 remains open.

| Module | Ownership | Effects | Failure | Blocking | Platform | Backend |
|--------|-----------|---------|---------|----------|----------|---------|
| Kernel (`JetStd` brace chain) | owned values | pure unless noted | typed `Result` / panic | sync | all native | audited intrinsic/ABI kernel |
| `core.files` / `core.path` | path/handle owned | `IO` | `IOError` | may block | host FS | reachable Core runtime + audited host ABI |
| `core.net` / `core.tls` | stream owned | `Net` | `NetError` | may block | Linux proven; macOS/Windows E9 | reachable Core runtime + audited host ABI |
| `core.http.*` | request/response owned | `Net` | `HTTPError` | may block | native | reachable Core runtime + audited network ABI |
| `core.data` | table/series owned | pure / bridge | `DataError` | sync | all | reachable Core runtime + audited data ABI |
| `core.compute` | `Tensor` owned | pure (CPU oracle) | `ComputeError` | sync | all | reachable Core runtime + CPU ABI; GPU E6 |
| `core.services` | tree/endpoint owned | tasks/channels | `ServiceError` | sync mailboxes | all | reachable Core runtime over taskgroup ABI |
| `core.archive` | bytes owned | pure | string/`Result` | sync | all | package boundary + explicit audited Rust ABI bridge |

## Differential conformance

1. Sema records the reachable Core closure and its source/ABI classification.
2. AOT, JIT, and deopt preserve the same canonical Core meaning and ABI
   behavior where a bridge applies; no engine adds a second policy or failure
   meaning.
3. Native cache identity includes the SHA-256 R10 source/closure descriptor
   (`jet-corelib-r10`) and length-delimited toolchain, dependency, target, mode,
   and instance facts, so a changed source package or ABI kernel cannot reuse a
   stale artifact.

## Offline delivery

Pinned toolchains and content-addressed Core builds remain the delivery path.
`core.archive` proves one package boundary and one audited ABI source tree
consumed by CoreProvider. It does not prove ordinary-Jet behavior authority.
No copied compiler-template fallback is allowed.

Hostile closure checks:

- Missing or mismatched R10 source/closure fingerprint → cache miss and
  rebuild; no silent reuse of a narrower reachable closure.
- Missing or mismatched `bin.sha256` → cache miss and rebuild; a truncated or
  modified cached binary is never treated as a valid artifact.
- Failure while publishing a new binary or digest → explicit build error; a
  partial cache write is never reported as a successful store.
- `core.archive` CoreProvider, AOT bridge, and JIT host must consume the same
  audited Rust ABI source; no second algorithm or compiler template is allowed.
- Offline builds use the pinned Jet toolchain identity recorded in the store;
  host drift that changes that identity invalidates the cache. An unreadable
  project manifest also disables cache reuse instead of becoming an empty
  identity.

AOT and default `jet run` (Cranelift / deopt) preserve the same reachable Core
meaning for every applicable module in the table above (I9).
