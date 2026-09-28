# Jet for Zed

The Zed extension is **Jet** (`jet-lang`). It supplies Tree-sitter syntax
highlighting and starts Jet's language server for diagnostics, completion,
hover, navigation, rename, formatting, semantic tokens, inlay hints,
quick-fixes, document links, folding, and run/test code lenses. The extension
manifest is [`extension.toml`](extension.toml); its process capability invokes
`jet self lsp`.

## Prepare and install a dev extension

From the repository root:

```sh
Tools/agent/jet-env cargo build
Tools/editors/zed/install.sh
```

In Zed, open **zed: extensions**, remove an older Jet dev extension if one is
present, choose **Add Dev Extension**, select `Tools/editors/zed/`, and reload the
window. The language picker entry is **Jet** with a capital J. The repository's
`.zed/settings.json` associates `.jet` files with that language when the
project is opened.

`install.sh` prepares two WebAssembly assets. It syncs the authoritative
Tree-sitter sources from [`Tools/editors/tree-sitter`](../tree-sitter/) into the
standalone `grammar-repo/`, builds `grammars/jet.wasm`, and prebuilds
`extension.wasm` from [`wasm-src`](wasm-src/). Prebuilding avoids Zed trying
to compile the extension root as a Rust project. If a rebuild is needed, use:

```sh
FORCE=1 Tools/editors/zed/install.sh
```

The script removes the generated `grammars/jet/` clone so Zed can fetch it
from the local grammar repository during extension installation.

## Server trust and discovery

The extension requests one literal command:

```text
jet self lsp
```

Zed's worktree-trust gate controls whether this process can start. Restricted
worktrees do not start the language server. The extension does not call
`Worktree::which`, read an executable path from the worktree, or execute a
worktree-provided `jet` binary; the command identity is fixed in
[`wasm-src/src/lib.rs`](wasm-src/src/lib.rs). Put the trusted Jet executable on
the editor process's `PATH` before opening a trusted project.

`jet self lsp` does not invoke `rustc`, so the plain executable produced by the
repository build is sufficient. Rebuild it and reload Zed after changing the
server.

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

Lexical grammar sources are in
[`Tools/editors/tree-sitter`](../tree-sitter/), while the compiler's token vocabulary
is defined in [`Syntax.rs`](../../../crates/jet-foundation/src/Syntax.rs). The LSP
semantic overlay refines live coloring for ownership markers, rules,
decorators, and effect rows; it does not change compiler parsing.

Useful repository checks are:

```sh
Tools/agent/jet-env cargo test --test lsp
Tools/agent/jet-env target/debug/jet self lsp doctor
Tools/agent/jet-env target/debug/jet self lsp --bench
```

The benchmark reports cold, warm-hit, and warm-edit measurements plus query
cache counters. Its timings are measurements, not a wall-clock pass/fail
contract.
