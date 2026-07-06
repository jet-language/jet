# Jet — Zed

Zed dev extension: generated Tree-sitter lexical highlighting plus `jet lsp`
for diagnostics, completion, hover, go-to-definition, rename, semantic tokens
(full/range/delta), inlay hints, quick-fixes, document symbols, document
links, code lenses, folding, selection ranges, call hierarchy, and type
hierarchy.

## Setup

From the repo root:

```bash
nix develop -c cargo build                  # produces target/debug/jet
nix develop -c editors/zed/install.sh       # syncs grammar, generates extension.toml
```

In Zed:

1. Command palette → **zed: extensions**
2. Remove any previous Jet dev extension (old id was `jet`, now `jet-lang`)
3. **Add Dev Extension** (top right)
4. Choose `editors/zed/` in this repo
5. **zed: reload window**
6. Open any `.jet` file

In the language picker, look for **Jet** (capital J), not `jet`.

This repo also ships `.zed/settings.json` so `.jet` files associate with Jet
when you open the project in Zed.

### Why `install.sh`?

Zed tries to compile extension Rust when it finds `Cargo.toml` in the extension
folder. That often fails with nix-only `rustc` (no `wasm32-wasip2` std) or when
`rustup` is not on the GUI app's PATH. This repo prebuilds `extension.wasm`
instead; the Rust sources live in `wasm-src/` so Zed skips compilation.

If you have rustup and want to rebuild manually:

```bash
rustup target add wasm32-wasip2
editors/zed/install.sh
```

## How the extension finds the server

In order:

1. `<workspace>/target/debug/jet` when the workspace contains `flake.nix`
   (developing the compiler in this repo).
2. `jet` on `$PATH` (e.g. from `nix develop`, or `nix profile install .#jet`).
3. Falls back to `<workspace>/target/debug/jet` for other projects.

`jet lsp` only runs the front end (no rustc), so the plain cargo binary works.
Rebuild with `cargo build` and reload Zed to pick up server changes.

## Verify

```bash
nix develop -c jet lsp doctor
nix develop -c cargo test --test lsp
```

In Zed, open a file with `let x = 1;` — expect **E0009** with a quick-fix
to `x :: 1` or `x := 1`.

## Grammar note

`grammars/jet.wasm` is prebuilt by `install.sh`. Authoritative grammar sources
live in `editors/tree-sitter/`; `install.sh` syncs them into `grammar-repo/`,
which Zed clones into `grammars/jet/` on install (that folder is removed by
`install.sh` so checkout stays clean).

Lexical token lists are generated from `crates/jet-foundation/src/Syntax.rs`:

```bash
nix develop -c cargo run --bin jet -- devtools grammars
nix develop -c cargo test --test grammar
```

The LSP semantic overlay refines live editor coloring for ownership (`copy`,
`^`, `&`) and markers (`#Test`, `#Unsafe`, `@Pure`). Retired/foreign spellings
are not colored as live syntax.

## Reinstall after changes

```bash
nix develop -c editors/zed/install.sh
# To rebuild grammar or extension wasm from scratch:
FORCE=1 nix develop -c editors/zed/install.sh
```

Remove and re-add the dev extension in Zed if the server or grammar did not
refresh.
