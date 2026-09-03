# Storage primitive probe

## What I built
I built a Jet package that uses the shipped filesystem, crypto, channels, tasks, and time primitives as storage-library building blocks. It implements an append-only fsync log, log-backed KV with compaction, a bounded pool, an at-least-once queue with consumer-group acknowledgements, and a content-addressed cache. A separate writer/recovery pair kills a process after a durable partial record and recovers the committed prefix.

Files: `pkg/main.jet`, `pkg/log.jet`, `pkg/kv.jet`, `pkg/pool.jet`, `pkg/queue.jet`, `pkg/cache.jet`, `pkg/storage_probe.jet`, `pkg/crash_write.jet`, `pkg/recover.jet`, `pkg/mmap_probe.jet`.

## What worked

- `JET_STORE_DIR=... scripts/agent/jet-env jet run pkg/storage_probe.jet` -> `log=first\nsecond`, `atomic_ok=true`, SHA-256 hash, `lock_exists=true`, `stream_lines=2`, `done`.
- `JET_STORE_DIR=... scripts/agent/jet check pkg/main.jet` -> passed; only L2510 hidden-cost warnings.
- `JET_STORE_DIR=... scripts/agent/jet run pkg/main.jet` -> append log `2` records; KV update/delete/replay; pool `[1, 2, 2]`; queue redelivery and drain; cache same-key and round-trip; `storage_probe_complete`.
- `jet build pkg/storage_probe.jet` -> `Built build/storage_probe in 12.7s`.
- Crash writer was started with `hub start`, reached `mid_write_ready`, then was SIGKILLed. Surviving bytes were `event|1|committed` plus `event|2|`.
- `JET_STORE_DIR=... jet run pkg/recover.jet` -> `recovered_records=1`, `discarded_tail=true`.
- `fs.write_atomic` is durable in the inspected implementation: temporary file `sync_all`, rename, then Unix parent-directory `sync_all` (`crates/jet-codegen/src/Prelude/CoreLib/Top/FSWriteOps.rs:44-131`).
- `fs.lock` is an atomic `create_new` lock file with RAII cleanup (`crates/jet-codegen/src/Prelude/CoreLib/Top/FSIoEnvOsTesting.rs:174-190`).

## Gaps

### prim-storage-G1 — impossible — file-backed memory mapping

Primitive capability: an OS-backed memory-mapped file / mapped byte region with lifetime and flush controls.

Evidence: `JET_STORE_DIR=... scripts/agent/jet-env jet check pkg/mmap_probe.jet` -> `Error [E1004]: core.files has no item mmap`; the diagnostic's complete alternatives list includes `read`, `read_bytes`, `read_at`, `write_at`, `fsync`, `write_atomic`, `lock`, and streaming APIs, but no mapping API. Areas: backend, data. Severity: hurts. Workaround: `read_bytes`/`read_at` and explicit buffering; this is not equivalent for large random-access files.

## Friction

The safe path is available but library code must explicitly sequence append/write, `fsync`, lock-file ownership, framing, replay, and compaction. `FileWriter.flush()` is a userspace flush; durable publication still requires a separate path-level `fsync`. Queue at-least-once semantics are author protocol code over the log and ack log, not one runtime primitive. Main checks emit L2510 warnings for repeated outcome/map work in replay-heavy code.

## Defects

### prim-storage-G2 — defect — imported fallible storage module fails native build

`jet check pkg/build_log.jet` passes, and `jet run pkg/main.jet` works, but `JET_STORE_DIR=... scripts/agent/jet-env jet build --verbose pkg/build_log.jet` emits `internal compiler error: the generated Rust did not compile` (`generated: .../build/build_log.rs`). The full integrated `jet build --verbose pkg/main.jet` fails the same way (`build/main.rs`). This blocks AOT delivery of the modular storage library while the single-file filesystem probe builds.

## Verdict

Today’s Jet is sufficient to build a correct local append/KV/queue/cache library with fsync, atomic replacement, lock files, streams, channels, tasks, and crash-prefix recovery. The material missing storage primitive is memory mapping; the material delivery blocker is the native compiler defect for imported fallible storage modules. No battery file: this is a cross-cutting primitive probe, not a critical-area battery.
