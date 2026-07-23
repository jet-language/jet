# Jet — Zed

Zed dev extension: generated Tree-sitter lexical highlighting plus `jet self lsp`
for diagnostics, completion, hover, go-to-definition, rename, semantic tokens
(full/range/delta), inlay hints, quick-fixes, document symbols, document
links, code lenses, folding, selection ranges, call hierarchy, and type
hierarchy.

## Language-server capabilities

| Area | Jet LSP 3.17 behavior |
|---|---|
| Documents | Incremental UTF-16 range sync, stale-version rejection, diagnostics, full/range formatting, quick fixes |
| Completion | Context-aware items, snippets, auto-import edits, signature help |
| Navigation | Hover, definition, references, prepare-rename/rename, document and workspace symbols across workspace folders |
| Structure | Folding, occurrence highlights, selection ranges, document links, run/test code lenses |
| Semantics | Semantic tokens full/range/delta, inlay hints, call hierarchy, trait/type hierarchy |
| Workspace | Multiple roots with folder add/remove notifications; `jet.impact` and `jet.budgetReports` commands |

Every advertised capability maps to a named non-vacuous test in `tests/lsp.rs`.

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

`jet self lsp` only runs the front end (no rustc), so the plain cargo binary works.
Rebuild with `cargo build` and reload Zed to pick up server changes.

## Verify

```bash
nix develop -c jet self lsp doctor
nix develop -c cargo test --test lsp
nix develop -c jet self lsp --bench
```

`--bench` reports cold, warm-hit, and warm-edit latency plus deterministic
query-cache memory counters. Timings are measurements, not flaky wall-clock
pass/fail assertions.

In Zed, open a file with `x :: 1` and expect a clean parse.

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
`^`, `&`), rules (`#Test`, `#Unsafe`), and effect rows (`--[]->`,
`--[Io]->`). Retired/foreign spellings are not colored as live syntax.

## Reinstall after changes

```bash
nix develop -c editors/zed/install.sh
# To rebuild grammar or extension wasm from scratch:
FORCE=1 nix develop -c editors/zed/install.sh
```

Remove and re-add the dev extension in Zed if the server or grammar did not
refresh.
