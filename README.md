# Jet

Jet is a programming language focused on developer experience, performance, & safety.

A memory-safe, compiled language that is **beginner-first without a
garbage collector**. Rust-class safety and performance; Python-class
onboarding. v1 transpiles to Rust: our front end owns every check and
every error message; rustc is the silent verifier and optimizer behind
the curtain.

**Status:** M0 walking skeleton written, awaiting its first
`cargo build && cargo test` (authored in a sandbox without a Rust
toolchain — see CLAUDE.md, task zero).

## Quickstart

```
cargo build
./target/debug/jet run examples/01_hello.jet
./target/debug/jet check examples/02_functions.jet
./target/debug/jet build examples/01_hello.jet --emit-rust
cargo test          # ui snapshots + golden examples
```

### Nix / NixOS

```bash
nix build                    # produces ./result/bin/jet
nix develop                  # dev shell with cargo + rustc + jet
```

See **docs/nix.md** for adding `jet` to `configuration.nix` via a flake
input.

## The pitch in three lines

```
fn main() {
    print("hello, world");
}
```

Errors tell you **what**, **why**, and **how to fix it** — try
`jet check tests/ui/unknown_function.jet`.

## Repo map

| Path      | What                                                    |
|-----------|---------------------------------------------------------|
| docs/     | 00 philosophy · 01 spec · 02 **syntax decisions (owner)** · 03 architecture · 04 diagnostics · 05 roadmap |
| src/      | the compiler: lexer → parser → sema → codegen + CLI     |
| examples/ | executable spec, with expected outputs                  |
| examples/preview/ | syntax previews (not compiled by golden tests)    |
| tests/ui/ | every error message, snapshot-pinned                    |
| CLAUDE.md | operating manual for AI agents building this            |

## How this project is run

A human owner ratifies every piece of user-facing syntax in
**docs/02-syntax-decisions.md** — that file is the steering wheel.
Agents do the building under the invariants in **CLAUDE.md**. The
philosophy doc settles arguments; the roadmap orders the work.
