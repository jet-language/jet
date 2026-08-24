# Jet — VS Code / Cursor / VSCodium

Extension id: **`jet-lang.jet`** (publisher `jet-lang`, name `jet`).
Generated TextMate syntax highlighting + LSP: diagnostics, quick-fixes,
formatting, semantic tokens (full/range/delta), inlay hints, navigation,
rename, document links, run/test code lenses, call hierarchy, and type
hierarchy.

## Language-server features

| Area | Jet LSP 3.17 behavior |
|---|---|
| Documents | Incremental UTF-16 range sync, stale-version rejection, diagnostics, full/range formatting, quick fixes |
| Completion | Context-aware items, snippets, auto-import edits, signature help |
| Navigation | Hover, definition, references, prepare-rename/rename, document and workspace symbols across workspace folders |
| Structure | Folding, occurrence highlights, selection ranges, document links, run/test code lenses |
| Semantics | Semantic tokens full/range/delta, inlay hints, call hierarchy, trait/type hierarchy |
| Workspace | Multiple roots with folder add/remove notifications; `jet.impact` and `jet.budgetReports` commands |

Every advertised feature maps to a named non-vacuous test in `tests/lsp.rs`.

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

1. The `jet.executablePath` setting, if set (supports `${workspaceFolder}` and `~`).
2. The legacy `jet.languageServerPath` setting, if set.
3. `<workspaceFolder>/target/debug/jet` — covers working on this repo.
4. `jet` on PATH — covers an installed jet (`nix profile install .#jet`) or an
   editor launched from the dev shell.

`jet self lsp` only runs the front end (no rustc), so the plain cargo binary works.
After `cargo build` the running server picks up the new binary via
**Jet: Restart Language Server** (or reload the window).

## Native debugging

The extension registers the `jet` DAP adapter. Use **Jet: Debug File** or press
F5 with a `.jet` file open. The adapter runs `jet debug --dap <file>` and maps
source breakpoints, stepping, pause, stack frames, and Jet locals through the
same native debugger path as the terminal command. LLDB must be on `PATH`.
The adapter starts the selected Jet executable with direct argv. It does not
run a shell. Set `jet.executablePath` when the editor must use a specific
build.
Restart keeps launch arguments and source breakpoints. It expires stack,
scope, and variable references, so the editor must refresh them after the stop.
The adapter accepts strict `Content-Length` frames up to 16 MiB. It requires
`adapterID: "jet"`, uses canonical local source paths, and follows the
`linesStartAt1` and `columnsStartAt1` values from DAP `initialize`.

Optional `.vscode/launch.json` configuration:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "jet",
      "request": "launch",
      "name": "Jet: Launch",
      "program": "${file}",
      "showRawFrames": false
    }
  ]
}
```

For a local attach, use the native debug binary and the matching `.jetmap`
sidecar. The extension reads the Jet source identity from that sidecar before
starting the adapter; the adapter then verifies the same-user process and
build identity.
If the sidecar stores a relative source path, the extension resolves it
relative to the sidecar itself.

```json
{
  "type": "jet",
  "request": "attach",
  "name": "Jet: Attach",
  "program": "${workspaceFolder}/target/debug/jet-program",
  "map": "${workspaceFolder}/target/debug/jet-program.jetmap",
  "processId": 12345
}
```

Set `showRawFrames` to `true` only when you need clearly marked generated-Rust
frames and scopes; the default projection stays in Jet terms.

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
`&`), rules (`#Test`, `#Unsafe`), and effect rows (`-[]>`, `-[IO]>`).
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
