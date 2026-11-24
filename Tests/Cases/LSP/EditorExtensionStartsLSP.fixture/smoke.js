// Activates the VS Code extension with no settings, standing in for the
// editor's own modules, then drives the language server it launches: the
// server answers initialize, and an opened file gets its diagnostics.
const fs = require("fs");
const Module = require("module");
const path = require("path");
const { spawn } = require("child_process");

const work = process.cwd();
const uri = `file://${path.join(work, "main.jet")}`;
const ignored = () => ({ dispose() {} });

class LanguageClient {
  constructor(_id, _name, serverOptions, clientOptions) {
    this.serverOptions = serverOptions;
    this.clientOptions = clientOptions;
  }

  start() {
    return drive(this.serverOptions, this.clientOptions);
  }

  stop() {
    return Promise.resolve();
  }
}

const vscode = {
  workspace: {
    isTrusted: true,
    workspaceFolders: [{ uri: { fsPath: work } }],
    // Every setting reads as unset.
    getConfiguration: () => ({ get: (_key, fallback) => fallback }),
    createFileSystemWatcher: ignored,
    onDidChangeTextDocument: ignored,
    textDocuments: [],
  },
  window: {
    showErrorMessage: (message) => console.log(`error: ${message}`),
    showWarningMessage: (message) => console.log(`warning: ${message}`),
  },
  commands: { registerCommand: ignored },
  debug: {
    registerDebugConfigurationProvider: ignored,
    registerDebugAdapterDescriptorFactory: ignored,
  },
  ViewColumn: { Beside: -2 },
  Uri: { parse: (text) => ({ fsPath: new URL(text).pathname }) },
};

const standIns = {
  vscode,
  "vscode-languageclient/node": { LanguageClient, TransportKind: { stdio: 0 } },
};
const load = Module._load;
Module._load = function (request, parent, isMain) {
  if (Object.hasOwn(standIns, request)) {
    return standIns[request];
  }
  return load.call(this, request, parent, isMain);
};

function drive(serverOptions, clientOptions) {
  console.log(`launch: ${serverOptions.command} ${serverOptions.args.join(" ")}`);
  console.log(`cwd: ${serverOptions.options.cwd === work ? "workspace folder" : serverOptions.options.cwd}`);
  console.log(`language: ${clientOptions.documentSelector.map((selector) => selector.language).join(",")}`);
  return new Promise((resolve, reject) => {
    const server = spawn(serverOptions.command, serverOptions.args, {
      cwd: serverOptions.options.cwd,
      stdio: ["pipe", "pipe", "inherit"],
    });
    const send = (message) => {
      const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }));
      server.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
      server.stdin.write(body);
    };
    const receive = (message) => {
      if (message.id === 1) {
        console.log(`initialize: ${message.result ? "answered" : JSON.stringify(message.error)}`);
        send({ method: "initialized", params: {} });
        send({
          method: "textDocument/didOpen",
          params: {
            textDocument: { uri, languageId: "jet", version: 1, text: fs.readFileSync("main.jet", "utf8") },
          },
        });
      } else if (message.method === "textDocument/publishDiagnostics" && message.params.uri === uri) {
        if (message.params.diagnostics.length === 0) {
          return;
        }
        for (const diagnostic of message.params.diagnostics) {
          const start = diagnostic.range.start;
          console.log(`diagnostic: ${start.line + 1}:${start.character + 1} ${diagnostic.code}`);
        }
        send({ id: 2, method: "shutdown" });
      } else if (message.id === 2) {
        send({ method: "exit" });
        server.stdin.end();
      }
    };
    let buffer = Buffer.alloc(0);
    server.stdout.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      for (;;) {
        const head = buffer.indexOf("\r\n\r\n");
        if (head < 0) {
          return;
        }
        const length = Number(/Content-Length: *(\d+)/i.exec(buffer.subarray(0, head).toString())[1]);
        if (buffer.length < head + 4 + length) {
          return;
        }
        receive(JSON.parse(buffer.subarray(head + 4, head + 4 + length).toString()));
        buffer = buffer.subarray(head + 4 + length);
      }
    });
    server.on("error", reject);
    server.on("exit", (code) => {
      console.log(`server exit: ${code}`);
      resolve();
    });
    send({ id: 1, method: "initialize", params: { processId: process.pid, rootUri: `file://${work}`, capabilities: {} } });
  });
}

require(path.join(work, "extension.js")).activate({ subscriptions: [] });
