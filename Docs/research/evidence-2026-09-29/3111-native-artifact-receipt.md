# #3111 — Native artifact and allocator compatibility receipt

Closer09, 2026-09-29. Source head `e9c708fa7` (working tree, shared checkout).

## Question

Does a native guest artifact carry a finite compatibility receipt (runtime,
toolchain, target, ABI contract, required system assets, C-safe ownership and
freeing rules), and is a wrong ABI, version, target or artifact rejected at
load?

## Method

1. Source inventory of the two native-artifact channels in Jetpack
   PackageModel (read, not executed):
   - Jet-built native guests (`.jetlib`):
     `Jetpack/PackageModel/Source/Artifacts/JetLibStamp.jet`,
     `Artifacts/JetLibTrust.jet`, tests `Tests/JetLibStamp.jet`.
   - Foreign (C/C++/Rust…) bridge artifacts:
     `Source/Foreign/Bridge/BridgeProvenance.jet`,
     `BoundaryContract.jet`, `BinderDescriptor.jet`.
2. Execution attempt of the card's proof command:
   `JETPACK_WORKER=closer09p3111 Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh --area PackageModel --tests`.

## Evidence

### Execution

The proof command assembled 244 sources / 379 test claims and then failed:
receipt `~/.cache/jet-dev/jetpack-bootstrap/closer09p3111-f0a9e45f67/check.receipt`
records `check-exit: 124` (wall-clock timeout) with `check-peak-bytes:
6071971840`, `jet-binary: target/debug/jet` (sha256 `1c6f33ed…`), `exit: 1`.
No `JETPACK CHECK OK` and no test was executed. Under tonight's machine load
the check phase cannot finish inside the harness timeout, so none of the
existing claims below is execution-proven here.

### Receipt inventory

| receipt item | `.jetlib` (`JetLibStamp`) | foreign bridge boundary row |
|---|---|---|
| runtime / compiler | `compiler_version`, `compiler_build` | `boundary-toolchain-identity` (toolchain/runtime/cc/compiler field, else `not-recorded`) |
| target | `target`, `target_triple`, `linker_identity` | `boundary-target-identity`, `boundary-coverage-target` |
| ABI contract | `abi_identity`, `abi_version` | ABI stamp only inside the boundary digest (`stable_fields` `abi`); no `abi-contract=<version>` provenance row |
| artifact digest | `payload_digest` (shape-validated), payload length check | `artifact.<name>` sha256 rows, `boundary-loaded-artifact` |
| required system assets | none | `boundary-transitive-dependencies` (optional, unknown stays unknown); no `system-asset=<soname@version>` rows |
| C-safe shapes | `JetLibScalar` export table + `JetLibAccess` conventions | `boundary-width-*`, `boundary-alignment`, `boundary-nullability`, `boundary-encoding` |
| ownership / freeing | none beyond access conventions | `boundary-ownership` (`SignatureDeclared`, `opaque-owned-handle`), `boundary-cleanup` prose; no per-handle `free-rule=<handle>:<release fn>` |

### Load-time rejection

`.jetlib` (`pkg_jetlib_validate_load_metadata`, `pkg_jetlib_check_compiler_identity`):
- ABI version ≠ `@JETLIB_ABI_VERSION` → `E1341 … unsupported .jetlib ABI version`.
- ABI identity ≠ `@JETLIB_ABI_IDENTITY` → `E1341 … unsupported native ABI identity`
  (claimed by test "jetlib load metadata rejects a wrong abi identity").
- target ≠ loader target → `E1341 … targets X, but this loader targets Y`.
- empty compiler build / target triple / linker identity → `E1341 … has no valid …`.
- invalid payload digest shape → `E1341`; truncated payload → decode error.
- compiler version mismatch → `E1338` before mapping (test "jetlib mismatched
  compiler identity is refused before mapping").

Foreign bridge (`pkg_bridge_read_boundary_provenance`): rejects only a missing
`boundary-schema` row and a missing `boundary-digest`. It does not compare the
ABI contract version, the target or the artifact digest with the loader.

## Verdict

- Criterion 1: partial. Runtime/toolchain/target/ABI are named for `.jetlib`
  and (except the ABI version row) for foreign boundaries; required system
  assets are not named on either channel.
- Criterion 2: partial. C-safe shapes are recorded on both channels; freeing
  rules exist only as prose `boundary-cleanup` and the ownership model name,
  not as observable per-handle release functions.
- Criterion 3: partial. `.jetlib` rejects wrong ABI version/identity, target,
  compiler identity and missing identity fields (source; tests exist but did
  not execute tonight). Foreign bridge boundaries reject none of ABI version,
  target or digest mismatch. No new native dependency or bundled-versus-system
  choice surfaced, so nothing needs a ballot.
- Criterion 4: not met / BLOCKED. The named check timed out (exit 124, 6.07 GB
  peak) before running any test, and the required new `#Test` claims in
  `Tests/ForeignBindingPlan.jet` do not exist (implementation work in the
  card's Changes 1–3, not evidence work).

## Follow-up (implementation, batch-jetpack-pkgmodel)

Card Changes 1–3 stand as written: add `abi-contract`, `calling-convention`,
`system-asset`, `free-rule` rows to the boundary row; make
`pkg_bridge_read_boundary_provenance` reject ABI-version, target and digest
mismatches and report a missing field as unsupported; emit free rules from
`ForeignAbiContract.ownership`. The `.jetlib` gate in `JetLibTrust.jet` is the
existing pattern to reuse.
