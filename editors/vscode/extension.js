// Jet VS Code extension — LSP v0 client (M6 phase 4). Plain JS, no build step.
//
// Server discovery, in order:
//   1. jet.languageServerPath setting (supports ${workspaceFolder} and ~)
//   2. <workspaceFolder>/target/debug/jet   (developing the compiler itself)
//   3. `jet` on PATH                        (installed, or editor launched from dev shell)
// `jet lsp` does not invoke rustc, so the plain cargo binary is enough.

const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

/** @type {LanguageClient | undefined} */
let client;

function expandPathSetting(value, workspaceFolder) {
  return value
    .replace(/\$\{workspaceFolder\}/g, workspaceFolder)
    .replace(/\$\{workspaceRoot\}/g, workspaceFolder)
    .replace(/^~(?=$|\/|\\)/, process.env.HOME || "~");
}

function findServer(workspaceFolder) {
  const configured = vscode.workspace
    .getConfiguration("jet")
    .get("languageServerPath", "");

  if (configured) {
    const expanded = expandPathSetting(configured, workspaceFolder || "");
    if (fs.existsSync(expanded)) {
      return expanded;
    }
    vscode.window.showWarningMessage(
      `jet.languageServerPath is set to "${expanded}" but nothing exists there; falling back to auto-discovery.`
    );
  }

  if (workspaceFolder) {
    const debugBin = path.join(workspaceFolder, "target", "debug", "jet");
    if (fs.existsSync(debugBin)) {
      return debugBin;
    }
  }

  // Bare command: the OS resolves it from PATH when the server spawns.
  return "jet";
}

function activate(context) {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const serverPath = findServer(workspaceFolder);

  client = new LanguageClient(
    "jet",
    "Jet Language Server",
    {
      command: serverPath,
      args: ["lsp"],
      options: { cwd: workspaceFolder },
      transport: TransportKind.stdio,
    },
    {
      documentSelector: [{ scheme: "file", language: "jet" }],
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher("**/*.jet"),
      },
    }
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("jet.restartServer", async () => {
      if (client) {
        await client.restart();
      }
    })
  );

  client.start().catch(() => {
    vscode.window.showErrorMessage(
      `Jet language server failed to start (tried: ${serverPath}). ` +
        `Build it with \`nix develop -c cargo build\` in the jet repo, ` +
        `or set jet.languageServerPath to a jet binary.`
    );
  });
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };
