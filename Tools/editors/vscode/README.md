# Jet for VS Code, Cursor, and VSCodium

The extension id is **`jet-lang.jet`** (publisher `jet-lang`, package name
`jet`). It provides Jet syntax highlighting, the language server, diagnostics,
completion, navigation, semantic tokens, inlay hints, formatting, rename, and
run/test code lenses. The package manifest is the authoritative list of
commands and settings: [`package.json`](package.json).

## Install from this checkout

Build the repository executable, then package and install the extension:

```sh
Tools/agent/jet-env cargo build
Tools/editors/vscode/install.sh
```

The installer packages `vscode-languageclient` into a `.vsix` and installs it
with the first available `cursor`, `codium`, or `code` command. If the editor
is already running, reload the window after installation. The extension
requires VS Code-compatible editor version 1.80 or newer.

Open a trusted workspace containing a `.jet` file. Untrusted workspaces do not
start the language server, debugger, or a workspace-selected executable.

## Server discovery

The extension chooses the executable in this order:

1. `jet.executablePath` (with `${workspaceFolder}`, `${workspaceRoot}`, and
   `~` expansion);
2. the legacy `jet.languageServerPath` setting;
3. `<workspaceFolder>/target/debug/jet` in a trusted workspace; and
4. `jet` on `PATH`.

The server is started as `jet self lsp` over stdio. It does not invoke `rustc`,
so a plain Jet executable is sufficient. After rebuilding the executable, use
**Jet: Restart Language Server** or reload the editor window.

Example settings:

```json
{
  "jet.executablePath": "${workspaceFolder}/target/debug/jet",
  "jet.inlayHints.cloneHints": true,
  "jet.inlayHints.typeAnnotations": false
}
```

`jet.languageServerPath` remains readable for existing workspaces; new
settings should use `jet.executablePath`.

## Commands and code lenses

The command palette provides:

- **Jet: Run File**, which opens a terminal for `jet run <file>`;
- **Jet: Test File**, which opens a terminal for `jet test <file>`;
- **Jet: Debug File**, which starts the native debug adapter;
- **Jet: Learn (Watch)** and **Jet: Learn Once**; and
- **Jet: Explain Reasoning**, which opens the static reasoning panel.

The run/test code lenses use the same executable selected for the language
server. They do not create a separate editor-specific execution path.

## Rename safety

Rename records the checked version of every open document before requesting
edits. If any affected document changes while the rename is pending, the
extension refuses the whole edit set; run **Rename** again to obtain a fresh
checked edit.

## Explain Reasoning

**Jet: Explain Reasoning** presents facts checked by the language service,
including value, ownership, relationship, source span, and producer details.
It does not execute the program. Refresh after changing source. A changed
source document marks the panel stale; **Open source** remains available, but
source spans are followed only after refresh. Pinning keeps the panel on its
current file while other files are inspected.

## Native debugging

The extension registers the `jet` DAP type. **Jet: Debug File** (or F5 with a
`.jet` file open) starts the selected executable directly with:

```text
jet debug --dap <file>
```

No shell is inserted between the editor and the executable. LLDB must be on
`PATH`; the supported native debugging path is Linux and macOS. Debugging
requires a trusted workspace.

A launch configuration can pass arguments, a working directory, environment
overrides, and `stopOnEntry`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "jet",
      "request": "launch",
      "name": "Jet: Launch",
      "program": "${file}",
      "args": [],
      "cwd": "${workspaceFolder}",
      "stopOnEntry": true,
      "showRawFrames": false
    }
  ]
}
```

For attach, `program` is the native debug binary, `map` must name its matching
`<executable>.jetmap` sidecar, and `processId` is the local same-user process
id. The extension reads the sidecar first and resolves its `jet_file` source
path relative to the sidecar, not the editor process's current directory:

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

The adapter verifies same-user ownership and executable/build identity before
attaching. The sidecar's source and generated-file hashes must also match the
requested debug inputs, so a replaced binary or edited map is rejected. Set
`showRawFrames` only when generated native frames and scopes are useful; the
default projection stays in Jet terms. DAP messages use strict
`Content-Length` framing with a 16 MiB maximum frame, and adapter identity is
`jet`. See [`crates/jet-debug`](../../../crates/jet-debug/) for the native adapter
implementation.

## Source highlighting

TextMate grammar sources live in
[`Tools/editors/vscode/syntaxes`](syntaxes/); their lexical vocabulary is generated
from [`Syntax.rs`](../../../crates/jet-foundation/src/Syntax.rs). Semantic tokens
refine live coloring for ownership markers, rules, decorators, and effect
rows. The editor surface is not a substitute for the compiler's parser: use
feature sources and the language specification for accepted syntax.

To regenerate the checked-in grammar while working on the repository:

```sh
Tools/agent/jet-env cargo run --bin jet -- self devtools grammars
```

## Focused checks

From the repository root, the language-server test and deterministic benchmark
can be run with:

```sh
Tools/agent/jet-env cargo test --test lsp
Tools/agent/jet-env target/debug/jet self lsp --bench
```

The benchmark reports cold, warm-hit, and warm-edit measurements plus cache
counters; its timings are measurements, not a wall-clock pass/fail contract.
