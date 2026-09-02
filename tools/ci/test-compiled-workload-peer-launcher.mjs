#!/usr/bin/env node
// Focused #1414 proof for the native peer boundary. This is intentionally a
// standalone test: it compiles the launcher with the host rustc, then probes
// the exact filesystem, network, process-tree, and result contracts.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const source = path.join(root, "tools", "ci", "compiled-workload-peer-launcher.rs");
const scratchBase = path.join(os.homedir(), ".cache", "jet-test-scratch");
fs.mkdirSync(scratchBase, { recursive: true });
const runRoot = fs.mkdtempSync(path.join(scratchBase, "compiled-workload-peer-launcher-"));
const launcher = path.join(
  runRoot,
  process.platform === "win32" ? "compiled-workload-peer-launcher.exe" : "compiled-workload-peer-launcher",
);
const workload = path.join(runRoot, "probe.mjs");
const outside = path.join(path.dirname(runRoot), `${path.basename(runRoot)}-outside`);
const outsideFile = path.join(outside, "marker");

function fail(message) {
  throw new Error(`peer launcher self-test: ${message}`);
}
function command(name) {
  const pathValue = process.env.PATH || "";
  for (const directory of pathValue.split(path.delimiter)) {
    const candidate = path.join(directory, name);
    if (fs.existsSync(candidate)) return candidate;
    if (process.platform === "win32") {
      for (const suffix of [".exe", ".cmd", ".bat"]) {
        const withSuffix = candidate + suffix;
        if (fs.existsSync(withSuffix)) return withSuffix;
      }
    }
  }
  return "";
}
function run(program, args, options = {}) {
  return spawnSync(program, args, {
    cwd: runRoot,
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
    ...options,
  });
}
function assertRejected(result, text) {
  if (!result.error && result.status === 0) fail(`expected rejection containing ${text}`);
  const output = `${result.stderr || ""}${result.stdout || ""}`;
  if (!output.includes(text)) fail(`rejection did not name ${text}: ${output}`);
}
function args(policy, command, commandArgs = []) {
  return [
    `--contract=compiled-workload-peer-isolation-v1`,
    "--task-id",
    "peer-launcher-self-test",
    "--root",
    runRoot,
    "--cwd",
    runRoot,
    "--network",
    policy,
    "--external-write",
    "disabled",
    "--host",
    "ambient",
    "--",
    command,
    ...commandArgs,
  ];
}

try {
  fs.mkdirSync(scratchBase, { recursive: true });
  fs.rmSync(outside, { recursive: true, force: true });
  fs.mkdirSync(outside);
  const rustc = command("rustc");
  if (!rustc) fail("rustc is not available to build the launcher");
  const build = run(rustc, ["--edition=2021", "-O", source, "-o", launcher], {
    cwd: root,
    stdio: "inherit",
  });
  if (build.status !== 0) fail(`launcher compilation failed with ${build.status}`);

  const contract = spawnSync(launcher, ["--contract"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
  });
  const primitiveAvailable = contract.status === 0
    && (contract.stdout || "").trim() === "compiled-workload-peer-isolation-v1";
  if (!primitiveAvailable) {
    // An unavailable host must still prove the fail-closed contract. Linux and
    // Windows hide their discovered primitive behind PATH; this catches a
    // missing bwrap/netsh without ever launching an unisolated child.
    const missingPath = spawnSync(launcher, ["--contract"], {
      cwd: root,
      env: { ...process.env, PATH: "" },
      encoding: "utf8",
      maxBuffer: 1024 * 1024,
    });
    if (process.platform === "linux") assertRejected(missingPath, "bubblewrap");
    if (process.platform === "win32") assertRejected(missingPath, "unavailable");
    const escapeArgs = args("disabled", process.execPath, [workload]);
    escapeArgs[6] = outside;
    const escape = run(launcher, escapeArgs, {
      env: { ...process.env, PATH: "" },
    });
    assertRejected(escape, "cwd");
    console.log(`peer launcher self-test: fail-closed unavailable host (${(contract.stderr || "").trim()})`);
    fs.rmSync(runRoot, { recursive: true, force: true });
    fs.rmSync(outside, { recursive: true, force: true });
    process.exit(0);
  }
  if (contract.stderr || contract.stdout.trim() !== "compiled-workload-peer-isolation-v1") {
    fail(`contract output is not exact: stdout=${contract.stdout} stderr=${contract.stderr}`);
  }

  fs.writeFileSync(
    workload,
    `import fs from "node:fs";
import net from "node:net";
import { spawn } from "node:child_process";
const mode = process.argv[2];
const outside = process.env.PEER_OUTSIDE;
const marker = process.env.PEER_MARKER;
const print = value => fs.writeSync(1, String(value) + "\\n");
if (mode === "write") {
  fs.writeFileSync("inside", "inside\\n");
  try { fs.writeFileSync(outside, "outside\\n"); print("outside=allowed"); }
  catch { print("outside=blocked"); }
} else if (mode === "network") {
  const connect = (host, port) => new Promise(resolve => {
    const socket = new net.Socket();
    let done = false;
    const finish = value => { if (!done) { done = true; socket.destroy(); resolve(value); } };
    socket.on("error", () => finish(false));
    socket.setTimeout(350, () => finish(false));
    socket.on("connect", () => finish(true));
    socket.connect(port, host);
  });
  const loopback = await new Promise(resolve => {
    const server = net.createServer(socket => {
      socket.on("error", () => {});
      socket.end("ok");
    });
    server.once("error", () => resolve(false));
    server.listen(0, "127.0.0.1", async () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      resolve(port > 0 && await connect("127.0.0.1", port));
      server.close();
    });
  });
  const external = await connect("198.51.100.1", 9);
  print(loopback ? "loopback=allowed" : "loopback=blocked");
  print(external ? "external=allowed" : "external=blocked");
} else if (mode === "descendant") {
  spawn(process.execPath, [process.argv[1], "delayed"], { env: { ...process.env }, stdio: "ignore" });
  print("spawned");
  process.exit(0);
} else if (mode === "delayed") {
  setTimeout(() => fs.writeFileSync(marker, "survived\\n"), 600);
} else if (mode === "signal") {
  process.kill(process.pid, "SIGTERM");
} else if (mode === "result") {
  fs.writeSync(1, "stdout-exact\\n");
  fs.writeSync(2, "stderr-exact\\n");
  process.exitCode = 23;
} else {
  process.exit(91);
}
`,
  );

  const write = run(launcher, args("disabled", process.execPath, [workload, "write"]), {
    env: { ...process.env, PEER_OUTSIDE: outsideFile },
  });
  if (write.status !== 0 || write.stdout !== "outside=blocked\n") {
    fail(`write policy mismatch: status=${write.status} stdout=${write.stdout} stderr=${write.stderr}`);
  }
  if (!fs.existsSync(path.join(runRoot, "inside")) || fs.existsSync(outsideFile)) {
    fail("write policy did not confine the writable tree");
  }

  const disabled = run(launcher, args("disabled", process.execPath, [workload, "network"]));
  if (disabled.status !== 0 || disabled.stdout !== "loopback=blocked\nexternal=blocked\n") {
    fail(`disabled network mismatch: status=${disabled.status} stdout=${disabled.stdout} stderr=${disabled.stderr}`);
  }
  const loopback = run(launcher, args("loopback-only", process.execPath, [workload, "network"]));
  if (loopback.status !== 0 || loopback.stdout !== "loopback=allowed\nexternal=blocked\n") {
    fail(`loopback-only network mismatch: status=${loopback.status} stdout=${loopback.stdout} stderr=${loopback.stderr}`);
  }

  const escapeArgs = args("disabled", process.execPath, [workload, "result"]);
  escapeArgs[6] = outside;
  const escape = run(launcher, escapeArgs);
  assertRejected(escape, "cwd");

  const authority = run(launcher, [
    "--contract=compiled-workload-peer-isolation-v1",
    "--task-id",
    "peer-launcher-self-test",
    "--root",
    runRoot,
    "--cwd",
    runRoot,
    "--network",
    "internet",
    "--external-write",
    "disabled",
    "--host",
    "ambient",
    "--",
    process.execPath,
    workload,
  ]);
  assertRejected(authority, "unsupported network authority");

  const result = run(launcher, args("disabled", process.execPath, [workload, "result"]));
  if (result.status !== 23 || result.stdout !== "stdout-exact\n" || result.stderr !== "stderr-exact\n") {
    fail(`result propagation mismatch: status=${result.status} stdout=${result.stdout} stderr=${result.stderr}`);
  }

  const marker = path.join(runRoot, "descendant-marker");
  const descendant = run(launcher, args("disabled", process.execPath, [workload, "descendant"]), {
    env: { ...process.env, PEER_MARKER: marker },
  });
  if (descendant.status !== 0 || descendant.stdout !== "spawned\n") {
    fail(`descendant launch mismatch: status=${descendant.status} stdout=${descendant.stdout}`);
  }
  await new Promise(resolve => setTimeout(resolve, 800));
  if (fs.existsSync(marker)) fail("descendant survived launcher exit");

  if (process.platform !== "win32") {
    const signal = run(launcher, args("disabled", process.execPath, [workload, "signal"]));
    if (signal.signal !== "SIGTERM") fail(`signal propagation mismatch: ${JSON.stringify(signal)}`);
  }
  const missing = spawnSync(launcher, ["--contract"], {
    cwd: root,
    env: { ...process.env, PATH: "" },
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
  });
  if (process.platform === "linux") assertRejected(missing, "bubblewrap");
  if (process.platform === "win32") assertRejected(missing, "unavailable");
  console.log("peer launcher self-test: filesystem, network, path, authority, result, signal, and descendant checks passed");
} finally {
  fs.rmSync(runRoot, { recursive: true, force: true });
  fs.rmSync(outside, { recursive: true, force: true });
}
