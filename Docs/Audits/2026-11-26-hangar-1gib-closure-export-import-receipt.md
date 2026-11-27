# Hangar >1 GiB Offline Closure Export and Import Receipt

Date: 2026-11-26
Card: `#6933` (c5)
Compiler: `jet@a0.0.46#bc46b6d42ba8829b056a2f1f40179f2c0fe5302902eec9b1a15a4c09b6266e85`
Host: Linux x86_64 (`$JET_SCRATCH_ROOT`)

## Summary

A multi-object toolchain closure exceeding 1 GiB (`1,208,111,845` bytes / `1.1251 GiB`, signed `jet-hangar-archive-v1`) was exported via streaming `jetpack hangar export` and imported into an empty Hangar via streaming `jetpack hangar import` with `JETPACK_DENY_NETWORK=1`, `PATH=/usr/bin:/bin`, and no host Nix.

- Peak RSS stayed under **18 MiB** for both export and import (1.5% of the archive size).
- Shared objects and identical files across closure objects were stored **once** via content-addressed object reuse and `shared-cas/` hardlink deduplication (`128.0 MiB` saved on first import; `0` additional bytes on idempotent re-import).
- Both replayed native binaries compiled with `jet build` executed cleanly after offline replay.

## Closure Composition

- Root entry: `jet-toolchain@offline` (`version: 1.0.0`, multi-output closure: `out` + `dev`)
- `out` object (`sha256-49e11c9338e1dbedea20d1a1f438f3a2ab088362397a6c374b77fa026fa2801f`):
  - `bin/toolchain_driver` (native binary built with `jet build`)
  - `lib/compiler-core-a.a` (`256 MiB`)
  - `lib/compiler-core-b.a` (`256 MiB`)
  - `share/stdlib-shared.pack` (`128 MiB`, byte-identical to `dev`)
- `dev` object (`sha256-aed108fd355569af3f07dcec63795a9d9f9d8bffc0e1b2f3f6c394cbb751edc6`):
  - `bin/toolchain_check` (native binary built with `jet build`)
  - `lib/runtime-sysroot-a.a` (`192 MiB`)
  - `lib/runtime-sysroot-b.a` (`192 MiB`)
  - `share/stdlib-shared.pack` (`128 MiB`, byte-identical to `out`)
- Total archived payload bytes: `1,208,106,416` (`1,152.14 MiB`)
- Signed `.hangar` archive file size: `1,208,111,845` bytes (`1.1251 GiB`)

## Measured Results

Environment for all steps: `JETPACK_DENY_NETWORK=1`, `NO_COLOR=1`, `PATH=/usr/bin:/bin` (no network, no host Nix). Peak RSS was measured per isolated child process via `resource.getrusage(RUSAGE_CHILDREN).ru_maxrss`.

| Step | Exit | Wall Time (s) | Peak RSS (KiB) | Peak RSS (MiB) | Notes |
|---|---:|---:|---:|---:|---|
| `hangar ingest` (source Hangar) | 0 | 4.526 | 286,256 | 279.55 | Multi-output ingest (`out` + `dev`) |
| `hangar export --yes --json` | 0 | 2.133 | 17,380 | 16.97 | `objects=2`, `bytes=1208106416`, `signed=true`, archive `1.1251 GiB` |
| `hangar import --yes --json` (empty Hangar) | 0 | 2.986 | 17,172 | 16.77 | `objects=2`, `bytes=1208106416`, `signed=true` |
| `hangar import --yes --json` (idempotent repeat) | 0 | 2.790 | 18,404 | 17.97 | Physical disk delta: `0` bytes |
| `jetpack use --offline -- toolchain_driver` | 0 | 1.270 | 24,656 | 24.08 | Output: `toolchain-driver 1gib-offline-ok` |
| Replayed `dev/bin/toolchain_check` | 0 | 0.000 | 13,428 | 13.11 | Output: `toolchain-check 1gib-offline-ok` |

## Disk Footprint and Deduplication Proof

- Destination Hangar objects (`$JET_SCRATCH_ROOT/dest-hangar/hangar/objects/`): exactly `2` unique content-addressed directories (`out` and `dev`).
- Unique physical bytes across `$JET_SCRATCH_ROOT/dest-hangar`: `1,073,912,888` bytes (`1,024.16 MiB`) vs `1,208,106,416` logical payload bytes (`1,152.14 MiB`) — `128.0 MiB` deduplicated.
- `share/stdlib-shared.pack` in both `out` and `dev` shares the exact same filesystem inode (`nlink = 3`: `out`, `dev`, and `shared-cas/`).
- Re-importing the same `1.1251 GiB` archive added `0` physical bytes (`1,073,912,888` bytes before and after).

## Automated Unit Coverage

- `Jetpack/Store/Tests/Archive.jet`: `#Test("Store/closure_export_import_offline")` (`jet test Compiler --profile packages --filter Store/closure_export_import_offline`) verifies streaming HMAC-SHA256 bit-for-bit parity with `TrustKey.sign`, exact wire parity between streaming and in-memory archive encoding, fail-closed tamper rejection on an empty Hangar, single-copy shared dependency + shared-CAS hardlink storage across multi-package closures, idempotent re-import, and verified `CacheLease` execution of replayed binaries.
