# Architecture

## Pipeline

```
 Jet source (.jet)
        │
        ▼
 jet-lexer ──► tokens (every token has a byte Span)
        │
        ▼
jet-parser ──► AST                      ┐
        │                               │  the FRONT END owns all
        ▼                               │  semantics and every
  jet-sema ──► checked AST              │  user-facing diagnostic
        │      (M2: + ownership check)  ┘
        ▼
jet-codegen ──► boring Rust source
        │
        ▼
     rustc  ──► native binary      (verifier + optimizer; never
                                    speaks to users — see R5)
```

### Typed IR (TIR) — the codegen seam

Codegen does not read the AST plus side registries; it lowers the checked AST to a
**typed IR** (`crates/jet-codegen/src/Codegen/TIR/`) that carries only sema-approved facts, then emits
Rust from the TIR with **zero inference** (every type/convention/mangle/overflow decision
is resolved at lowering — R1/I3). The TIR is the **only** codegen seam (R7) for every emitted
body: free functions, methods, trait methods, `#Test` block bodies, and error-conversion
`impl Old -> New` bodies all lower through it. A per-surface gate (`tir_covers*`) decides
coverage, and a construct **outside** the TIR subset is an **internal compiler error** (R5 ICE),
never an AST fallback or a miscompile. The legacy AST codegen path (`emit_expr`/`emit_stmt`/
`emit_stmts`/`emit_lambda`) was deleted (c109) once a whole-test-suite byte-parity check proved
every reachable body routes through the TIR; no legacy emit machinery remains. Constructs the
gate excludes are provably sema-unreachable (generic-struct methods E0311, bare `?? return`
in a value fn E0405, nested `T??`, a bare `Variant(n) ->` arm) — they never reach codegen.
(Type *definitions* — `emit_struct`/`emit_enum`/`emit_trait_def` — are structural, not bodies,
and emit directly; only executable bodies go through the TIR.)

## Compiler crate map

D-COMPILERSEAMS1/2 split the compiler into workspace seam crates. The root
`jet` crate is now a thin facade and binary host over these internal APIs.

| Crate | Job | May emit diagnostics? |
|-------|-----|-----------------------|
| `jet-foundation` | shared leaf types: `Syntax`, `Diagnostics`, `AST`, `Span`, `Generics`, `JitBackend` | renders them |
| `jet-lexer` | text to tokens | yes (E00xx) |
| `jet-parser` | tokens to AST, formatter | yes (E00xx) |
| `jet-comptime` | comptime values and interpreter support | no user-facing surface by itself |
| `jet-sema` | all semantic checks, collects all front-end diagnostics | yes (E01xx+) |
| `jet-codegen` | checked program to Rust text; TIR is internal here | **never** |
| `jet-pkg-model` | shared read-only package/config data model: `pkg.jet` manifest parsing, lock, hangar store listing, ref classification, FFI bridge construction, inline script deps, plus the pure effect-budget/lint-policy computation over that data (no network/provider/shell) | package/FFI diagnostics |
| `jetpack` | package manager engine: provider/network/shell realization, JetOS, CLI — depends on `jet-pkg-model` for the data it reads/writes | package/JetOS diagnostics |
| `jetos` | `jetos` binary front door for OS workflows; still dispatches into `jetpack`'s `os` verb (JetOS realization hasn't split out of `jetpack::JetOS` yet — card #367 slice 4) | package/JetOS diagnostics (via `jetpack`) |
| `jet-driver` | front-end orchestration and compile outputs; depends on `jet-pkg-model` (never `jetpack`'s engine) for manifest/lock/FFI preparation | front-end diagnostics only |
| `jet-queries` | std-only demand cache for incremental inputs and derived query values | no |
| `jet-semindex` | stable semantic index over checked programs for tooling | no new diagnostics |
| `jet-impact` | blast-radius reports over `jet-semindex` | no |
| `jet-rt` | runtime helpers shared by generated code and JIT/dev paths | no |
| `jet-jit` | dev/JIT execution tier over codegen/TIR facts | internal fallback only |
| `jet-net` | runtime/comptime fetch helper with TLS diagnostics | yes, for fetch failures |

I6 is machine-checked by `tests/truthfulness.rs`: the root compiler and named
compiler seams may use only workspace path dependencies. Runtime/tool siblings
such as `jet-jit` and `jet-net` are separate workspace members with their own
owner-approved dependency posture; that does not permit an external dependency
to leak into a checked compiler manifest.

`tests/workspace_crates.rs` pins the current path-dependency direction. Compiler
front-end crates may not grow back-edges into driver/codegen clients. Tooling and
runtime crates stay outside the compiler seam unless their dependency row is
changed here and in the test. Jetpack/JetOS live in `crates/jetpack`; `jetos`
(its own crate/binary, card #367 slice 2) is a thin front door still
dispatching into `jetpack::JetOS`. The shared manifest/lock/store-listing/
FFI-bridge/script-dep data model, plus the pure effect-budget/lint-policy
policy computation (card #367 slice 3), lives in `crates/jet-pkg-model`
(D-PRODUCT-SPLIT1=C), which `jetpack` re-exports under the legacy module
names so its own internal call sites are unchanged. `jet-driver` depends on
`jet-pkg-model` directly — never on `jetpack` — so the compiler's module
loader never needs Jetpack's provider/network/shell engine to resolve
`use <pkg>` imports.

The root `jet` package (`Source/`) routes the same way: it no longer carries a
blanket `pub use jetpack as Jetpack` re-export (card #367 slice 3). Read-only
model needs (`PackageManifest`, `Manifest`, `ScriptDeps`, `Lock`, `CBind`,
`CFFI`, `FFI`, `EffectBudget`, `LintPolicy`, and the hangar-listing half of
`Store` as `PkgStore`) come from `jet-driver`'s `jet-pkg-model` re-export, the
same seam the compiler itself uses. Genuine `jetpack`-engine calls that
haven't split out yet (`Overlay`, `WorkspaceFile`, `ModuleEval`,
`WorkspaceLock`, `Discovery`, `JetPin`, `ScriptLock`) stay direct `jetpack::…`
references in a small, explicit set of files
(`tests/workspace_crates.rs::direct_jetpack_imports_stay_behind_known_boundaries`
pins exactly which). `Source/Canvas`'s package-manifest scans route through
the shared model the same way; its workspace/env-plan scans
(`WorkspaceFile`/`ModuleEval`) stay on the `jetpack` engine until card #367
slice 4 splits ModuleEval's plan model out of env-runtime/JetOS realization.
The root binary still owns native build execution: `Source/CmdCompile.rs`
invokes rustc, classifies linker/tool failures, renders the I2 ICE banner, and
links any prepared FFI artifact. Do not move that responsibility into a seam
crate in documentation until the code moves with it.

### Adding an FFI bridge

Foreign dependencies stay behind the existing runtime boundary; they never
become dependencies of the compiler workspace crates.

1. Start from a ratified interop surface and dependency approval. Do not add a
   new user spelling, ABI policy, or external stdlib dependency from code alone.
2. Parse and type-check the Jet declaration in the normal front end. Unsupported
   declaration types, unsafe signatures, and invalid boundary conversions need
   Jet diagnostics before codegen (I2/I3/I4). Tool and library availability is
   not a source-level fact; classify it later at bridge build/native link time.
3. Extend `crates/jet-pkg-model/src/FFI.rs` (or `CFFI.rs` for C) to prepare the
   bridge (re-exported at `crates/jetpack::FFI`/`::CFFI` and `crates/jet-driver::FFI`).
   The Rust bridge is a generated, content-addressed crate under Jet's cache: its
   generated `Cargo.toml` owns foreign dependencies, `cargo build` produces an
   rlib, and `FfiLink` records the crate name, rlib, and dependency directory.
   Do not put the dependency in the root or compiler-seam `Cargo.toml` files.
4. Thread the prepared `FfiLink` through `crates/jet-driver/src/Driver/mod.rs` and
   `CompileOutput`. `Source/CmdCompile.rs::build` is the real native link edge: it
   passes `--extern`/`-L dependency` to rustc and keeps missing-tool/library
   failures out of the ICE path by reporting build/link diagnostics there.
5. Keep generated wrappers minimal and audited. Safe Jet cannot acquire an
   ungated unsafe operation through a bridge; boundary ownership, error, and
   layout conversions must be explicit.
6. Add focused tests for front-end rejection, generated wrapper/link arguments,
   cache reuse, and a real end-to-end bridge call. Add the diagnostic snapshot
   and docs for every new error, then run the project verification workflow.

### Incremental Compiler Service

D-LSP1 makes editor tooling a client of the front end, not a second checker.
`crates/jet-queries` is a std-only demand cache for file inputs and derived
queries. The LSP stores open-buffer text as query inputs, memoizes lexing,
diagnostics, checked bundles, and fix data through that cache, and invalidates
only dependencies whose input revision changed. D-LSP2 requires every
advertised LSP capability to have named coverage in `tests/lsp.rs`; the server
must not advertise speculative features.

## Rules

- **R1 — Codegen is dumb.** No checks, no decisions, no "see if rustc
  accepts it". If codegen needs to know something, sema should have
  established it.

  **I1 amendment (D-LL1, ratified 2026-06-16, E2-M13).** I1 originally read
  *"no `unsafe` in the language or generated code, ever (v1)."* The expert
  low-level tier (S58) amends it: generated `unsafe` appears **only** inside
  user-written gated regions — an `#Unsafe("reason") { … }` block (or bare
  `#Unsafe { … }`, which emits lint L3101) or an `#Unsafe fn` contract, both
  unlocked by `use core.mem` — plus vetted std/mem internals. Ordinary,
  memory-safe Jet still emits **zero** `unsafe`; the boundary is enforced by
  sema (E3101/E3102/E3103) and tested in `tests/golden.rs` (every example but
  the audited `48_lowlevel` must contain no `unsafe`, and even there every
  `unsafe` must be a gated `unsafe {`/`unsafe fn` form). Codegen stays dumb:
  it lowers an already-checked `#Unsafe` region straight to a Rust `unsafe`
  region and makes no safety decision of its own.
- **R2 — Sema is the gatekeeper.** Any program that passes sema must
  produce Rust that compiles. New language features land as: spec →
  parser → sema checks → codegen → tests, in that order.
- **R3 — Single surface.** User-typeable strings live in
  `crates/jet-foundation/src/Syntax.rs` only. Renaming a keyword starts there;
  parser/formatter tests, generated grammars, snapshots, and docs must move with
  it.
- **R4 — Spans everywhere.** Any AST node an error might point at carries
  its span. Adding a node without a span is a review-blocker.
- **R5 — ICE policy.** rustc failing on generated code prints the
  internal-compiler-error banner owned by `Source/CmdCompile.rs`, exits 101,
  and is treated as a P0 bug. rustc's stderr is shown only inside that banner.
  Missing rustc, linker, or C library is a tool/user diagnostic, not an ICE.
- **R6 — Name mangling.** User identifiers are emitted as `user_<name>`
  (`main` excepted) so user code can never collide with Rust keywords,
  macros, or std items.
- **R7 — Backend is swappable.** Rust emission stays in
  `crates/jet-codegen/src/`; native rustc invocation and ICE classification
  stay in `Source/CmdCompile.rs`. The lexer, parser, and sema crates do not
  depend on either responsibility. Another backend replaces the codegen and
  binary-build edges without changing the front end.
- **R8 — Small, self-contained binaries.** The root binary build path in
  `Source/CmdCompile.rs` calls `rustc` directly with `strip=symbols` and thin LTO,
  so the linker keeps only
  what the program uses ("only link what's needed"). Output is one
  self-contained native binary. Floor: Rust's std links a baseline
  (low-hundreds-of-KB), accepted as the cost of a beginner-friendly
  std-backed runtime — we do NOT pursue `no_std` in v1 (it would remove
  the conveniences priority #2 depends on). A size-minimal profile
  (`opt-level="z"`, possibly `panic=abort`) is decision S15, exposed
  later as `jet build --small`; the default leans toward speed.
- **R9 — A file is a complete program.** `jet run foo.jet` compiles and
  runs a single file with no manifest, no project folder, no config.
  The compiler invokes `rustc` on one generated `.rs` file — it never
  creates or requires a Cargo project for user code. Agents must not add
  a mandatory project structure, lockfile, or manifest for users; any
  future multi-file/package story is opt-in and post-v1 (see roadmap).
- **R10 — Std is pay-for-what-you-call.** M10 standard-library modules are
  compiler-known namespaces, but importing them is free. Sema records the
  core helpers that a checked program can call, and codegen emits only those
  helper templates. A program that imports every core module but calls none
  should stay in hello-world size territory.
- **R11 — Generated code re-enters the front end.** Every build-time
  code-generation step — a derive body, a comptime splice, any future
  metaprogram — emits a **typed source fragment** that re-enters
  lexer→parser→sema exactly like hand-written code. **No generation path may
  inject pre-parsed AST past the sema gatekeeper** (R2). The guarantee that
  buys: generated code is trustworthy-by-construction (R1 codegen-dumb, R2
  sema-gatekeeper, R5/I2 rustc-never-speaks all keep holding through
  generation), and any error in generated output surfaces as a **real sema
  diagnostic pinned to the user's trigger site** — the struct, field, or
  derive marker that caused it — with the generated fragment shown only as
  optional context, never as raw rustc output. The shipped `@[Codable]` derive
  already works this way; it is the required shape for all future derives and
  build-time steps (S56 user derives, comptime). (D-CTCODEGEN1=A, ratified
  2026-06-25; pairs with D-METADERIVE1=A, which makes a user derive's output a
  source fragment for exactly this reason.)
- **R12 — Two consumers, one executable IR.** Every executable TIR variant is
  owned by two consumers: the Rust emitter for AOT binaries and the dev/JIT
  lowerer for `jet dev`. The Rust emitter and the JIT lowerer must both handle
  executable TIR exhaustively, wildcard-free, either by real lowering or by a
  named internal unsupported reason that falls through transparently to the
  next dev tier. A feature PR is incomplete unless its example/golden proves
  the AOT path and `tests/dev.rs::dev_default_matches_compiled_binary` proves
  default `jet dev` has the same stdout, stderr, exit code, diagnostics,
  panics, and side effects. Native JIT coverage is a performance tier; semantic
  parity is mandatory.

## Exit codes (stable contract)

The `jet` driver returns one of a small, stable set of exit codes so
scripts and CI can branch on the outcome without parsing output. This
table is pinned by the `tests/cli/` transcripts and is part of the public
contract — codes are never repurposed.

| Code | Meaning | Who produces it |
|------|---------|-----------------|
| `0`   | Success. (Includes the no-args greeting — orientation, not an error.) | driver |
| `1`   | User error: a reported diagnostic, a failed `check`, a missing file, a failed `test`. | driver |
| `2`   | Usage error: unknown subcommand (E2101), unknown/ambiguous flag (E2102), or a missing required argument. | driver |
| `70`  | A built program panicked at runtime (`panic`/`require`, S36). Forwarded through by `jet run`. | the user's program |
| `101` | Internal compiler error (I2/R5): rustc rejected generated code. A P0 bug, never the user's fault. | driver |

Presentation is TTY-aware (E2-M3): color and progress appear only when
the relevant stream is a terminal. `NO_COLOR` and `--color=never` force
plain output; `FORCE_COLOR` and `--color=always` force color; `--color=auto`
(the default) defers to TTY detection. Piped or CI output is always plain,
deterministic, ANSI-free bytes — scripts never parse escape sequences.

## Testing strategy

1. **diagnostic snapshots** (tests/diagnostic_snapshots.rs): every
   diagnostic's exact text, pinned. The error messages are the product;
   treat snapshot diffs like UI diffs.
2. **golden examples** (tests/golden.rs): examples/ must front-end-pass,
   contain no `unsafe`, and — when rustc is present — build and print
   exactly examples/features/expected/*.out.
3. rustc-as-verifier: golden tests assert rustc accepts generated code,
   so a sema soundness hole becomes a loud test failure, not a shipped bug.

## Why transpile to Rust (recorded rationale)

The front end is hand-built either way; only the backend was a choice.
Rust gives: a soundness verifier for our ownership checker (critical when
agents write the compiler), LLVM optimization, cross-compilation, and std
— for free. Known costs, accepted: compile times stack on rustc's;
debuggers show generated Rust until M6+ tooling. Precedent: cfront, Nim,
TypeScript, Gleam.
