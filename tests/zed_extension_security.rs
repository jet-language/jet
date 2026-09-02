//! Structural security checks for the Zed extension.
//!
//! Zed owns worktree trust and executes the compiled extension outside the
//! Rust integration-test harness. Keep this check beside the other extension
//! configuration checks so a future edit cannot restore worktree PATH lookup.

mod common;

use std::fs;
use std::path::PathBuf;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process::Command;


fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn artifact_contains(artifact: &[u8], needle: &[u8]) -> bool {
    artifact.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn zed_extension_keeps_hostile_worktree_path_out_of_the_server_command() {
    let root = repo_root().join("editors/zed");
    let source = fs::read_to_string(root.join("wasm-src/src/lib.rs"))
        .expect("Zed extension source must be present");
    let manifest = fs::read_to_string(root.join("extension.toml.in"))
        .expect("Zed extension manifest template must be present");
    let readme = fs::read_to_string(root.join("README.md")).expect("Zed README must be present");
    let artifact = fs::read(root.join("extension.wasm"))
        .expect("tracked Zed extension artifact must be present");

    // A hostile worktree may put `jet` earlier on the worktree PATH. The
    // extension must return the literal command identity that the manifest
    // approves; it must never ask Zed for an executable path from Worktree.
    assert!(
        source.contains(
            "command: \"jet\".to_string(),\n            args: vec![\"self\".to_string(), \"lsp\".to_string()]"
        ),
        "{source}"
    );
    assert!(!source.contains("worktree.which"), "worktree PATH lookup returned: {source}");
    assert!(!source.contains("shell_env"), "worktree environment lookup returned: {source}");
    assert!(!source.contains("root_path"), "worktree path lookup returned: {source}");
    assert!(manifest.contains("kind = \"process:exec\""), "{manifest}");
    assert!(manifest.contains("command = \"jet\""), "{manifest}");
    assert!(manifest.contains("args = [\"self\", \"lsp\"]"), "{manifest}");
    assert!(
        !artifact_contains(&artifact, b"worktree.which"),
        "tracked artifact still embeds worktree PATH lookup"
    );
    assert!(
        !artifact_contains(&artifact, b"/target/debug/jet"),
        "tracked artifact still embeds a worktree debug-binary fallback"
    );

    // The trust decision is made by Zed. Keep the operational contract
    // visible to users rather than implying that source-tree files are trust.
    assert!(readme.contains("worktree-trust gate"), "{readme}");
    assert!(readme.contains("does not call\n`Worktree::which`"), "{readme}");
}

#[cfg(unix)]
#[test]
fn vscode_untrusted_workspace_rejects_hostile_binary_and_shell_launches() {
    let scratch = common::Scratch::new("vscode-hostile-workspace");
    let workspace = scratch.join("untrusted workspace;$(touch vscode-injected)");
    let debug_dir = workspace.join("target/debug");
    fs::create_dir_all(&debug_dir).expect("create hostile workspace target");
    let binary = debug_dir.join("jet");
    fs::write(
        &binary,
        r#"#!/bin/sh
printf 'executed\n' > "$VSCODE_BINARY_MARKER"
"#,
    )
    .expect("write hostile workspace binary");
    let mut permissions = fs::metadata(&binary)
        .expect("stat hostile workspace binary")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&binary, permissions).expect("make hostile workspace binary executable");

    let run_file = workspace.join("src/hostile file.jet");
    fs::create_dir_all(run_file.parent().expect("run file parent"))
        .expect("create hostile workspace source directory");
    fs::write(&run_file, "fn run() {}\n").expect("write hostile workspace source");
    let binary_marker = scratch.join("vscode-binary-executed");
    let injection_marker = scratch.join("vscode-injected");
    let node_harness = r#"
    const fs = require("fs");
    const path = require("path");
    const Module = require("module");

    const workspace = process.env.WORKSPACE;
    const workspaceBinary = process.env.WORKSPACE_BINARY;
    const runFile = process.env.RUN_FILE;
    const clients = [];
    const terminals = [];
    const commands = new Map();
    let debugProvider;
    let debugFactory;
    let debugStarts = 0;
    let debugDescriptors = 0;

    function resolveCommand(command) {
      if (path.isAbsolute(command) || command.includes(path.sep)) {
        return command;
      }
      for (const directory of (process.env.PATH || "").split(path.delimiter)) {
        if (!directory) {
          continue;
        }
        const candidate = path.join(directory, command);
        try {
          fs.accessSync(candidate, fs.constants.X_OK);
          return candidate;
        } catch (_) {
          // Match the OS lookup: keep searching when this PATH entry is absent
          // or not executable.
        }
      }
      return command;
    }
const vscode = {
  workspace: {
    isTrusted: false,
    workspaceFolders: [{ uri: { fsPath: workspace } }],
    getConfiguration() {
      return {
        get(key) {
          if (key === "executablePath" || key === "languageServerPath") {
            return workspaceBinary;
          }
          return "";
        },
      };
    },
    createFileSystemWatcher() {
      return { dispose() {} };
    },
  },
  window: {
    createTerminal(options) {
      terminals.push(options);
      if (resolveCommand(options.shellPath) === workspaceBinary) {
        require("child_process").execFileSync(resolveCommand(options.shellPath), {
          env: process.env,
        });
      }
      return { show() {} };
    },
    showWarningMessage() {},
    showErrorMessage() {},
  },
  commands: {
    registerCommand(name, handler) {
      commands.set(name, handler);
      return { dispose() {} };
    },
  },
  debug: {
    registerDebugConfigurationProvider(_id, provider) {
      debugProvider = provider;
      return { dispose() {} };
    },
    registerDebugAdapterDescriptorFactory(_id, factory) {
      debugFactory = factory;
      return { dispose() {} };
    },
    startDebugging() {
      debugStarts += 1;
    },
  },
  Uri: {
    parse() {
      return { fsPath: runFile };
    },
    file(value) {
      return { fsPath: value };
    },
  },
  DebugAdapterExecutable: class {
    constructor(command, args, options) {
      debugDescriptors += 1;
      this.command = command;
      this.args = args;
      this.options = options;
    }
  },
};

const originalLoad = Module._load;
Module._load = function(request, parent, isMain) {
  if (request === "vscode") {
    return vscode;
  }
  if (request === "vscode-languageclient/node") {
    return {
      TransportKind: { stdio: "stdio" },
      LanguageClient: class {
        constructor(id, name, server, clientOptions) {
          this.id = id;
          this.name = name;
          this.server = server;
          this.clientOptions = clientOptions;
          clients.push(this);
        }
        start() {
          const command = resolveCommand(this.server.command);
          if (command === workspaceBinary) {
            require("child_process").execFileSync(command, {
              env: process.env,
            });
          }
          return Promise.resolve();
        }
        stop() {
          return Promise.resolve();
        }
      },
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};

const manifest = JSON.parse(fs.readFileSync(process.env.PACKAGE, "utf8"));
const supported = manifest.capabilities?.untrustedWorkspaces?.supported;
if (supported !== false) {
  throw new Error("VS Code must disable activation for untrusted workspaces");
}

const extension = require(process.env.EXTENSION);
extension.activate({
  subscriptions: {
    push() {},
  },
});
const runFileCommand = commands.get("jet.runFile");
const testFileCommand = commands.get("jet.testFile");
if (typeof runFileCommand !== "function" || typeof testFileCommand !== "function") {
  throw new Error("Jet run/test commands were not registered");
}
runFileCommand("file:hostile");
testFileCommand("file:hostile");
const debugFileCommand = commands.get("jet.debugFile");
if (typeof debugFileCommand !== "function" || !debugProvider || !debugFactory) {
  throw new Error("Jet debug command/provider/factory were not registered");
}
debugFileCommand("file:hostile");
if (debugProvider.resolveDebugConfiguration(undefined, { program: runFile }) !== undefined) {
  throw new Error("untrusted activation resolved a debug configuration");
}
try {
  debugFactory.createDebugAdapterDescriptor({
    configuration: { request: "launch", program: runFile },
    workspaceFolder: { uri: { fsPath: workspace } },
  });
  throw new Error("untrusted activation created a debug descriptor");
} catch (error) {
  if (!String((error && error.message) || error).includes("trusted workspace")) {
    throw error;
  }
}

if (
  clients.length !== 0 ||
  terminals.length !== 0 ||
  debugStarts !== 0 ||
  debugDescriptors !== 0
) {
  throw new Error("untrusted activation launched or described a process");
}

process.stdout.write([
  `manifest-untrusted-supported=${String(supported)}`,
  `lsp-launches=${clients.length}`,
  `terminal-launches=${terminals.length}`,
  `debug-starts=${debugStarts}`,
  `debug-descriptors=${debugDescriptors}`,
].join("\n") + "\n");
"#;
    let path_env = format!(
        "{}:{}",
        debug_dir.display(),
        std::env::var_os("PATH")
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let output = Command::new("node")
        .current_dir(&scratch.path)
        .arg("-e")
        .arg(node_harness)
        .env("WORKSPACE", &workspace)
        .env("WORKSPACE_BINARY", &binary)
        .env("RUN_FILE", &run_file)
        .env("VSCODE_BINARY_MARKER", &binary_marker)
        .env("EXTENSION", repo_root().join("editors/vscode/extension.js"))
        .env("PACKAGE", repo_root().join("editors/vscode/package.json"))
        .env("PATH", path_env)
        .output()
        .expect("run VS Code extension with deterministic mocks");
    assert!(
        output.status.success(),
        "VS Code hostile-workspace harness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected =
        "manifest-untrusted-supported=false\n\
lsp-launches=0\n\
terminal-launches=0\n\
debug-starts=0\n\
debug-descriptors=0\n";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "untrusted workspace must refuse LSP and terminal process launches"
    );
    assert!(
        !binary_marker.exists(),
        "workspace-controlled jet binary was executed"
    );
    assert!(
        !injection_marker.exists(),
        "workspace path was reparsed as shell syntax"
    );
}
