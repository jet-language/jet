# Jet for Zed

The Zed extension is **Jet** (`jet-lang`). It supplies Tree-sitter syntax
highlighting and starts Jet's language server for diagnostics, completion,
hover, navigation, rename, formatting, semantic tokens, inlay hints,
quick-fixes, document links, folding, and run/test code lenses. The extension
manifest is [`extension.toml`](extension.toml); its process capability invokes
`jet self lsp`.

## Build and install the dev extension

From the repository root:

```sh
Tools/editors/zed/install.sh
```

`install.sh` builds the two WebAssembly assets and links the extension into
Zed:

- `grammars/jet.wasm` is compiled from the committed, generated parser in
  [`Tools/editors/tree-sitter/src`](../tree-sitter/src) with the same command
  Zed's extension builder uses (its wasi-sdk `clang -fPIC -shared -Os`), so a
  later **Rebuild** inside Zed produces the same parser. The script also
  mirrors the grammar into the local git repository `grammar-repo/` that
  `extension.toml` names, and removes `grammars/jet/` so Zed can clone it.
- `extension.wasm` is prebuilt from [`wasm-src`](wasm-src/) for
  `wasm32-wasip2`. Zed only compiles Rust when `Cargo.toml` sits in the
  extension root, which fails for nix-only toolchains, and it loads a present
  `extension.wasm` on its own.
- The dev-extension link `~/.local/share/zed/extensions/installed/jet-lang` is
  pointed at `Tools/editors/zed/` of the checkout the script runs from (a
  published, non-symlink install is left alone). Zed's extension watcher
  reloads it; **zed: reload extensions** forces it. Moving the checkout
  breaks the link, so rerun the script after a move.

Only stale assets are rebuilt; `FORCE=1` rebuilds both and `NO_LINK=1` skips
the Zed link. The language picker entry is **Jet** with a capital J.

When highlighting disappears, look in `~/.local/share/zed/logs/Zed.log` for
`failed to load language Jet` or `Failed to load extension: jet-lang`: a query
that names a node or string the grammar lacks makes Zed drop the whole
language.

## Server trust and discovery

The extension starts `jet self lsp`. Zed's worktree-trust gate controls whether
the process can start; restricted worktrees do not start the language server.
The executable is chosen in this order
([`wasm-src/src/lib.rs`](wasm-src/src/lib.rs)):

1. the Zed setting `lsp.jet-lsp.binary.path`, with `binary.arguments`
   (default `self lsp`) and `binary.env` when set;
2. otherwise `jet` on the worktree's `PATH` (Zed's `Worktree::which`);
3. otherwise the server does not start and Zed shows an error that names
   `lsp.jet-lsp.binary.path`.

```json
{ "lsp": { "jet-lsp": { "binary": { "path": "/abs/path/to/jet" } } } }
```

The extension never reads the worktree's files or guesses a repository build
output. A trusted worktree whose `PATH` puts a
different `jet` first gets that `jet`; pin `binary.path` to avoid it.

`jet self lsp` does not invoke a host compiler, so a plain Jet executable is
sufficient. Rebuild it and reload Zed after changing the server.

## Native debugging

This extension is language support only; it does not register a Jet DAP
adapter. Use a terminal for native debugging:

```sh
jet debug path/to/file.jet
```

The VS Code extension documents the DAP launch and attach configuration. Zed's
extension API does not provide the trust and authorization hooks needed to
safely add that adapter here.

## Grammar and focused checks

The editor grammar is [`Tools/editors/tree-sitter`](../tree-sitter/). It is
deliberately permissive: Jet source is read as balanced `()`/`[]`/`{}` and
string groups of elements, and only what the editor colors, folds, indents or
outlines gets its own node (declarations, calls, members, bindings, labels,
constructors, variants, markers, literals). New syntax therefore keeps
parsing; the compiler remains the only judge of validity. Keyword spellings
come from the generated `BEGIN/END GENERATED JET SYNTAX HIGHLIGHTS` sections,
rendered from the syntax registry
([`Highlights.jet`](../../../Compiler/JetFoundation/Data/Syntax/Highlights.jet)
and the active rows of [`Markers.jet`](../../../Compiler/Prelude/Markers.jet))
by `node Tools/agent/editor-grammars.mjs --write`. Every string in
`highlights.scm` must be an anonymous grammar token. The LSP semantic overlay
refines live coloring for
ownership markers, rules, decorators, and effect rows; it does not change
compiler parsing.

After editing `grammar.js`, regenerate and check it in
`Tools/editors/tree-sitter`:

```sh
tree-sitter generate && tree-sitter test
tree-sitter parse --quiet --stat ../../../Examples/**/*.jet
```

then rerun `install.sh`.

Useful repository checks, with `JET` naming an admitted Jet executable, are:

```sh
node Tools/agent/editor-grammars.mjs --check
Tools/agent/jet-env "$JET" test Compiler/JetLSP
Tools/agent/jet-env "$JET" self lsp doctor
Tools/agent/jet-env "$JET" self lsp --bench
```

The benchmark reports cold, warm-hit, and warm-edit measurements plus query
cache counters. Its timings are measurements, not a wall-clock pass/fail
contract.
