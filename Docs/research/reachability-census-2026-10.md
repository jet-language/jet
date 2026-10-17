# Reachability census (repo dev bd874ed71, 2026-10)

Read-only census of what each build step includes, measured at dev commit bd874ed71.

Findings:

- Module loading is file-granular.
- Core bodies are reachability-pruned (Pipeline.jet:3073; CoreReachability.jet:97-123).
- The generated Rust prelude keeps about 110 parts unconditional (mod.rs:309-420, 729-753).
- The native x64 build always includes 32 runtime functions (Image/Runtime.jet:741-775).
- Monomorphization is demand-driven (TIR/mod.rs:1417-1420).
- Codec pruning is default-off (MIRRust.rs:370).

This census corresponds to lever 12 in the speed plan (see compiler-speed-levers-2026-10.md). The source summary does not record a later S6 plan in Tower.
