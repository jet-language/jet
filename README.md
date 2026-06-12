# 

<h1 align="center">
    <img src="./jetlang.png" width="120px" /> 
</h1>

Jet is a programming language focused on developer experience, performance, & safety.


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

Errors tell you **what**, **why**, and **how to fix it** — try
`jet check tests/ui/unknown_function.jet`.

## Repo Map

| Path      | What                                                    |
|-----------|---------------------------------------------------------|
| docs/     | 00 philosophy · 01 spec · 02 **syntax decisions (owner)** · 03 architecture · 04 diagnostics · 05 roadmap |
| src/      | the compiler: lexer → parser → sema → codegen + CLI     |
| examples/ | executable spec, with expected outputs                  |
| examples/preview/ | syntax previews (not compiled by golden tests)    |
| tests/ui/ | every error message, snapshot-pinned                    |
