# Jet

<h1 align="center">
    <img src="./assets/jetlang.png" width="120px" />
</h1>

Jet is a compiled, memory-safe language built for beginners and small tools.
You write Jet; the compiler checks everything in plain language, then generates
Rust for speed. No `unsafe`, no exceptions, no hidden control flow.

## Quickstart

```bash
nix develop                  # Rust, rustc, jet wrapper — see docs/dev/nix.md
cargo build
jet run examples/features/01_hello.jet
```

`hello, world` means the toolchain is working. Next:

```bash
jet check examples/features/02_functions.jet
cargo test                   # golden examples + error snapshots
```

Read the **[15-minute tour](docs/guide/tour.md)** for the full language sketch.

## Showcase tools

Three real CLI tools live in `examples/showcase/`. They are golden-tested like
`examples/features/` and show what Jet looks like in practice.

| Tool | Lines | What it exercises |
|------|------:|-------------------|
| [jetgrep](examples/showcase/jetgrep.jet) | 253 | `std.fs`, `std.io`, `std.process`, CLI flags, exit codes |
| [jsonfmt](examples/showcase/jsonfmt.jet) | 56 | `std.json`, fallible `T ? E`, stdin/files |
| [wordfreq](examples/showcase/wordfreq.jet) | 96 | `Map`, sorting, directory walk, closures |

```bash
jet run examples/showcase/jetgrep.jet pattern examples/showcase/fixtures/
jet run examples/showcase/jsonfmt.jet examples/showcase/fixtures/sample.json
jet run examples/showcase/wordfreq.jet examples/showcase/fixtures/
```

## Errors that teach

Every diagnostic has a stable code, plain **what / why / fix**, and a snapshot
test. Try a typo:

```bash
jet check tests/ui/unknown_function.jet
```

Browse generated pages: [docs/reference/errors/](docs/reference/errors/) (e.g.
[E0102](docs/reference/errors/E0102.md), [E0107](docs/reference/errors/E0107.md)).

## FAQ

**How is Jet different from Rust?**  
Jet keeps ownership and safety but drops most of Rust's surface syntax and
jargon. Errors are values (`T ? E`), not exceptions. There is no macro
system, no `async`/`await` in v1, and the compiler never speaks rustc's
language to you.

**How is Jet different from Go?**  
Jet is statically typed with generics and traits, explicit `val`/`var`, and
stricter error handling — you cannot ignore a fallible result. No goroutines
in v1; use `std.tasks` channels when you need concurrency (v1 is blocking).

**Where is async?**  
Not in v1. Use blocking I/O and `std.tasks` for background work. Async syntax
is planned post-1.0 (see [roadmap](docs/spec/roadmap.md)).

**Why semicolons?**  
Statements end with `;`. Block headers (`if`, `while`, `for`, `fn`) do not.
`jet fmt` settles every formatting argument — run it and move on.

**Can I use this in production?**  
Jet is approaching v1.0. The compiler and stdlib are still evolving; pin your
toolchain in `jet.toml` and read [versioning](docs/reference/versioning.md).

## Repo map

| Path | What |
|------|------|
| [docs/](docs/README.md) | Docs index — start here to find anything |
| [docs/guide/](docs/guide/) | Learner's guide + 15-minute tour |
| [docs/spec/](docs/spec/) | Authoritative: philosophy, syntax decisions, diagnostics, roadmap |
| [docs/reference/](docs/reference/) | Stdlib, versioning, generated error pages |
| [docs/plans/](docs/plans/) | Milestone implementation plans |
| [docs/research/](docs/research/) | Exploratory notes & cross-language idea banks |
| [examples/features/](examples/features/) | Executable spec with golden expected output |
| [examples/showcase/](examples/showcase/) | Real CLI tools (jetgrep, jsonfmt, wordfreq) |
| [editors/](editors/) | VS Code / Zed extensions + tree-sitter grammar |
| [tests/ui/](tests/ui/) | Snapshot-pinned diagnostics |
| `src/` | Compiler: lexer → parser → sema → codegen |

## Nix / NixOS

```bash
nix build                    # produces ./result/bin/jet
nix develop                  # dev shell
```

See [docs/dev/nix.md](docs/dev/nix.md) for flake inputs and `configuration.nix`.

## License

See repository license file.
