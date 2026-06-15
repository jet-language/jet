# Roadmap

Each milestone is done when its exit criteria pass as tests. Examples are
the executable spec: a milestone ships with new examples/ programs and new
tests/ui fixtures, all green.

> **Naming canon (owner, 2026-06-15):** **jet** is the language + compiler;
> **jetpack** is the package-manager engine/binary (binary packages +
> environments); **jetos** is the operating system (working title), built on
> jetpack. The near-term **jetpack & jetos** track — Phase 1 is a
> Nix-`shell`/`devenv`-class `jetpack run github:...` temporary environment;
> Phase 2 is the jetos distro/ISO — has its own consolidated plan and remaining
> owner decision gates in
> **docs/plans/jetpack-jetos/README.md**. JetOS parity baseline:
> `/home/nate/nixos` / HalcyonOmega NixOS; anything that setup supports must be
> expressible in JetOS unless the owner approves an explicit exception.

**M3 onward each have a full implementation plan in docs/plans/** (one
file per milestone: surface, grammar, sema rules, lowering, diagnostics,
tests, out-of-scope). Implementing agents follow docs/plans/README.md.
Plans are gated on the decision ballots in docs/spec/decision-ballots.md —
a milestone may not start until its ballot group is ratified in docs/spec/syntax-decisions.md.

**Owner direction (2026-06-11, amended 2026-06-12):** the v1.x horizon is
a complete language — data types, errors, collections, closures,
generics/traits, std library, package manager, real LSP — good enough
that experts rewriting small Rust/Go/C tools would *choose* Jet.
Concurrency was deferred past v1 by S53 and is now implemented as E2-M1
(verified 2026-06-14). Formerly "deferred indefinitely" items (generics,
package manager) are promoted onto the roadmap below. Philosophy ranks are
unchanged; single-file `jet run` stays ceremony-free forever (R9).

## M0 — Walking skeleton  *(done; verified 2026-06-11)*

Hello world end-to-end: jet → parse → sema → emit Rust → rustc → run.
Diagnostics framework, ui snapshot harness, golden harness, ICE policy.
**Exit:** `cargo test` green; `jet run examples/features/01_hello.jet` prints. ✓

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
in sema, E02xx diagnostics written to docs/spec/diagnostics.md voice rules. Teaching errors
for foreign `read`/`write` (E0017/E0018). References cannot be stored or
returned in tier 1 — therefore no lifetime syntax exists.
**Exit:** an example that *fails* ownership exists for every E02xx code,
each with a `.fixed.jet` companion that compiles (`tests/ui_fixes.rs`);
lint snapshots in `tests/ui_lint/`; golden tests prove rustc never rejects
what sema passes (the verifier earning its keep). ✓

## M3 — Data  *(plan: docs/plans/epoch-1/m03-data.md; ballots: Group 2 ✅)* ✓ 2026-06-11

Structs, enums (sum types), `switch` exhaustiveness for enums ("you
forgot the `Circle` case"), `==` pattern tests, Option (`T?`, no null,
ever), methods (S27: `self`, `c.area()`, definable inside the type or
in `impl Type { }` blocks), invisible auto-boxing for recursive types.
No inheritance, ever (non-goal). Traits/interfaces (S28) are explicitly
out of M3 — they land in M9.
**Exit:** examples `10_structs`–`13_recursive_enum`; every E03xx/L0301
and E0020–E0023 has a ui snapshot; exhaustiveness errors list missing
cases verbatim. ✓

## M4 — Errors as values  *(plan: docs/plans/epoch-1/m04-errors.md; done 2026-06-11)*

`T ? E` fallible returns, `ok`/`err`, propagation `?` (S7), `??`
fallback, `panic`/`require` for bugs with a friendly runtime report.
No exceptions, no null, no silently ignored failures.
**Exit:** a file-parsing example showing the happy path staying clean;
the runtime report format pinned by a golden stderr test. ✓

## M5 — Collections & one string story  *(plan: docs/plans/epoch-1/m05-collections.md; ballots: Group 4)* ✓ 2026-06-12

`[T]`, `[K, V]` (bridging Rust's Vec/BTreeMap internally),
literals, iteration, indexing with friendly runtime reports, copy-based
slicing without exposing references, `Char`, a real String API. Exactly
one string type.
**Exit:** wordcount example; out-of-bounds and iterator-invalidation
mistakes produce great errors, not Rust concepts. ✓

## M6 — Tooling I  *(plan: docs/plans/epoch-1/m06-tooling.md; ballots: Group 5; four phases)* ✓ 2026-06-12

`jet fmt` (one true style, zero config), `jet test` (`test "name" { }`
blocks), `jet new`. Multi-file imports (S16: `import "path" as alias;`)
and cross-file visibility enforcement (S18). A `jet build --small`
profile (S15). LSP **v0**: diagnostics + S14 autocorrect quick-fixes +
formatting, with a minimal VS Code extension. Single binary, no config
files (philosophy: minimal configuration).
**Exit:** fmt is idempotent on all examples; autocorrect turns a pasted
C-style snippet into canonical Jet; `--small` produces a measurably
smaller binary than the default; a new project runs in two commands.

## M7 — Rust FFI (interop tier)  *(plan: docs/plans/epoch-1/m07-ffi.md; done 2026-06-12)*

`extern rust` blocks for calling vetted Rust functions across an
owned/copied boundary (no borrowed returns), version-pinned, built via a
hidden cached cargo bridge — the user's directory never grows a cargo
project. This is C2's resolution: interop without importing Rust's type
system.
**Exit:** an example calling a real Rust crate function. ✓

## M8 — Functions as values  *(plan: docs/plans/epoch-1/m08-closures.md; done 2026-06-12)* ✓

Lambdas, function types, closures whose captures obey the M2 ownership
rules (no Fn/FnMut/FnOnce surfaced), and the closure-powered collection
methods: `map`/`filter`/`each`/`find`/`sort_by`/`reduce`.
**Exit:** a pipeline example; capture-ownership fixtures both failing
and fixed; rustc-as-verifier battery over Fn-inference cases. ✓

## M9 — Generics & traits  *(plan: docs/plans/epoch-1/m09-generics-traits.md)*

`fn f<T: Trait>`, generic structs/enums, `trait` + in-type `impl` or
`impl Type: Trait`, trait-as-type with invisible boxing/dynamic dispatch,
built-in traits with S55 hybrid derive (auto `Printable`/`Equatable`;
explicit `derive Comparable;` / `derive Serialize;`).
Monomorphized by rustc, proven by sema (R2). Comptime (S26, ratified) is
deliberately separate: traits own all polymorphism; comptime computes
values only and lands in M9.5.
**Exit:** shapes-with-traits example; generic container example; an
instantiation soundness matrix test. ✓

## M9.5 — Comptime v1 (CTFE)  *(plan: docs/plans/epoch-1/m095-comptime.md; resolves S26 layer 1, S57)*

`comptime x = expr;` — evaluate a pure, deterministic Jet subset at
compile time (sema tree-walking interpreter; no FFI/IO/time/random),
fuel-limited with call-trace diagnostics; `panic` at comptime is a
user-authored compile error; `embed_file("path")` bakes a file into the
binary; results lower to plain Rust constant data (codegen stays dumb).
One law (S26): comptime never creates, parameterizes, or selects a type,
and never affects dispatch.
**Exit:** lookup-table and embed_file examples; comptime-panic ui
snapshots; the **differential battery** green in CI (every
comptime-evaluable fixture also runs at runtime and must agree
bit-for-bit — divergence is a P0 miscompile).

## M10 — Standard library  *(plan: docs/plans/epoch-1/m10-stdlib.md; Group 7 ✅)* ✓ 2026-06-13

`import std.<module>`: fs, io, env, process, math, random, time, json —
exact v1 APIs frozen in the plan; every fallible call returns `T ? E`.
`U8` byte buffers and byte/string conversions. Enough batteries for real
CLI tools. User-facing reference: **docs/reference/stdlib.md**.
**Exit:** file-transform, JSON, and mini-CLI examples with golden tests. ✓

## M12 — Package manager  *(plan: docs/plans/epoch-1/m12-packages.md; ✅ ratified 2026-06-13; two phases)*

Opt-in `jet.toml` + content-hashed graph `jet.lock` (spec in
docs/plans/epoch-1/m12-packages.md). M12.1: path + git deps, moving
branch/`@latest` selectors, `jet add`/`fetch`/`update`, Nix-style store
(`~/.jet/store/<name>-<version>-<fingerprint>/`), optional FFI metadata,
optional `.jet/` source root, no install-time code execution. M12.2:
git-index registry, semver ranges + resolver, `jet publish`/`vendor`/`audit`.
Single files never need any of it (R9).
**M12.1 verified 2026-06-13.** M12.2 is next.
**Exit:** M12.1 store+lock battery + new→add→run ✓; M12.2 registry and
resolver snapshots per plan.

## M13 — LSP v2  *(plan: docs/plans/epoch-1/m13-lsp.md)* ✓ 2026-06-13

The real language server: completion (incl. switch-arm snippets for
enums), hover with types + ownership + doc comments, go-to-definition,
references, rename, structured quick-fixes (shared with a new CLI
`--fix`), semantic tokens, inlay hints. Crash-proof, latency-budgeted,
fed by unsaved buffers. Tree-sitter + TextMate grammars.
**Exit:** scripted LSP transcript tests per capability; a bench harness
under budget in CI.
**M13 verified 2026-06-13.**

## M14 — v1.0  *(plan: docs/plans/epoch-1/m14-v1.md)* ✓ 2026-06-14

The proof: three showcase tools (grep-lite, JSON formatter, wordfreq)
benchmarked at ≤1.5× their Rust references. Diagnostics,
soundness-fuzz, and performance audits; Open Decisions emptied; language
tour + generated error-code index; prebuilt binaries; tag `v1.0.0`.
**Exit:** see the plan — ends with a stranger shipping a tool from the
README in an afternoon.

## Deferred past v1.0 (owner can promote)

**M11 / E2-M1 — Concurrency** (S53 ratified deferred to v2, then promoted):
tasks + channels (`tasks.spawn`, `Task<T>.join`, `Channel<T>`), no shared
mutable state. **E2-M1 verified 2026-06-14.**
Async/await, user macros (rejected forever in token/AST form by S26 —
the sanctioned path is S56), Mutex/shared-state concurrency, networking
std modules, self-hosting, debugger source maps (DAP), comptime layer 3:
typed reflection / user-defined derives (S56).

## Committed future additions (owner direction 2026-06-12 — need plans)

Promoted from "maybe" to "needed, post-v1, in roughly this order."
Each requires a docs/plans/ file and (where marked) a decision ballot
before work starts. None of these may compromise the v1 milestones.

1. **Tier-2 references** — stored/returned references behind explicit
   syntax: the full Rust-class borrow checker surfaced gradually. The
   single biggest unlock for Rust-territory programs (zero-copy
   parsing, graphs, arenas). Needs a plan file and ballots.
2. **Generics v1.5** — associated types and default method bodies, the
   minimum for serious library-building. Trait inheritance/blanket
   impls re-evaluated on evidence.
3. **Error conversion for `?`** — propagate across differing error
   types (a `From`-equivalent, beginner-safe spelling TBD). The "same
   error type only" v1 rule does not survive multi-module programs.
4. **Streaming I/O** — file handles/readers; whole-file reads stop
   scaling exactly when programs get real.
5. **Networking std modules** — blocking sockets + HTTP client over
   tasks/channels (no async). This is the Go-territory stdlib buildout.
6. **Expert low-level tier** (S58) — `std/mem`-style gated access to
   allocators, layout, volatile, raw memory; never in onboarding.
7. **C FFI** (S59 ratified deferred to v2) — `extern c` import and a
   Jet-export story; the gateway to the non-Rust ecosystem and to
   embedded toolchains. **Manual `extern c` blocks only in Epoch 2**
   (E2-M14). C-header auto-binding (`jet bind` / magic `import c`) is
   deferred post–Epoch 2: docs/plans/post-epoch-2/c-header-bindings.md.
8. **Freestanding profile** — `no_std`-class output for embedded/
   kernels (long-horizon target per docs/spec/philosophy.md owner direction).
9. **Pure-function marking & evaluation** (S60 ratified) — `pure fn`
   and `jet eval --pure` over marked-pure functions; layer 3 post-v1
   (docs/plans/epoch-1/m12-packages.md § out of scope;
   docs/plans/jetpack-jetos/README.md).
10. **Rapid-prototype execution mode** (owner direction 2026-06-12) —
    compile speed is a product priority: instant iteration while
    prototyping, full compilation when the product is ready. Components
    required (each needs a docs/plans/ file before work starts):
    - **`jet dev` watch server** — a long-running process in the
      TypeScript `tsc --watch`/tsserver mold: watches the import graph,
      re-checks incrementally on save, re-runs instantly, streams
      diagnostics. **Shares its foundation with the M13 LSP server**
      (SourceProvider overlay, file-granular incremental front end,
      crash policy) — build that machinery once, host both; M13 carries
      a design note to this effect.
    - **Execution phase 1: interpreter.** The M9.5 comptime
      tree-walking interpreter extended to whole programs. Its
      differential battery (bit-for-bit agreement with compiled output,
      P0 on divergence) extends to dev mode — same semantics guarantee,
      already CI-enforced.
    - **Execution phase 2: JIT.** Optional native-speed dev execution
      (e.g. Cranelift-backed); requires owner crate approval (I6) and
      its own plan; never replaces rustc for release builds.
    - **Boundaries:** FFI calls and tasks need real native code — dev
      mode either bridges to prebuilt artifacts or says plainly "this
      program needs a full build." Interpreted/JIT performance is for
      iteration, never benchmarks; `jet build` (rustc, full
      optimization) remains the only release path.
11. **Interactive REPL** (`jet repl`, E2-M18) — line-based exploration
    for beginners: persistent session bindings, interpreter-backed eval,
    transcript-tested UX. Separate from `jet dev` (file watch). Blocked on
    owner decisions D-REPL1…D-REPL21; plan:
    docs/plans/epoch-2/m18-repl.md. Depends on E2-M4 interpreter. Web
    playground remains a separate gate (D-REPL2/D-REPL19).

**Reconciled (manifest + architecture, amended 2026-06-15):**
docs/plans/epoch-1/m12-packages.md is the single source of truth for v1
Jet source-library package management (`jet add`, `jet.toml`, `jet.lock`).
The binary/environment package-manager and OS track lives in
docs/plans/jetpack-jetos/README.md: Phase 1 is independent
`jetpack run/build/list/clean/add/remove`; Phase 2 is jetos on top of jetpack,
including installable ISOs.
