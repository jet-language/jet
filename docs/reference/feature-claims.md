# Advertised feature claims

This is the canonical inventory of broad public Jet claims. Every row has one
stable claim ID and one reviewed entry in
`docs/spec/feature-claim-manifest.json`. CLI commands and `core.*` modules
are inventoried independently from their source registries by the same gate.

| Claim ID | Public claim |
| --- | --- |
| `claim.syntax-law` | Every current unbuilt syntax note has a machine-checked status-matrix row. |
| `claim.examples-spec` | Every feature example has a declared expected-output artifact. |
| `claim.native-language` | Jet compiles safe source to native programs with Jet-owned semantics. |
| `claim.tier-parity` | AOT, Cranelift JIT (`jet run` / `jet dev`), interpreter, and web preserve one executable meaning (AGENTS.md I9). Semantics live only in the embedded Prelude parts (`crates/jet-foundation` prelude modules and `crates/jet-codegen/src/Prelude/**`) and ratified CoreLib. Engines only marshal and call those functions; parallel validation, defaults, policy, and error behavior violate I9/R12. Closure proves AOT and default `jet run`, and deopt ambient calls the same Prelude symbol. AOT-only ships and durable `jit_gaps` entries are forbidden. |
| `claim.static-guarantees` | Refinements, contracts, information flow, budgets, and replay share one facts engine. |
| `claim.discard-control` | Ignoring a must-use or fallible value is explicit and audited. |
| `claim.prelude-control` | Beginners get the prelude automatically and experts may opt out explicitly. |
| `claim.maturity-tags` | Dependency maturity can be stated without changing runtime semantics. |
| `claim.generic-modules` | Generic modules instantiate applicatively with type parameters and closed Bool, Int, Char, String, and fieldless-enum values, including Int `[T#capacity]`. |
| `claim.metaprogramming` | Typed generated items use the ordinary Jet grammar and semantic pipeline; build materialization may write the checked items as `.jet` source. |
| `claim.tooling-cli` | Jet ships one coherent beginner-first command-line toolset. |
| `claim.ide-debug` | Editor analysis and debugging share incremental Jet semantic facts. |
| `claim.format-test` | Project formatting and testing have complete deterministic workflows. |
| `claim.package-build` | Builds, packages, environments, and OS operations have canonical product ownership. |
| `claim.core-foundation` | Core foundation APIs are real, reachable, documented Jet software. |
| `claim.core-concurrency` | Tasks, events, and async loading use one structured runtime. |
| `claim.core-files-data` | Files, paths, archives, compression, and databases meet production contracts. |
| `claim.core-encoding-text` | Codecs and Unicode text follow their published standards. |
| `claim.core-network-http` | Networking and HTTP are bounded, interoperable production implementations. |
| `claim.core-security` | Cryptographic APIs are misuse-resistant and fail closed. |
| `claim.core-data-compute` | Typed data and statistical sketches execute their documented semantics. |
| `claim.core-ui-web` | Reactive UI and web targets run one typed component model. |
| `claim.game-product` | Jet ships a playable game runtime plus source-backed editor workflow. |
| `claim.plugin-ffi` | Foreign libraries use one typed, safe interop structure. |
| `claim.embedded` | Typed target machines produce real firmware and kernel artifacts. |
| `claim.adaptive-runtime` | Applications choose runtime fidelity through one explicit manual signal. |
| `claim.logic-programming` | `core.solve` records finite Boolean constraints deterministically. |
| `claim.structural-merge` | Jet has a checked structural diff and merge path keyed by semantic identity. |
| `claim.proof-replay` | `jet prove` combines contracts, effects, budgets, tests, and replay facts. |
| `claim.performance-budgets` | Typed performance budgets are enforced against pinned baselines. |
| `claim.product-boundaries` | `jet`, `jetpack`, and `jetos` have clean canonical ownership. |
