# Card #1414: public compiled-workload task sources

This note freezes task sources for the gate. It records task definitions and
build boundaries. It does not report performance.

## Source rule

Use the named tag or revision in `tests/compiled_workloads/peer_ledger.tsv`.
Before a measurement run, resolve the tag to a commit hash and record the
hash in the report. Do not compare moving branches.

The source set uses first-party repositories and benchmark definitions:

- [ripgrep](https://github.com/BurntSushi/ripgrep) defines the systems CLI
  workload and publishes versioned releases.
- [GNU Coreutils](https://github.com/coreutils/coreutils) supplies a domain
  peer for file and process behavior.
- [TechEmpower FrameworkBenchmarks](https://github.com/TechEmpower/FrameworkBenchmarks)
  defines HTTP service tasks, test implementations, and run boundaries.
- [libarchive](https://github.com/libarchive/libarchive) defines the archive
  CLI and library boundary.
- [Serde JSON](https://github.com/serde-rs/json) defines the typed JSON
  library boundary and test entry point.
- [JSONTestSuite](https://github.com/nst/JSONTestSuite) supplies hostile JSON
  inputs for the library task.
- [Go](https://github.com/golang/go), [Swift](https://github.com/swiftlang/swift),
  and [Zig](https://codeberg.org/ziglang/zig) provide the language toolchain
  and standard-library peer paths.
- [Embassy](https://github.com/embassy-rs/embassy) and
  [Zephyr](https://github.com/zephyrproject-rtos/zephyr) define embedded
  firmware build and board-run paths.
- [CoreMark](https://github.com/eembc/coremark) supplies a portable embedded
  compute peer.
- [Qt](https://code.qt.io/cgit/qt/qtbase.git), [Fyne](https://github.com/fyne-io/fyne),
  [Swift](https://github.com/swiftlang/swift), [raylib](https://github.com/raysan5/raylib),
  and [Flutter](https://github.com/flutter/flutter) define cross-platform app
  peers.
- [SDL](https://github.com/libsdl-org/SDL) supplies a domain peer for the
  window and input path.
- [The Computer Language Benchmarks Game](https://benchmarksgame-team.pages.debian.net/benchmarksgame/description/nbody.html)
  supplies a public compute task definition for n-body and related programs.
- [jq](https://github.com/jqlang/jq) and [SQLite](https://www.sqlite.org/)
  are domain peers for command-line data work and the library boundary.

The TechEmpower project describes implementations as code and configuration
that satisfy a test definition. The gate uses that task boundary. It does not
copy a leaderboard rank into a language claim.

## Frozen rows

| Task | Complete work | Public definition | Peer candidates |
| --- | --- | --- | --- |
| `systems-file-index` | large-tree index and hostile path handling | ripgrep CLI and release source | Rust, C++, gate candidates |
| `service-json-http` | health, readiness, JSON request, shutdown | TechEmpower HTTP task definitions | Go, Rust, domain service |
| `cli-archive-filter` | archive inspection and hostile headers | libarchive and bsdtar source | C++, Rust, domain CLI |
| `library-json-roundtrip` | public typed API, tests, canonical JSON | Serde JSON source and test boundary | Rust, C++, Go, Swift, Zig |
| `compute-mandelbrot` | fixed checksum, parallel mode, failure cases | language standard-library build paths plus benchmark task | Zig, Rust, C++, Go, Swift |
| `embedded-sensor-ring` | freestanding firmware, board smoke, host replay | Embassy and Zephyr board/sample paths | Rust, C++, domain C ABI |
| `cross-platform-notes` | keyboard interaction, persistence, hostile save | Qt, Fyne, SwiftUI, raylib, Flutter examples | C++, Go, Swift, Zig, domain app |

The task definitions in `tests/compiled_workloads/task-definitions/` freeze the
input shape, hostile cases, beginner default, expert control, and output
contract. The peer ledger freezes the build command, run command, dependency
rule, source boundary, and target list.

## Known source limits

Some peer rows are candidates, not selected best peers. The gate requires one
`best-applicable` row per task. The selected row can change only with a new
measurement record and owner review.

Swift is not applicable to the embedded firmware row in this manifest. The
domain contract records that fact. It is not treated as a zero or a Jet win.

No row is evidence of external trust, community size, or ecosystem age. Those
claims stay outside this gate.
