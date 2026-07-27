# Jet — VS Code / Cursor / VSCodium

Extension id: **`jet-lang.jet`** (publisher `jet-lang`, name `jet`).
Generated TextMate syntax highlighting + LSP: diagnostics, quick-fixes,
formatting, semantic tokens (full/range/delta), inlay hints, navigation,
rename, document links, run/test code lenses, call hierarchy, and type
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

`jet self lsp` only runs the front end (no rustc), so the plain cargo binary works.
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

## Highlighting

Lexical token lists are generated from `crates/jet-foundation/src/Syntax.rs`:

```bash
nix develop -c cargo run --bin jet -- devtools grammars
nix develop -c cargo test --test grammar
```

The LSP semantic overlay refines live editor coloring for ownership (`~`, `^`,
`&`), rules (`#Test`, `#Unsafe`), and effect rows (`=[]=>`, `=[Io]=>`).
Retired or foreign spellings are not colored as live syntax.

Code lenses use **Jet: Run File** and **Jet: Test File**, which open a terminal
running the same `jet` binary the language server uses.

## Verify

```bash
cargo test --test lsp
jet self lsp --bench
```

`--bench` reports cold, warm-hit, and warm-edit latency plus deterministic
query-cache memory counters. Timings are measurements, not flaky wall-clock
pass/fail assertions.

In the editor, open a `.jet` file containing `x :: 1` and expect a clean parse.
In a v5 ownership sample, `copy`, `^`, `&`, and PascalCase markers should color
consistently.
