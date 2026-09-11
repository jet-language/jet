# Universal language and Core product parity

This page defines the evidence boundary for broad language and product claims. It
records durable claim rules and rationale; Tower is the planning and status home.

Vocabulary: [Jet vocabulary](../../spec/vocabulary.md).

## Claim boundary

Jet may claim language or ecosystem parity only when a user can build, debug,
profile, test, package, deploy, and operate representative production systems
without a hidden mock, transcript, fixture, schema-only path, unsupported
lowering, silent omission, or unverified platform claim.

An API name is not proof. A parser path is not execution. A generated schema is
not a package manager. A fake DOM is not browser proof. A three-frame transcript
is not a game runtime. A validated TIR followed by AST emission is not R12. A
fallback counts only when its observable contract is the same, its limitations
are explicit, and its acceptance evidence proves them.

## Evidence classes

Every product claim uses exactly one evidence class:

1. **Reserved** — syntax, type, command, or schema is recognized but not run.
2. **Facade** — a public shape exists over a mock, deterministic transcript,
   placeholder, silent omission, or non-production backend.
3. **Partial** — real execution exists for a named subset; unsupported cases fail
   loudly with a Jet diagnostic.
4. **Implemented** — complete documented behavior works on one supported path.
5. **Proven** — implemented behavior passes the cross-tier, cross-platform,
   live, hostile, scale, recovery, and dogfood lanes applicable to its claim.

Only **Proven** supports a broad product claim. Partial paths remain useful when
they are labeled truthfully; they do not support a broader claim.

## Product laws

1. **One executable meaning.** Parser, sema, TIR, AOT, JIT, comptime, REPL,
   web, debugger, editor, and analysis tools consume the same semantic facts.
2. **Exhaustive lowering.** No wildcard arm may silently omit a checked node or
   synthesize `undefined`, `()`, empty text, or a success-shaped placeholder.
3. **Core is real software.** Typed signatures and emitted templates do not
   count without a real implementation, failure semantics, standards tests, and
   target conformance.
4. **No fake closure.** Fake DOMs, fake clocks, fake registries, fake QEMU,
   in-memory transports, and deterministic transcripts support tests; live
   acceptance evidence supports product claims.
5. **Beginner magic and expert control share one mechanism.** Defaults select
   safe policy; expert flags expose target, scheduler, authority, cache, device,
   generated code, and proof facts without changing meaning.
6. **Silent degradation is a defect.** Unsupported behavior fails before
   execution with a Jet diagnostic and the smallest valid fix.
7. **Platform claims are literal.** Linux evidence says Linux. Desktop means
   Linux, macOS, and Windows. Mobile means iOS and Android. Web means supported
   browser engines. Each tier has a published matrix.
8. **Performance claims include correctness.** Benchmarks run equivalent work,
   publish source and environment, enforce memory/startup/tail-latency budgets,
   and never excuse semantic gaps.
9. **Foreign bridges are accelerators, not facades.** Every bridge exposes
   provenance, safety, ownership, replacement state, and live conformance.
10. **Proof is reproducible.** A clean machine can rerun the exact proof from
    source and obtain the recorded artifact, transcript, metrics, and report.

## Competitive contract

Jet does not copy surface syntax. It adopts strong semantic and product
properties through one coherent Jet mechanism.

| Domain | Reference bar | Evidence required for the Jet claim |
| --- | --- | --- |
| Safety and systems | Rust ownership, Zig comptime/build control, Swift ergonomics | ownership adversary corpus; no safe-source `unsafe`; verified FFI; supported target portfolio; compile/startup/runtime budgets |
| Concurrency and services | Go task/network ergonomics; Erlang/Elixir supervision and upgrade discipline | one task scheduler; nonblocking I/O; cancellation/deadline proof; supervised service tree; cluster, chaos, and rolling-generation tests |
| Interactive work | Python/Julia/Jupyter REPL and notebook loop | structural multiline REPL; persistent search; rich display; Jupyter protocol; interrupt/debug/profile; AOT-equivalent Core semantics |
| Web applications | React server components; Svelte runes; SvelteKit/Next routing, data, forms, SSR; Vite HMR | one typed application graph; fine-grained dependency compilation; SSR/SSG/streaming; hydration; server actions/forms; accessibility; real-browser HMR and deployment evidence |
| Native/mobile UI | SwiftUI and Jetpack Compose state, previews, accessibility, adaptive layout | one renderer/component model; desktop/mobile backends; previews; hot reload; navigation/lifecycle/restoration; accessibility and packaging evidence |
| Data/ML/compute | NumPy broadcasting/ufunc/linalg; dataframe/lazy-query systems; accelerator graphs and explicit kernels | one typed compute model beneath executable TIR; ndarray/table schemas; Arrow/Parquet; lazy fusion; autodiff; CPU/GPU parity; device ownership; multi-device evidence |
| Tooling | rust-analyzer incremental semantics; Go tool coherence; .NET diagnostics | shared incremental query service; LSP/DAP; formatter/test/doc/profile/trace; stable JSON; one diagnostic model; project-scale latency evidence |
| Packages/builds | Nix hermetic store/build model plus Cargo/uv/pnpm ergonomics | hermetic package/build/store behavior with independent live evidence |
| Games/media | mature engine asset/render/audio/input/editor/replay workflows | real renderer/audio/input; asset import/cook/hot reload; ECS; replay/networking; editor; packaged game on supported platforms |
| Ecosystem reach | C/C++/Rust/Swift/JS/Python/JVM/.NET/R/Julia integration | generated typed bindings; ownership/error/async mapping; real upstream suites; in-situ native replacement evidence |

These reference bars are rationale, not a work queue. The claim boundary above
is the authority for labeling evidence.

## Source of truth

Normative syntax decisions live in
[`docs/spec/syntax-decisions.md`](../../spec/syntax-decisions.md). Architecture
laws live in [`docs/spec/architecture.md`](../../spec/architecture.md), and
release compatibility lives in [`docs/spec/release-policy.md`](../../spec/release-policy.md).
Executable capability truth lives in compiler and runtime source, tests, and
examples. This page does not duplicate their implementation inventories or
create a second acceptance ledger.
