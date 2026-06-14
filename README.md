# Jet

<h1 align="center">
    <img src="./jetlang.png" width="120px" />
</h1>

Jet is a compiled, memory-safe language built for beginners and small tools.
You write Jet; the compiler checks everything in plain language, then generates
Rust for speed. No `unsafe`, no exceptions, no hidden control flow.

## Quickstart

```bash
nix develop                  # Rust, rustc, jet wrapper — see docs/nix.md
cargo build
jet run examples/01_hello.jet
```

`hello, world` means the toolchain is working. Next:

```bash
jet check examples/02_functions.jet
cargo test                   # golden examples + error snapshots
```

Read the **[15-minute tour](docs/tour.md)** for the full language sketch.

## Showcase tools

Three real CLI tools live in `showcase/`. They are golden-tested like
`examples/` and show what Jet looks like in practice.

| Tool | Lines | What it exercises |
|------|------:|-------------------|
| [jetgrep](showcase/jetgrep.jet) | 253 | `std.fs`, `std.io`, `std.process`, CLI flags, exit codes |
| [jsonfmt](showcase/jsonfmt.jet) | 56 | `std.json`, fallible `T ? E`, stdin/files |
| [wordfreq](showcase/wordfreq.jet) | 96 | `Map`, sorting, directory walk, closures |

```bash
jet run showcase/jetgrep.jet pattern showcase/fixtures/
jet run showcase/jsonfmt.jet showcase/fixtures/sample.json
jet run showcase/wordfreq.jet showcase/fixtures/
```

## Errors that teach

Every diagnostic has a stable code, plain **what / why / fix**, and a snapshot
test. Try a typo:

```bash
jet check tests/ui/unknown_function.jet
```

Browse generated pages: [docs/errors/](docs/errors/) (e.g.
[E0102](docs/errors/E0102.md), [E0107](docs/errors/E0107.md)).

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
is planned post-1.0 (see [roadmap](docs/admin/05-roadmap.md)).

**Why semicolons?**  
Statements end with `;`. Block headers (`if`, `while`, `for`, `fn`) do not.
`jet fmt` settles every formatting argument — run it and move on.

**Can I use this in production?**  
Jet is approaching v1.0. The compiler and stdlib are still evolving; pin your
toolchain in `jet.toml` and read [versioning](docs/versioning.md).

## Repo map

| Path | What |
|------|------|
| [docs/tour.md](docs/tour.md) | 15-minute language tour (every snippet compiles) |
| [docs/errors/](docs/errors/) | Error code pages generated from snapshots |
| [docs/08-stdlib.md](docs/08-stdlib.md) | Standard library reference (synced from stdlib.md) |
| [docs/admin/](docs/admin/) | Philosophy, syntax decisions, diagnostics, roadmap |
| [examples/](examples/) | Executable spec with golden expected output |
| [showcase/](showcase/) | Real CLI tools (jetgrep, jsonfmt, wordfreq) |
| [tests/ui/](tests/ui/) | Snapshot-pinned diagnostics |
| `src/` | Compiler: lexer → parser → sema → codegen |

## Nix / NixOS

```bash
nix build                    # produces ./result/bin/jet
nix develop                  # dev shell
```

See [docs/nix.md](docs/nix.md) for flake inputs and `configuration.nix`.

## License

See repository license file.
