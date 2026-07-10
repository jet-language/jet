# Jet

<h1 align="center">
    <img src="./assets/jetlang.png" width="120px" />
</h1>

Jet is a compiled, memory-safe language: magic out-of-the-box for beginners,
full expert control behind explicit opt-in. You write Jet; the compiler checks
everything in plain language, then generates Rust for speed. No hidden `unsafe`,
no exceptions, no hidden control flow.

**Current status: Epoch 3** (Epoch 1 v1.0 + Epoch 2 GA are complete — see [roadmap](docs/spec/roadmap.md)).

## Quickstart

```bash
nix develop -c cargo build
nix develop -c jet run examples/features/basics/hello.jet
```

`hello, world` means the toolchain is working. Next:

```bash
nix develop -c jet check examples/features/basics/functions.jet
nix develop -c cargo test    # golden examples + error snapshots
```

The language spec lives in [docs/spec/spec.md](docs/spec/spec.md). Ratified syntax decisions are in [docs/spec/syntax-decisions.md](docs/spec/syntax-decisions.md).

## Syntax canon

[`examples/canon.jet`](examples/canon.jet) is the compiling syntax showcase — every
line is ratified and implemented. It is golden-tested (`tests/release_gates.rs`). Milestone
feature programs live under [`examples/features/`](examples/features/) with the
same golden harness (`tests/golden.rs`).

```bash
nix develop -c jet run examples/canon.jet
```

## Errors that teach

Every diagnostic has a stable code, plain **what / why / fix**, and a snapshot
test. Try a typo:

```bash
nix develop -c jet check tests/ui/unknown_function.jet
```

Browse generated pages: [docs/reference/errors/](docs/reference/errors/) (e.g.
[E0102](docs/reference/errors/E0102.md), [E0107](docs/reference/errors/E0107.md)).

## FAQ

**How is Jet different from Rust?**  
Jet keeps ownership and safety but drops most of Rust's surface syntax and
jargon. Errors are values (`T ? E`), not exceptions. There is no macro
system, no `async`/`await`, and the compiler never speaks rustc's language to
you. Expert unsafe is opt-in via `#Unsafe("reason") { … }`, not the default.

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
Not yet ready: registry upload (`jet publish` validates but does not upload —
use git-based dependencies), `jet gc` (stub until M12.2 registry lands), and
`jet doctor --online` (registry not wired). TLS requires the separate `jet.tls`
package; the built-in HTTP client is plain HTTP only.

## Repo map

| Path | What |
|------|------|
| [docs/](docs/README.md) | Docs index — start here to find anything |
| [docs/spec/](docs/spec/) | Authoritative: philosophy, syntax decisions, diagnostics, roadmap |
| [docs/reference/](docs/reference/) | Stdlib, versioning, generated error pages |
| [docs/research/](docs/research/) | Exploratory notes & cross-language idea banks |
| [docs/](docs/) | Project management: milestone plans, ballots, epoch tracking |
| [examples/features/](examples/features/) | Executable spec — golden-tested feature programs |
| [examples/canon.jet](examples/canon.jet) | Compiling syntax showcase (golden-tested) |
| [examples/features/](examples/features/) | Milestone feature programs (golden-tested) |
| [editors/](editors/) | VS Code / Zed extensions + tree-sitter grammar |
| [tests/ui/](tests/ui/) | Snapshot-pinned diagnostic fixtures |
| `Source/` | Compiler: lexer → parser → sema → codegen |

## Nix / NixOS

```bash
nix build                    # produces ./result/bin/jet
nix develop                  # dev shell with Rust, jet, and repo utilities
```

All `jet` and `cargo` commands should run inside `nix develop -c …` to use the pinned toolchain.
CI uses the same Nix path (`nix flake check`, `nix develop -c cargo check`,
and focused test suites), so local failures match the hosted environment.

## License

See repository license file.
