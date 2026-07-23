# Syntax-law/source status matrix — 2026-07-07

Purpose: machine-readable enough inventory for #339. Rows cover every current
`unbuilt` / `not yet implemented` note in `docs/spec/syntax-decisions.md`, plus
the adjacent source-law drift found by the language audit. Downstream cards
close stale docs or implement true gaps; this file does not reopen syntax.

Status keys:

- `shipped`: source evidence exists; docs/tests may still need cleanup.
- `partial`: some source exists, but the ratified breadth is not proven.
- `gap`: ratified text has no complete implementation evidence.
- `gated`: intentionally blocked by a named later gate or owner approval.
- `declined`: no implementation should appear.

| ID | Spec hook | Status | Source evidence | Follow-up |
| --- | --- | --- | --- | --- |
| `D-S14-PAUSE` | S14 alias policy | shipped | `retired_s14_teaching_is_paused`; parser recoveries gated off; `while`/`for` lex as identifiers | #338 done |
| `S74-D-DESTRUCT1-ARM` | dispatch-arm struct-pattern head note | shipped, stale doc | `crates/jet-parser/src/Parser/Expressions.rs` parses `Pattern::Struct`; `crates/jet-sema/src/Sema/CheckerItems.rs`; `crates/jet-codegen/src/Codegen/TIR/{lower,subset}.rs`; `tests/tir_patterns_and_fields.rs` struct-arm coverage | #341 |
| `D-REFINE1` | refinements | shipped | Parser accepts `#Invariant("value >= lo && value < hi")` on `distinct Int`; sema proves fixed-list indexes in-bounds; TIR lowers proven indexes without `jet_index_vec`; `examples/features/types/refinements.jet` and `tests/corelib.rs` cover it | #347 |
| `D-COLLBREADTH1` | collections breadth | partial | `Deque<T>` / `Set<T>` shipped; HashMap and iterator breadth still audit-sized | #305 |
| `D-ITER1` | iterator adapters | partial | Core iteration exists, full Rust/itertools parity not proven | #305 |
| `D-TYPEDTEXT2` | typed text prefixes | shipped | Expected-type `Sql`/`Html` path shipped; parser recognizes adjacent `sql"..."` / `html"..."` prefixes and sema rewrites through the same typed-text constructor path; `examples/features/safety/typed_sql.jet` covers no-expected-type bindings | #348 |
| `D-IGNORERET1` | visible discard sigil | shipped by successor, stale doc | `D-IGNORERET2` `.drop("reason")`; sema checks in `crates/jet-sema/src/Sema/CheckerCore.rs`; examples under `examples/features/errors/` | #340 |
| `D-SMELLLINT1` | semantic-smell lints | partial | Some lint infrastructure exists; float-eq/duplicate-branch/always-true breadth not proven | #343 |
| `D-REPLAY1` | `#Replayable` soundness | shipped | `Func.is_replayable`; parser `#Replayable fn`; effect fixpoint E0725 rejects reachable `Time`/`Rand`/`Net`/`Io`; `tests/ui/replayable_reaches_io.jet` proves transitive `Io` rejection | #349 |
| `D-CTFIND1` | comptime `find(glob)` | shipped | `crates/jet-comptime/src/Comptime/Methods.rs` implements sorted std-only glob `*`/`**`/`?`/`{a,b}`/`[a-z]`; `examples/features/comptime/find.jet` covers it | #350 |
| `D-CTFIND2` | comptime `find(glob)` lock semantics | shipped | `find` records each matched file as `ComptimeInput { path, hash }`; `comptime_find_glob_records_sorted_lock_inputs` covers sorted lock evidence | #350 |
| `D-PLUGIN1` | `target: plugin` WASM component target | shipped | `target: plugin`, deny-by-default effects, `core.plugin`, export shape checks, and version diagnostics are implemented and tested | #351 |
| `D-DEP-WASM1` | sandboxed WASM dependency target | shipped | Component build path emits `.wit`, invokes `wasm-tools`, reports E1259 on tool failure, and loads through wasmtime host with zero imports | #351 |
| `D-NOSTD1` | no `no_std` flag; target controls runtime layer | shipped, stale doc | Card #118 done; runtime layering tied to typed `target:` law | #340 |
| `D-PATHFS1` | typed Path API | shipped, stale doc | `crates/jet-codegen/src/Prelude/CoreLib.rs`; `crates/jet-sema/src/Sema/CheckerCoreLib.rs`; TIR method lowering; `examples/features/io/path.jet` | #340, #288 |
| `D-TIMEDEPTH1` | civil time depth | shipped/partial, stale doc | `core.time.date` / `datetime` sema, prelude, TIR emit; examples/tests use `core.time` | #340, #295 |
| `D-MATHLIB1` | vectors/matrices/decompositions/FFT | partial | scalar `core.math` shipped; `Matrix<M,N>` substrate still unproven | #293 |
| `D-HTTPLIB1` | HTTP server surface | partial | std HTTP server/router in CoreLib; route duplicate runtime leak still fails global diagnostic bar | #301, #343 |
| `D-HTTPLIB2` | HTTP request/response ergonomics | partial | client/server builders in sema/TIR/CoreLib; parity breadth not proven | #301 |
| `D-HTTPLIB3` | middleware/websocket/depth | gap/partial | basic blocking HTTP shipped; websocket/middleware parity not proven | #301 |
| `D-ROUTE1` | router and params | shipped/partial | CoreLib router and `req.params["id"]`; tests/examples under net/http routes | #301 |
| `D-HONESTNUM1` | `core.science.measurement` | shipped/partial, stale doc | measurement type sema/TIR/CoreLib and `examples/features/types/measurement.jet` | #340, #310 |
| `D-OPTGC1` | scoped automatic GC | implemented | `#Policy(gc)` / package `gc: true`; no public wrapper | #658 |
| `D-WEBAPP1` | remaining `app.live` application graph | gap/partial | `Db.Read`/`Db.Write` leaf inference shipped; `app` graph, `core.ws`, client signal binding, and transactional invalidation remain unbuilt | #438, #134 |

Open global proof issue discovered while verifying #338/#339: `cargo test --test
diagnostic_snapshots ui_snapshots` currently fails because
`tests/ui/E2714_derive_old_for.jet` emits generic `E0003` instead of the
snapshot-pinned `E2714`. That belongs to #343/#344, not this matrix.
