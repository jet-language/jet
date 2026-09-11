# Executable tier census — 2026-09-03

## Result

This report inventories the authoritative TIR enums and the `CoreCallRecord` registry. It scans source and emits one named three-tier differential probe per row. Executable mode measured one batch of 1188 differential markers on AOT, Cranelift JIT, and the interpreter.

Command:

```text
node scripts/agent/tier-census.mjs
```

The census has **150 TIR variants** (103 `TExprKind`, 47 `TStmt`) and **1038 `CoreCallRecord` rows** (779 plain, 259 receiver). It emits **1188 named probes** in `tests/fixtures/tier-census/programs.json`. The TSV has **1196 data rows** plus one header (**1197 rows total**). The exhaustive table is in [JSON](tier-census-2026-09-03.json) and [TSV](tier-census-2026-09-03.tsv).

A status is `covered` only when the backend has an explicit variant arm, host symbol, pair projection, or support branch. `refused` records an explicit refusal or arity rejection. `absent` records no explicit static consumer. Wildcard arms, default constructors, and default route logic never count as coverage. The script exits nonzero if it cannot classify a declaration or finds an unknown TIR reference.

## Static status

`covered / refused / absent` counts are shown for TIR variants and Core rows. Dynamic cells are every non-covered cell that still needs an execution probe.

| backend | TIR | CoreCallRecord | dynamic cells |
| --- | --- | --- | --- |
| aot_rust | 150 / 0 / 0 | 769 / 69 / 200 | 269 |
| interpreter | 150 / 0 / 0 | 1038 / 0 / 0 | 0 |
| cranelift | 150 / 0 / 0 | 701 / 259 / 78 | 337 |
| web | 150 / 0 / 0 | 732 / 102 / 204 | 306 |

Uncovered cells: **912**.

| surface | aot_rust | interpreter | cranelift | web |
| --- | --- | --- | --- | --- |
| TExprKind + TStmt | 0 | 0 | 0 | 0 |
| CoreCallRecord | 269 | 0 | 337 | 306 |
| Total | 269 | 0 | 337 | 306 |

## Spelling pairs

Interpreter wall time was measured for both forms of every pair. Pairs outside 10% cite #2886 and #2892.

| pair | canonical | alternate | static evidence | source span | timing |
| --- | --- | --- | --- | --- | --- |
| `if_else_vs_match` | if / else | match | `crates/jet-codegen/src/Codegen/TIR/mod.rs:4775`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:4842`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8511` | `7f8766c7fc998d0c44a86d4cf40864cd8e6f4ae9c7c56a9d626bac0b94173f2e` | canonical 163.342 ms; alternate 164.395 ms; mean 163.869 ms; delta 0.64% |
| `fallback_vs_match` | ?? fallback | match on the failure carrier | `crates/jet-codegen/src/Codegen/TIR/mod.rs:8573`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8556` | `1b0c36793fd67f9c1707e4f75bf08c709d79409492874ad3a2229853481935cf` | canonical 153.043 ms; alternate 154.418 ms; mean 153.731 ms; delta 0.9% |
| `compound_assign_vs_binary_assign` | compound assignment | binary expression followed by assignment | `crates/jet-codegen/src/Codegen/TIR/mod.rs:4734`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8129` | `e3f5ff7d0d698aead47c0bc87a852a38e2a5cb3b6db5686bdb4281cb678c1f42` | canonical 149.426 ms; alternate 149.527 ms; mean 149.477 ms; delta 0.07% |
| `interpolation_vs_concat` | interpolation | text concatenation | `crates/jet-codegen/src/Codegen/TIR/mod.rs:8004`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8496` | `2962c53eb3c645be05f7a3245bdada5ca1ed71d4b825a86454c974d2037b4612` | not-measured: tier command exited 101 |
| `range_loop_vs_indexed_loop` | range loop | indexed loop | `crates/jet-codegen/src/Codegen/TIR/mod.rs:4808`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:4911`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8350` | `b86eded7c3abe68e31300136f5a53f0e729a90dcdcbf690f435d8c224ddf8a93` | not-measured: tier command exited 101 |
| `explicit_copy_vs_implicit_copy` | explicit copy | implicit clone/materialization | `crates/jet-codegen/src/Codegen/TIR/mod.rs:8414`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8410`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:8426` | `72b97ea804f9dd1efd744229b909dfb47859e09866312c6f8aaa37fa897dcc74` | canonical 180.2 ms; alternate 178.273 ms; mean 179.236 ms; delta 1.08% |
| `map_method_vs_loop_push` | map method | loop plus push | `crates/jet-codegen/src/Codegen/TIR/mod.rs:8623`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:4911` | `e95440ad0fe52f66eb940511da9ae1a9bc62c23dd2d93ce36305c4e7a4804c10` | not-measured: tier command exited 101 |
| `expr_arrow_vs_block_body` | expression-bodied function | block-bodied function | `crates/jet-codegen/src/Codegen/TIR/mod.rs:8597`, `crates/jet-codegen/src/Codegen/TIR/mod.rs:4941` | `b672dce6aee7d990bb1946b6220966fb66cf61b42dfb046eaac980f2826175e2` | canonical 148.522 ms; alternate 143.378 ms; mean 145.95 ms; delta 3.59% |

## Sources and limits

- TIR declarations: `crates/jet-codegen/src/Codegen/TIR/mod.rs`.
- Core registry and ambient route manifest: `crates/jet-foundation/src/Syntax/core_calls.rs`.
- AOT projection: `crates/jet-codegen/src/Codegen/MIRRust.rs`.
- Interpreter evaluator: `crates/jet-codegen/src/Codegen/MIREval.rs` plus ambient route plumbing in `crates/jet-jit/src/ambient_interp.rs`.
- Cranelift host and lowering sources: `crates/jet-jit/src`.
- Web projection and symbol resolver: `crates/jet-codegen/src/Codegen/MIRWeb.rs`.

The JSON keeps exact file, line, source-line, and stable source-span-hash evidence for every row. Line numbers are display coordinates; the hash is the durable identity used by fixture comparison. The named probes are checked into `tests/fixtures/tier-census/programs.json`; executable mode records the batch and per-tier outcomes in the JSON and manifest.
