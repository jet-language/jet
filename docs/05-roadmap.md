# 05 — Roadmap

Each milestone is done when its exit criteria pass as tests. Examples are
the executable spec: a milestone ships with new examples/ programs and new
tests/ui fixtures, all green.

**M3 onward each have a full implementation plan in docs/plans/** (one
file per milestone: surface, grammar, sema rules, lowering, diagnostics,
tests, out-of-scope). Implementing agents follow docs/plans/README.md.
Plans are gated on the decision ballots in docs/06-decision-ballots.md —
a milestone may not start until its ballot group is ratified in docs/02.

**Owner direction (2026-06-11):** the v1.x horizon is a complete
language — data types, errors, collections, closures, generics/traits,
std library, concurrency, package manager, real LSP — good enough that
experts rewriting small Rust/Go/C tools would *choose* Jet. Formerly
"deferred indefinitely" items (generics, threads & channels, package
manager) are hereby promoted onto the roadmap below. Philosophy ranks
are unchanged; single-file `jet run` stays ceremony-free forever (R9).

## M0 — Walking skeleton  *(done; verified 2026-06-11)*

Hello world end-to-end: jet → parse → sema → emit Rust → rustc → run.
Diagnostics framework, ui snapshot harness, golden harness, ICE policy.
**Exit:** `cargo test` green; `jet run examples/01_hello.jet` prints. ✓

## M1 — Values and expressions  *(done; verified 2026-06-11)*

Bindings (S2: `val`/`var`), Int/Float/Bool/String (S11), arithmetic + comparison,
compound assignment (S17: `+=` `-=` `*=` `/=` `%=` `&=` `|=` `^=` `<<=`
`>>=`),
string interpolation (S8), escape sequences + `{{`/`}}` literal braces (S20),
multi-argument calls, `if`/`else`, `while` + `for i in <range>` loops
(S19; inclusive ranges S22; `break`/`continue` S23), `switch` with
condition arms (S24), Float display rule (S21),
local type inference (annotations optional, S4).
Compiler work: error recovery (multiple parse errors per run),
unicode-aware caret columns, E0005 retires. Teaching errors for familiar
foreign spellings (S14): recognize `and`/`or`/`not`, `try`, `let`/`let mut`,
`func`/`def`, `println`, `set`, `Text`, `use`, `match` and point to the
canonical Jet form (E0008–E0016). Comparison distribution in `&&`/`||`
chains (S25). No autocorrect yet — that's M6/LSP.
**Exit:** examples 03–07 (fizzbuzz-class programs + switch) run; ui suite
covers every new error; type errors name both types in plain words. ✓

## M2 — Ownership v1  *(done; verified 2026-06-11)* ★ the crown jewel

Moves, implicit copy for scalars, explicit `.clone()`, parameter access
keywords (S10: default/`mut`/`take`/`view`/`ref`), the ownership checker
in sema, E02xx diagnostics written to docs/04 voice rules. Teaching errors
for foreign `read`/`write` (E0017/E0018). References cannot be stored or
returned in tier 1 — therefore no lifetime syntax exists.
**Exit:** an example that *fails* ownership exists for every E02xx code,
each with a `.fixed.jet` companion that compiles (`tests/ui_fixes.rs`);
lint snapshots in `tests/ui_lint/`; golden tests prove rustc never rejects
what sema passes (the verifier earning its keep). ✓

## M3 — Data  *(plan: docs/plans/m03-data.md; ballots: Group 2 ✅)* ✓ 2026-06-11

Structs, enums (sum types), `switch` exhaustiveness for enums ("you
forgot the `Circle` case"), `==` pattern tests, Option (`T?`, no null,
ever), methods (S27: `self`, `c.area()`, definable inside the type or
in `impl Type { }` blocks), invisible auto-boxing for recursive types.
No inheritance, ever (non-goal). Traits/interfaces (S28) are explicitly
out of M3 — they land in M9.
**Exit:** examples `10_structs`–`13_recursive_enum`; every E03xx/L0301
and E0020–E0023 has a ui snapshot; exhaustiveness errors list missing
cases verbatim. ✓

## M4 — Errors as values  *(plan: docs/plans/m04-errors.md; ballots: Group 3)*

`T or E` fallible returns, `ok`/`err`, propagation `?` (S7), `or`
fallback, `panic`/`assert` for bugs with a friendly runtime report.
No exceptions, no null, no silently ignored failures.
**Exit:** a file-parsing example showing the happy path staying clean;
the runtime report format pinned by a golden stderr test.

## M5 — Collections & one string story  *(plan: docs/plans/m05-collections.md; ballots: Group 4)*

`List[T]`, `Map[K, V]` (bridging Rust's Vec/BTreeMap internally),
literals, iteration, indexing with friendly runtime reports, copy-based
slicing without exposing references, `Char`, a real String API. Exactly
one string type.
**Exit:** wordcount example; out-of-bounds and iterator-invalidation
mistakes produce great errors, not Rust concepts.

## M6 — Tooling I  *(plan: docs/plans/m06-tooling.md; ballots: Group 5; four phases)*

`jet fmt` (one true style, zero config), `jet test` (`test "name" { }`
blocks), `jet new`. Multi-file imports (S16: `import "path" as alias;`)
and cross-file visibility enforcement (S18). A `jet build --small`
profile (S15). LSP **v0**: diagnostics + S14 autocorrect quick-fixes +
formatting, with a minimal VS Code extension. Single binary, no config
files (philosophy: minimal configuration).
**Exit:** fmt is idempotent on all examples; autocorrect turns a pasted
C-style snippet into canonical Jet; `--small` produces a measurably
smaller binary than the default; a new project runs in two commands.

## M7 — Rust FFI (interop tier)  *(plan: docs/plans/m07-ffi.md; ballots: Group 5)*

`extern rust` blocks for calling vetted Rust functions across an
owned/copied boundary (no borrowed returns), version-pinned, built via a
hidden cached cargo bridge — the user's directory never grows a cargo
project. This is C2's resolution: interop without importing Rust's type
system.
**Exit:** an example calling a real Rust crate function.

## M8 — Functions as values  *(plan: docs/plans/m08-closures.md; ballots: Group 6)*

Lambdas, function types, closures whose captures obey the M2 ownership
rules (no Fn/FnMut/FnOnce surfaced), and the closure-powered collection
methods: `map`/`filter`/`each`/`find`/`sort_by`/`reduce`.
**Exit:** a pipeline example; capture-ownership fixtures both failing
and fixed; rustc-as-verifier battery over Fn-inference cases.

## M9 — Generics & traits  *(plan: docs/plans/m09-generics-traits.md; ballots: Group 6; resolves S26/S28)*

`fn f[T: Trait]`, generic structs/enums, `trait` + `impl Trait for
Type`, trait-as-type with invisible boxing/dynamic dispatch, built-in
`Printable`/`Comparable`/`Equatable`. Monomorphized by rustc, proven by
sema (R2). Comptime (S26) closes as rejected for v1.
**Exit:** shapes-with-traits example; generic container example; an
instantiation soundness matrix test.

## M10 — Standard library  *(plan: docs/plans/m10-stdlib.md; ballots: Group 7)*

`import "std/…"`: fs, io, env, process, math, random, time, json —
exact v1 APIs frozen in the plan; every fallible call returns `T or E`.
`Byte` and byte/string conversions. Enough batteries for real CLI tools.
**Exit:** file-transform, JSON, and mini-CLI examples with golden tests.

## M11 — Concurrency  *(plan: docs/plans/m11-concurrency.md; ballots: Group 7)*

Tasks + channels (`tasks.spawn`, `Task[T].join`, `Channel[T]`), no
shared mutable state — the ownership checker proves data-race freedom
with no lifetime syntax. Panics in tasks fail loud at `join`.
**Exit:** parallel-sum and producer/consumer examples with
deterministic goldens; shared-capture fixtures failing with the human
framing and fixed with channels.

## M12 — Package manager  *(plan: docs/plans/m12-packages.md; ballots: Group 7; two phases)*

Opt-in `jet.toml` (path + git deps, exact pins), `jet add`/`jet fetch`,
content-hashed `jet.lock`, FFI deps in the manifest, no install-time
code execution. Phase 2: static-index registry. Single files never need
any of it (R9).
**Exit:** fixture workspaces covering resolution, locking, conflicts,
and tampering; an end-to-end new→add→run flow.

## M13 — LSP v2  *(plan: docs/plans/m13-lsp.md)*

The real language server: completion (incl. switch-arm snippets for
enums), hover with types + ownership + doc comments, go-to-definition,
references, rename, structured quick-fixes (shared with a new CLI
`--fix`), semantic tokens, inlay hints. Crash-proof, latency-budgeted,
fed by unsaved buffers. Tree-sitter + TextMate grammars.
**Exit:** scripted LSP transcript tests per capability; a bench harness
under budget in CI.

## M14 — v1.0  *(plan: docs/plans/m14-v1.md)*

The proof: three showcase tools (grep-lite, JSON formatter, parallel
wordfreq) benchmarked at ≤1.5× their Rust references. Diagnostics,
soundness-fuzz, and performance audits; Open Decisions emptied; language
tour + generated error-code index; prebuilt binaries; tag `v1.0.0`.
**Exit:** see the plan — ends with a stranger shipping a tool from the
README in an afternoon.

## Deferred past v1.0 (owner can promote)

Async/await (tasks + channels are the v1 answer), user macros, Mutex/
shared-state concurrency, networking std modules, self-hosting, debugger
source maps (DAP), comptime (closed by S26 unless reopened), sized
integer menu beyond `Int`/`Byte` (revisit if M7 FFI demands it).
