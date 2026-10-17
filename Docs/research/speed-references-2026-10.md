# Compiler speed techniques in other toolchains (reference notes, 2026-10)

Scope: reference notes on how other compilers reach fast builds, cited by the Jet speed plan. Source notes were only partly read; the points below are the ones named in the row summaries.

- Go 1.26.7: per-package compile with action IDs; function-level backend queue ordered longest-first (compile.go:32-215); parser allocates a string per identifier (scanner.go:66, 392). Few measured numbers.
- Zig: InternPool, MultiArrayList, lazy Sema, ZIR cache, Config.zig.
- Carbon: ID types and SIMD lexing. TCC: 16,384-bucket hash. rustc: arenas and interning.

No repo Docs page matched these topics before this note. Re-read the sources before citing specific numbers.
