# Compiled-workload source boundary

**Status:** Active gate input. Card #1414 is in `building` and currently blocked by the shared measurement card #2858. This note records task identity and source boundaries only. It does not report performance, runtime success, or compiled-language parity.

## Purpose

The compiled-workload gate must compare complete Jet work with an applicable peer without changing the task after the fact. The task definitions freeze the input shape, hostile cases, beginner default, expert control, and output contract. The peer ledger freezes the build command, run command, dependency rule, source boundary, and target list.

## Frozen task rows

| Task | Complete-work boundary | Public definition | Peer candidates |
|---|---|---|---|
| `systems-file-index` | Large-tree index and hostile path handling | [ripgrep](https://github.com/BurntSushi/ripgrep) CLI and release source | Rust, C++, gate candidates |
| `service-json-http` | Health, readiness, JSON request, and shutdown | [TechEmpower FrameworkBenchmarks](https://github.com/TechEmpower/FrameworkBenchmarks) task definitions | Go, Rust, domain service |
| `cli-archive-filter` | Archive inspection and hostile headers | [libarchive](https://github.com/libarchive/libarchive) and bsdtar source | C++, Rust, domain CLI |
| `library-json-roundtrip` | Public typed API, tests, and canonical JSON | [Serde JSON](https://github.com/serde-rs/json) source and test boundary | Rust, C++, Go, Swift, Zig |
| `compute-mandelbrot` | Fixed checksum, parallel mode, and failure cases | Language standard-library build paths plus the benchmark task | Zig, Rust, C++, Go, Swift |
| `embedded-sensor-ring` | Freestanding firmware, board smoke, and host replay | [Embassy](https://github.com/embassy-rs/embassy) and [Zephyr](https://github.com/zephyrproject-rtos/zephyr) samples | Rust, C++, domain C ABI |
| `cross-platform-notes` | Keyboard interaction, persistence, and hostile save | Qt, Fyne, SwiftUI, raylib, and Flutter examples | C++, Go, Swift, Zig, domain app |

The first-party task definitions live in `tests/compiled_workloads/task-definitions/`. The peer ledger is `tests/compiled_workloads/peer_ledger.tsv`; resolve every named tag or revision to a commit hash before measurement and record that hash in the resulting report.

## Selection rules and alternatives

- Candidate peers are not automatically the selected best peers. The gate needs one `best-applicable` row per task, selected by a new measurement record and owner review.
- Swift is structurally inapplicable to the embedded firmware row. Record that fact; never turn it into a zero or a Jet win.
- A task row is not evidence of external trust, community size, or ecosystem age. Those questions remain outside this gate.
- Keep one shared workload harness. Do not create a second benchmark framework or substitute a synthetic task for the public definition.

## Current card evidence

Read-only Tower queries on 2026-09-05 report:

- #1414: `building`; criterion 1 (the frozen manifest) is met; criteria 2–7 remain open. Its lane is blocked by #2858.
- #2858: `ready`; it owns the deferred matched runs and does not accept missing, unmeasured, or invalid rows as green. Its latest logged full run was `0 win, 0 parity, 11 loss, 14 unmeasured`; this is a Tower status record, not a result produced by this note.
- #2919 remains the separate post-implementation runtime and suite campaign. Closure of source or implementation cards does not provide runtime proof for that campaign.

## Evidence and recovery

- Task definitions: `tests/compiled_workloads/task-definitions/`
- Peer identity ledger: `tests/compiled_workloads/peer_ledger.tsv`
- Gate owner: #1414, with deferred measurement in #2858
- Existing compiled-workload audit: `docs/audits/compiled-workload-gate.md`
- Original working note is retired after this distillation. Exact source recovery: `git show cleanup-source-2026-09-05-114107:docs/research/card-1414-compiled-peer-task-definitions.md` (blob `5e24166d60781bdf750b3d9c2f74d9b169c43c77`).

## Proof still required

Do not claim completion from this topic note. The gate still needs matched peer execution, all required comparable metrics, cross-target/tier evidence where applicable, removal canaries, independently reviewed fairness, and one result for every required cell. Those proofs are intentionally unrun in this cleanup.
