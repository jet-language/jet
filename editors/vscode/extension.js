// Jet VS Code extension — LSP v0 client (M6 phase 4). Plain JS, no build step.
//
// Server discovery, in order:
//   1. jet.executablePath setting (supports ${workspaceFolder} and ~)
//   2. legacy jet.languageServerPath setting
//   3. <workspaceFolder>/target/debug/jet   (developing the compiler itself, trusted workspaces only)
//   4. `jet` on PATH                        (installed, or editor launched from dev shell)
// `jet self lsp` does not invoke rustc, so the plain cargo binary is enough.

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
  const settings = vscode.workspace.getConfiguration("jet");
  // Workspace settings and workspace binaries are both untrusted until the
  // editor has crossed its workspace-trust boundary.
  const explicit = vscode.workspace.isTrusted ? settings.get("executablePath", "") : "";
  const legacy = vscode.workspace.isTrusted ? settings.get("languageServerPath", "") : "";
  const configured = explicit || legacy;
  const configuredName = explicit ? "jet.executablePath" : "jet.languageServerPath";

  if (configured) {
    const expanded = expandPathSetting(configured, workspaceFolder || "");
    if (fs.existsSync(expanded)) {
      return expanded;
    }
    vscode.window.showWarningMessage(
      `${configuredName} is set to "${expanded}" but nothing exists there; falling back to auto-discovery.`
    );
  }

  if (vscode.workspace.isTrusted && workspaceFolder) {
    const debugBin = path.join(workspaceFolder, "target", "debug", "jet");
    if (fs.existsSync(debugBin)) {
      return debugBin;
    }
  }

  // Bare command: the OS resolves it from PATH when the server spawns.
  return "jet";
}

function canonicalProgram(program) {
  const absolute = path.resolve(program);
  return fs.existsSync(absolute) ? fs.realpathSync(absolute) : absolute;
}

function debugSourceForConfiguration(configuration) {
  if (configuration.request !== "attach") {
    return canonicalProgram(configuration.program);
  }
  if (!configuration.map) {
    throw new Error("Jet attach needs the existing `map` sidecar to identify its .jet source.");
  }
  let map;
  const mapPath = canonicalProgram(configuration.map);
  try {
    map = JSON.parse(fs.readFileSync(mapPath, "utf8"));
  } catch (error) {
    throw new Error(`Jet attach cannot read its verified map sidecar: ${error.message}`);
  }
  if (typeof map.jet_file !== "string" || !map.jet_file) {
    throw new Error("Jet attach map sidecar does not identify a Jet source file.");
  }
  // The sidecar records the source path used by the build. Relative paths are
  // relative to the sidecar, not to the editor process's current directory.
  const source = path.isAbsolute(map.jet_file)
    ? map.jet_file
    : path.resolve(path.dirname(mapPath), map.jet_file);
  return canonicalProgram(source);
}

function shellQuote(value) {
  return `"${String(value).replace(/(["\\$`])/g, "\\$1")}"`;
}

function uriArgToPath(uriArg) {
  if (typeof uriArg === "string" && uriArg.startsWith("file:")) {
    return vscode.Uri.parse(uriArg).fsPath;
  }
  return vscode.window.activeTextEditor?.document.uri.fsPath;
}

function runJetInTerminal(serverPath, args) {
  const terminal = vscode.window.createTerminal("Jet");
  terminal.show();
  terminal.sendText([shellQuote(serverPath), ...args.map(shellQuote)].join(" "));
}

function debugFile(file) {
  if (!debuggingIsAllowed()) {
    return;
  }
  const uri = vscode.Uri.file(canonicalProgram(file));
  const folder = vscode.workspace.getWorkspaceFolder(uri);
  vscode.debug.startDebugging(folder, {
    type: "jet",
    request: "launch",
    name: "Jet: Debug File",
    program: canonicalProgram(file),
  });
}

function activate(context) {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const serverPath = findServer(workspaceFolder);
  const debugProvider = {
    resolveDebugConfiguration(_folder, config) {
      if (!debuggingIsAllowed()) {
        return undefined;
      }
      const program = config.program || vscode.window.activeTextEditor?.document.uri.fsPath;
      if (!program) {
        vscode.window.showErrorMessage("Jet debugger needs an open .jet file.");
        return undefined;
      }
      return {
        ...config,
        type: "jet",
        request: config.request || "launch",
        name: config.name || "Jet: Debug File",
        program: canonicalProgram(program),
      };
    },
  };
  const debugFactory = {
    createDebugAdapterDescriptor(session) {
      if (!debuggingIsAllowed()) {
        throw new Error("Jet debugging requires a trusted workspace.");
      }
      const program = debugSourceForConfiguration(session.configuration);
      const cwd = session.workspaceFolder?.uri.fsPath || workspaceFolder;
      return new vscode.DebugAdapterExecutable(
        serverPath,
        ["debug", "--dap", program],
        cwd ? { cwd } : undefined
      );
    },
  };

  client = new LanguageClient(
    "jet",
    "Jet Language Server",
    {
      command: serverPath,
      args: ["self", "lsp"],
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
    }),
    vscode.commands.registerCommand("jet.runFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        runJetInTerminal(serverPath, ["run", file]);
      }
    }),
    vscode.commands.registerCommand("jet.testFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        runJetInTerminal(serverPath, ["test", file]);
      }
    }),
    vscode.commands.registerCommand("jet.debugFile", (uriArg) => {
      const file = uriArgToPath(uriArg);
      if (file) {
        debugFile(file);
      }
    }),
    vscode.debug.registerDebugConfigurationProvider("jet", debugProvider),
    vscode.debug.registerDebugAdapterDescriptorFactory("jet", debugFactory)
  );

  client.start().catch(() => {
    vscode.window.showErrorMessage(
      `Jet language server failed to start (tried: ${serverPath}). ` +
        `Build it with \`nix develop -c cargo build\` in the jet repo, ` +
        `or set jet.languageServerPath to a jet binary.`
    );
  });
}

function debuggingIsAllowed() {
  if (vscode.workspace.isTrusted) {
    return true;
  }
  vscode.window.showErrorMessage("Jet debugging requires a trusted workspace.");
  return false;
}

function deactivate() {
  if (client) {
    return client.stop();
  }
}

module.exports = { activate, deactivate };
