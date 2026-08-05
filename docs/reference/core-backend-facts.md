# Core backend facts

Published ownership, effect, failure, blocking, platform, and backend facts for
compiler-known Core modules (R10 / #1134). Accelerated backends must
differentially conform to these facts against the CPU/oracle path. `core.archive`
is an ordinary Jet source package loaded and checked by the normal frontend.
Its source calls one audited ABI kernel for byte-format primitives; no compiler
template or engine-specific public fallback is part of the path.

| Module | Ownership | Effects | Failure | Blocking | Platform | Backend |
|--------|-----------|---------|---------|----------|----------|---------|
| Kernel (`JetStd` brace chain) | owned values | pure unless noted | typed `Result` / panic | sync | all native | audited intrinsic/ABI kernel |
| `core.files` / `core.path` | path/handle owned | `IO` | `IOError` | may block | host FS | reachable Core runtime + audited host ABI |
| `core.net` / `core.tls` | stream owned | `Net` | `NetError` | may block | Linux proven; macOS/Windows E9 | reachable Core runtime + audited host ABI |
| `core.http.*` | request/response owned | `Net` | `HTTPError` | may block | native | reachable Core runtime + audited network ABI |
| `core.data` | table/series owned | pure / bridge | `DataError` | sync | all | reachable Core runtime + audited data ABI |
| `core.compute` | `Tensor` owned | pure (CPU oracle) | `ComputeError` | sync | all | reachable Core runtime + CPU ABI; GPU E6 |
| `core.services` | tree/endpoint owned | tasks/channels | `ServiceError` | sync mailboxes | all | reachable Core runtime over taskgroup ABI |
| `core.archive` | bytes owned | pure | empty bytes / JSON `[]` on invalid input | sync | all native + interpreter | reachable ordinary-Jet package plus one dependency-free audited ABI kernel |

The archive facts are also published per operation:

| Operation | Ownership | Effects | Failure | Blocking | Platform | Backend authority |
|-----------|-----------|---------|---------|----------|----------|-------------------|
| `zip_compress` | reads `String`, `[U8]` | pure | empty bytes when input cannot be represented | sync | all native + interpreter | canonical archive ABI kernel |
| `zip_decompress` | reads `[U8]` | pure | empty bytes on malformed or checksum-invalid archive | sync | all native + interpreter | canonical archive ABI kernel |
| `tar_add` | reads archive/name/data | pure | invalid names are omitted; malformed input starts an empty archive | sync | all native + interpreter | canonical archive ABI kernel |
| `tar_get` | reads archive/name | pure | empty bytes on malformed or missing entry | sync | all native + interpreter | canonical archive ABI kernel |
| `tar_names_json` | reads archive | pure | `[]` on malformed or empty archive | sync | all native + interpreter | canonical archive ABI kernel |

## Differential conformance

1. Sema records the reachable Core closure and its source/ABI classification.
2. For the shipped `core.archive` bridge, AOT, JIT, deopt, and the resident
   interpreter include the same canonical ABI source; no engine adds a second
   policy or failure meaning. Web is not an applicable archive tier and must
   reject an archive call before emission rather than synthesize a browser
   implementation.
3. Native cache identity includes the SHA-256 R10 source/closure descriptor
   (`jet-corelib-r10`) and length-delimited toolchain, dependency, target, mode,
   and instance facts, so a changed source package or ABI kernel cannot reuse a
   stale artifact.

## Offline delivery

Pinned toolchains and content-addressed Core builds remain the delivery path.
`core.archive` ships a source closure and a dependency-free ABI kernel. The
source closure is part of the cache identity and is compiled through the
normal frontend before the package's internal ABI calls are emitted. No copied
compiler-template fallback is allowed.

Hostile closure checks:

- Missing or mismatched R10 source/closure fingerprint → cache miss and
  rebuild; no silent reuse of a narrower reachable closure.
- Missing or mismatched `bin.sha256` → cache miss and rebuild; a truncated or
  modified cached binary is never treated as a valid artifact.
- Missing or mismatched `artifacts.sha256` in the hidden Core/FFI bridge →
  remove that cache entry's link products and rebuild; artifact existence alone
  is never proof of a valid AOT/JIT bridge.
- Failure while publishing a new binary or digest → explicit build error; a
  partial cache write is never reported as a successful store.
- `core.archive` AOT, default JIT/dev, interpreter, and applicable web checks
  consume the same source-owned TIR. Only the source package's internal ABI
  calls cross into the audited Rust kernel; no second public algorithm or
  compiler template is allowed.
- Offline Cargo-backed Core builds require a regular lockfile and a realized
  pinned Jet toolchain. A missing pin, missing closure artifact, or unreadable
  source tree is a miss/error; host Cargo is not a fallback. Host drift that
  changes the pinned identity invalidates the cache. An unreadable project
  manifest also disables cache reuse instead of becoming an empty identity.

AOT and default `jet run` (Cranelift / deopt) preserve the same reachable
`core.archive` meaning through the emitted source package and its ABI kernel. The remaining
compiler-owned rows require the UL3 source-boundary migration and their
per-tier proof before this page can claim universal Core parity (I9).
