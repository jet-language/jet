# Jet — VS Code / Cursor / VSCodium

Extension id: **`jet-lang.jet`** (publisher `jet-lang`, name `jet`).
Syntax highlighting + LSP v0: diagnostics, quick-fixes for teaching
errors (e.g. `let` → `val`), and formatting.

## Setup

From the repo root:

```bash
nix develop                 # toolchain (cargo, rustc, node)
cargo build                 # produces target/debug/jet — the language server
editors/vscode/install.sh   # packs a .vsix and installs it into cursor/codium/code
```

Then open the repo normally (`cursor .`) and open any `.jet` file.
No workspace file and no settings are needed.

## How the extension finds the server

In order:

1. The `jet.languageServerPath` setting, if set (supports `${workspaceFolder}` and `~`).
2. `<workspaceFolder>/target/debug/jet` — covers working on this repo.
3. `jet` on PATH — covers an installed jet (`nix profile install .#jet`) or an
   editor launched from the dev shell.

`jet lsp` only runs the front end (no rustc), so the plain cargo binary works.
After `cargo build` the running server picks up the new binary via
**Jet: Restart Language Server** (or reload the window).

## Manual install

`--install-extension` needs a `.vsix` file or an extension id — not a
directory path. If you don't want install.sh:

```bash
cd editors/vscode
npm install
npx --yes @vscode/vsce package -o jet.vsix   # bundles vscode-languageclient
cursor --install-extension "$(pwd)/jet.vsix"
```

## Verify

```bash
cargo test --test lsp
```

In the editor, open a `.jet` file containing `let x = 1;` — expect an
**E0009** diagnostic pointing at `::` / `:=` binding sigils (use `x :: 1` or `x := 1`).
