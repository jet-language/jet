# toolseams

## Ratified

- **D-FRONTENDAPI1=A** — `core.compiler` is the stable read-only lexer/parser/check/semantic-index/source-map value API plus a CLI JSON mirror; internal compiler crates stay private and “no AST mutation enters compilation.” — `docs/spec/syntax-decisions.md:4730-4739`; `docs/spec/architecture.md:168-181`
- **D-BUILDQUERY1=A** — graph inspection is `jet inspect graph`, `jet inspect query build`, and `jet inspect explain-build <target/file/action>`, with LSP sharing the graph/provenance model. — `docs/spec/syntax-decisions.md:4716-4724`
- **D-BUILD-UNITS1=A** — hidden compilation uses two crates per unit: an interface crate with types/traits/method tables/generic bodies/declarations and a body crate with ordinary function bodies; signature/fingerprint mismatch stops linking. — `tower` (`D-BUILD-UNITS1` outcome A)
- **D-BUILD-NOCHANGE1=A** — memoized checking is valid only when exact source bytes and every imported interface, policy, target/profile, compiler, generated, and external input are unchanged; typed diagnostics replay, with `--no-cache`/`--verify` exits. — `tower` (`D-BUILD-NOCHANGE1` outcome A)
- **D-BUILD-SESSION1=A** — attach build/run/test only to an existing `jet dev --serve` process; there is no daemon, and `--session=auto|off|required` selects reuse or fallback. — `tower` (`D-BUILD-SESSION1` outcome A)
- **D-BUILD-STORE1=E** — one machine-wide store uses `min(20 GiB, 10%)`, a 2 GiB free-space reserve, pre-write admission, live leases, and host override; pruning is explicit. — `docs/spec/architecture.md:124-138`; `tower` (`D-BUILD-STORE1` outcome E)
- **D-BUILD-PREBUILT1=D** — federated prebuilt objects are signed by Jet/Core or registry-authorized builders under separate roots; every hit verifies the full compiler/rustc/target/ABI/linker/LTO/panic/features/profile/dependency identity. — `tower` (`D-BUILD-PREBUILT1` outcome D)
- **D-BUILDBENCH1=E** — build claims require a strict wall-clock win against ordinary and expert Cargo, paired randomized samples, a 95% upper ratio below 1.00, independent resource ceilings, and real programs as the gate. — `tower` (`D-BUILDBENCH1` outcome E)
- **D-BUILD-UI1=A** — the accepted build surface is a terminal live board, one-line receipt, standard trace, and self-contained `explain-build --html`, all rendered from one model. — `tower` (`D-BUILD-UI1` outcome A)
- **D-BUILD-DEFAULT1=B** — `jet run`/`jet dev` use the fast profile; `jet build` keeps optimized; explicit `--profile`/`--release` overrides, and ambient environment does not. — `docs/spec/syntax-decisions.md:4741-4746`
- **D-BUILD-FAIL1=A** — a recognized `fn build` writes a `BuildPlan`; an implicit build failure is a `BuildError`. — `tower` (`D-BUILD-FAIL1` outcome A)
- **D-BUILDCTX-FLAGS1=A** — build facts are typed defaults/CLI overrides, but the later operative wording narrows contribution to `b.contribute(fact, value)` against a declared fact; `fn build` cannot mint an undeclared fact. — `docs/spec/syntax-decisions.md:7178-7182`; `tower` (`D-BUILDCTX-FLAGS1` outcome A)
- **D-BUILDTARGET1=A / D-BUILDACTION1=A / D-BUILDTOOLCHAIN1=A / D-BUILDPROBE1=A** — `fn build` registers typed target handles, cached actions versus explicit uncached side effects, typed toolchain handles with identity, and typed reproducible/ambient configure probes. — `docs/spec/syntax-decisions.md:4704-4714`
- **D-BUILDCACHE1=A / D-BUILDREMOTE1=A / D-BUILDSCHED1=A** — local action caching includes inputs, outputs, argv, environment, caps, tool/target/policy/toolchain/compiler/generated identities; remote cache/execution are separate grants; scheduling is deterministic with named resource pools. — `docs/spec/syntax-decisions.md:4716-4722`
- **D-BUILDLEGACY1=A / D-BUILDPLUGIN1=A** — legacy build systems are Tier-2 declared wrappers; first-party and packaged WASM build plugins use one contract and emit the same `BuildPlan`. — `docs/spec/syntax-decisions.md:4726-4731`
- **D-DSLBLOCK1=A / D-METAMUTATE1=A** — stdlib directive blocks are a fixed whitelist, third-party grammar mutation is rejected, and Jai-style AST mutation/macros are rejected; the additive power surface is generated modules/overlays, targets/actions, graph inspection, DSL blocks, and front-end APIs. — `docs/spec/syntax-decisions.md:4733-4739`
- **R7 / D-TIR seam law** — checked AST lowers to typed IR carrying only sema-approved facts; Rust emission performs “zero inference”; TIR is “the only codegen seam,” and constructs outside it are ICEs, never AST fallback or miscompile. — `docs/spec/architecture.md:40-50`

## Shipped

- `core.compiler.lex`, `.parse`, `.check`, and `.source_map` are the documented immutable compiler values; `jet inspect compiler lex|parse|check|source-map` emits the versioned `schema_version: 1`, `api_version: 1` JSON mirror. — `docs/spec/architecture.md:168-181`
- `jet inspect graph` and `jet inspect explain-build` expose graph order, action inputs/outputs, cache reason, and `jet.explain-build/v1`; compiler owns node identity/dependencies and the store owns run evidence. — `docs/spec/architecture.md:27-38`
- The build hook probe successfully called `core.compiler.lex/parse/check`, emitted generated facts, and used compiler inspect, graph inspect, explain-build, replay/debug, and a lossless rename by returned spans. — `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:3-15`
- Build graph inspection and `explain-build` are usable at the CLI, including one declared action and a `cache=Cached` explanation; a second generated-module build currently fails E3510. — `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:12,16-20`
- The probe's `jet run --record`/`jet debug --replay` path succeeds and altered source fails identity E3621; this is the existing replay seam available to tools, not a graph-diff API. — `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:12-15`
- TIR is the sole code-generation seam and `CmdCompile.rs` drives build-state without a callback/dependency edge back into the root. — `docs/spec/architecture.md:40-50,347-349`
- The machine-wide artifact store, cache status/prune/limit commands, and capacity rules are the canonical store surface; command code is only a surface over the API. — `docs/spec/architecture.md:124-138`
- `D-BUILD-UI1` is ratified as a product shape, while the probe evidence here proves CLI graph/compiler behavior, not completion of every live-board or HTML rendering detail. — `tower` (`D-BUILD-UI1` outcome A); `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:7-13`

## Undecided

- Should Jet expose compiler facts to runtime tool code through a safe, versioned bridge, or must tools remain build-time/CLI clients of the compile-time-only `core.compiler` surface?
- Should graph facts and receipt deltas have a stable in-process typed API, rather than only `jet inspect` CLI output and private store files?
- Should repeated `b.generate`/generated-module builds be idempotent and replace one logical generated module, eliminating E3510 without creating a second source authority?
- Should `jet inspect` expose one stable graph-plus-receipt diff view, and what identity/ordering contract would it use?
- Should generated modules expose a declared dependency/output identity that lets graph and receipt tools explain regeneration without reading private paths?
- Should build plugins receive a typed runtime observation hook, or is the ratified build-time graph/plugin contract sufficient for tool authors?

## Conflicts

- **Compile-time boundary:** D-FRONTENDAPI1 explicitly says build code may inspect compiler values and “runtime code receives E0956”; a runtime compiler-facts proposal must be a new amendment, not a claim that the current API is already callable at runtime. — `docs/spec/architecture.md:170-181`; probe `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:16-20`
- **One graph authority:** D-BUILDQUERY1 and architecture law already select `BuildPlan` plus `jet inspect graph/query/explain-build`; a new independent graph registry or path-local graph would conflict with compiler-owned node identity/dependency ownership. — `docs/spec/architecture.md:27-38`
- **No AST mutation/macros:** D-DSLBLOCK1 and D-METAMUTATE1 reject a runtime or third-party syntax-tree mutation loop. The safe tool path is read-only front-end facts plus declared generated modules/overlays.
- **No daemon:** D-BUILD-SESSION1 permits reuse only through an existing foreground `jet dev --serve`; proposing a separate resident build daemon would reopen the selected session boundary.
- **One store and one receipt mechanism:** D-BUILD-STORE1 owns machine-wide artifacts; a graph/receipt-diff proposal must not create a second cache, receipt store, or private cache policy.
- **Declared facts only:** the current D-BUILDCTX-FLAGS1 wording says `fn build` cannot mint an undeclared fact; arbitrary generated facts need a declared package/build contract, not an implicit escape.
- **Generated output defect:** E3510 is a current idempotence defect observed on the second build, not evidence for a new compiler language or a second generation mechanism. — `~/.cache/jet-luna/dx3/prim-tooling-hooks/probe.md:16-20`
- **TIR boundary:** tool proposals cannot ask codegen to infer or repair missing facts; outside-TIR constructs remain ICEs under the architecture law. — `docs/spec/architecture.md:42-50`
