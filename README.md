# Jet

<h1 align="center">
    <img src="./assets/jetlang.png" width="120px" />
</h1>

Jet is a compiled, memory-safe language: magic out-of-the-box for beginners,
full expert control behind explicit opt-in. You write Jet; the compiler checks
everything in plain language, then generates Rust for speed. No hidden `unsafe`,
no exceptions, no hidden control flow.

<!-- Stable IDs bind advertised claims to docs/spec/capability-claim-manifest.json. -->
<!-- CAPABILITY_CLAIMS:BEGIN -->
<!-- CAPABILITY_CLAIM: claim.syntax-law | Unbuilt syntax notes are machine inventoried. -->
<!-- CAPABILITY_CLAIM: claim.examples-spec | Feature examples declare expected output artifacts. -->
<!-- CAPABILITY_CLAIM: claim.native-language | Jet compiles safe source to native programs. -->
<!-- CAPABILITY_CLAIM: claim.tier-parity | Execution tiers target one semantic meaning. -->
<!-- CAPABILITY_CLAIM: claim.static-guarantees | Static guarantees share one facts model. -->
<!-- CAPABILITY_CLAIM: claim.discard-control | Must-use discard is explicit and audited. -->
<!-- CAPABILITY_CLAIM: claim.prelude-control | Prelude defaults and opt-out share one loader. -->
<!-- CAPABILITY_CLAIM: claim.maturity-tags | Maturity is declared without runtime ambiguity. -->
<!-- CAPABILITY_CLAIM: claim.generic-modules | Modules instantiate with types and closed Bool, Int, Char, String, or fieldless-enum values. -->
<!-- CAPABILITY_CLAIM: claim.metaprogramming | Generated source re-enters Jet semantics. -->
<!-- CAPABILITY_CLAIM: claim.embedded | Target profiles produce embedded artifacts. -->
<!-- CAPABILITY_CLAIM: claim.adaptive-runtime | Runtime policy consumes measured environment facts. -->
<!-- CAPABILITY_CLAIM: claim.logic-programming | Jet exposes a bounded logic subset. -->
<!-- CAPABILITY_CLAIM: claim.structural-merge | Programs merge by semantic identity. -->
<!-- CAPABILITY_CLAIM: claim.proof-replay | Proof and replay share typed facts. -->
<!-- CAPABILITY_CLAIM: claim.performance-budgets | Budgets enforce pinned expectations. -->
<!-- CAPABILITY_CLAIM: claim.product-boundaries | jet, jetpack, and jetos have canonical owners. -->
<!-- CAPABILITY_CLAIMS:END -->

Live development status and work order are in [Tower](docs/README.md); the
[roadmap](docs/spec/roadmap.md) records verified history and durable program
ownership.

## Quickstart

```bash
scripts/agent/jet-env cargo build
scripts/agent/jet-env jet run examples/features/basics/hello.jet
```

`hello, world` means the toolchain is working. Next:

```bash
scripts/agent/jet-env jet check examples/features/basics/functions.jet
scripts/agent/jet-env cargo test --test golden examples_compile_and_run
```

The language spec lives in [docs/spec/spec.md](docs/spec/spec.md). Ratified syntax decisions are in [docs/spec/syntax-decisions.md](docs/spec/syntax-decisions.md).

## Syntax canon

[`examples/canon.jet`](examples/canon.jet) is the compiling syntax showcase — every
line is ratified and implemented. It is golden-tested (`tests/release_gates.rs`). Milestone
feature programs live under [`examples/features/`](examples/features/) with the
same golden harness (`tests/golden.rs`).

```bash
scripts/agent/jet-env jet run examples/canon.jet
```

## Errors that teach

Every diagnostic has a stable code, plain **what / why / fix**, and a snapshot
test. Try a typo:

```bash
scripts/agent/jet-env jet check tests/ui/unknown_function.jet
```

Browse generated pages: [docs/reference/errors/](docs/reference/errors/) (e.g.
[E0102](docs/reference/errors/E0102.md), [E0107](docs/reference/errors/E0107.md)).

## FAQ

**How is Jet different from Rust?**  
Jet keeps ownership and safety but drops most of Rust's surface syntax and
jargon. Errors are values (`T ? E`), not exceptions. There is no macro
system, no `async`/`await`, and the compiler never speaks rustc's language to
you. Expert unsafe is opt-in via `@Unsafe("reason") { … }`, not the default.

**How is Jet different from Go?**  
Jet is statically typed with generics and traits, and stricter error handling —
you cannot ignore a fallible result. Bindings are `name :: value` (immutable)
or `name := value` (mutable). Use `core.tasks` channels for concurrency
(blocking; async is deferred to a later epoch).

**Where is async?**  
Deferred to a later epoch. Use blocking I/O and `core.tasks` for background
work today (see [roadmap](docs/spec/roadmap.md)).

**Do I type semicolons?**  
No. The lexer inserts statement terminators automatically (Go-style). Block
headers (`if`, `loop`, `fn`) don't need them; line continuation works when
the next line starts with `.` or a binary operator. `jet fmt` handles layout.

**Can I use this in production?**  
The language, compiler, and core library are post-v1.0. Pin your toolchain with
`edition:` in `pkg.jet` and read [versioning](docs/reference/versioning.md).
Not yet ready: registry upload (`jet registry publish` validates but does not upload —
use git-based dependencies), `jet store gc` (stub until M12.2 registry lands), and
`jet self doctor --online` (registry not wired). TLS requires the separate `jet.tls`
package; the built-in HTTP client is plain HTTP only.

## Repo map

| Path | What |
|------|------|
| [docs/](docs/README.md) | Docs index — start here to find anything |
| [docs/spec/](docs/spec/) | Authoritative: philosophy, syntax decisions, diagnostics, roadmap |
| [docs/reference/](docs/reference/) | Stdlib, versioning, generated error pages |
| [docs/research/](docs/research/) | Exploratory notes & cross-language idea banks |
| [docs/plans/](docs/plans/) | Project management: epoch plans, proposals, sidequests, ballots |
| [examples/features/](examples/features/) | Executable spec — golden-tested feature programs |
| [examples/canon.jet](examples/canon.jet) | Compiling syntax showcase (golden-tested) |
| [examples/features/](examples/features/) | Milestone feature programs (golden-tested) |
| [editors/](editors/) | VS Code / Zed extensions + tree-sitter grammar |
| [tests/ui/](tests/ui/) | Snapshot-pinned diagnostic fixtures |
| [crates/](crates/) | Compiler seams, runtime, and developer-product crates |
| `Source/` | Thin root binary host and native build executor |

## Build environment

```bash
nix build                           # produces ./result/bin/jet
scripts/agent/jet-env cargo build   # pinned contributor environment
```

Run contributor `jet`, `cargo`, and repository-tool commands through
`scripts/agent/jet-env`; it selects the pinned environment used by CI.

## License

See repository license file.
