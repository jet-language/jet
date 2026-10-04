// Native behavior oracle for the Jet launcher. This does not compile or invoke
// Rust, and an unavailable primitive is a failure, never isolation parity.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import net from "node:net";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const self = fileURLToPath(import.meta.url);
const mode = process.argv[2];
const print = value => fs.writeSync(1, `${value}\n`);
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
function check(condition, message) { if (!condition) throw new Error(message); }

// The workload executes inside the real isolation boundary.
if (mode === "write") {
  fs.writeFileSync("inside", "inside\n");
  try { fs.writeFileSync(process.env.PEER_OUTSIDE, "outside\n"); print("outside=allowed"); }
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
    const server = net.createServer(socket => { socket.on("error", () => {}); socket.end("ok"); });
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
  spawn(process.execPath, [self, "delayed"], { env: { ...process.env }, stdio: "ignore" });
  print("spawned");
  process.exit(0);
} else if (mode === "delayed") {
  setTimeout(() => fs.writeFileSync(process.env.PEER_MARKER, "survived\n"), 600);
} else if (mode === "interrupted-delayed") {
  setTimeout(() => fs.writeFileSync(process.env.PEER_MARKER, "survived\n"), 1500);
} else if (mode === "signal") {
  process.kill(process.pid, "SIGTERM");
} else if (mode === "result") {
  fs.writeSync(1, "stdout-exact\n");
  fs.writeSync(2, "stderr-exact\n");
  process.exitCode = 23;
} else if (mode === "argv-env") {
  print(JSON.stringify(process.argv.slice(3)));
  print(process.env.PEER_INHERITED);
  print(process.cwd());
} else if (mode === "interrupt-worker") {
  process.on("SIGINT", () => fs.writeFileSync("first-signal", "SIGINT\n"));
  spawn(process.execPath, [self, "interrupted-delayed"], { env: { ...process.env }, stdio: "ignore" });
  fs.writeFileSync("ready", "ready\n");
  // An oracle failure must not leave an immortal worker behind.
  setTimeout(() => process.exit(91), 4000);
} else if (mode === "test") {
  const scenario = process.argv[3];
  const configured = process.argv[4];
  check(configured && path.isAbsolute(configured), "JET_COMPILED_WORKLOAD_PEER_LAUNCHER must name the real built Jet executable");
  const launcher = fs.realpathSync(configured);
  const base = path.join(os.homedir(), ".cache", "jet-test-scratch");
  fs.mkdirSync(base, { recursive: true });
  const root = fs.mkdtempSync(path.join(base, "jet-peer-launcher-test-"));
  const outside = `${root}-outside`;
  fs.mkdirSync(outside);
  // Put the real workload under the writable root for Windows ACL projection.
  const workload = path.join(root, "workload.mjs");
  fs.copyFileSync(self, workload);
  let interruptChild;
  const argv = (policy, command = process.execPath, extra = []) => [
    "--contract=compiled-workload-peer-isolation-v1", "--task-id", "peer-launcher-self-test",
    "--root", root, "--cwd", root, "--network", policy,
    "--external-write", "disabled", "--host", "ambient", "--", command, ...extra,
  ];
  const run = (args, options = {}) => spawnSync(launcher, args, {
    cwd: root, encoding: "utf8", maxBuffer: 1024 * 1024, timeout: 10000, ...options,
  });
  const rejected = (result, text) => {
    check(!result.error && result.status === 78, `expected exit 78: ${JSON.stringify(result)}`);
    check(`${result.stderr || ""}${result.stdout || ""}`.includes(text), `rejection did not name ${text}`);
  };
  try {
    const version = run(["--version"]);
    check(version.status === 0 && version.stdout === "compiled-workload-peer-isolation-v1\n" && !version.stderr, "version contract differs");
    if (scenario === "missing-primitive") {
      const missing = run(["--contract"], { env: { ...process.env, PATH: "" } });
      if (process.platform === "linux") rejected(missing, "bubblewrap");
      else if (process.platform === "win32") rejected(missing, "unavailable");
      else {
        const contract = run(["--contract"]);
        check(contract.status === 0 && contract.stdout === version.stdout && !contract.stderr, "Seatbelt capability unavailable");
      }
    } else {
      const contract = run(["--contract"]);
      check(!contract.error && contract.status === 0 && contract.stdout === version.stdout && !contract.stderr,
        `native provider unavailable; NOT an isolation pass: ${contract.stderr}`);
      if (scenario === "contract") {
        // The exact primitive probe above is this test's assertion.
      } else if (scenario === "write") {
        const marker = path.join(outside, "marker");
        const result = run(argv("disabled", process.execPath, [workload, "write"]), { env: { ...process.env, PEER_OUTSIDE: marker } });
        check(result.status === 0 && result.stdout === "outside=blocked\n", `write policy differs: ${JSON.stringify(result)}`);
        check(fs.existsSync(path.join(root, "inside")) && !fs.existsSync(marker), "writable tree escaped");
      } else if (scenario === "disabled" || scenario === "loopback-only") {
        const policy = scenario === "disabled" ? "disabled" : "loopback-only";
        const result = run(argv(policy, process.execPath, [workload, "network"]));
        const expected = policy === "disabled" ? "loopback=blocked\nexternal=blocked\n" : "loopback=allowed\nexternal=blocked\n";
        check(result.status === 0 && result.stdout === expected, `network policy differs: ${JSON.stringify(result)}`);
      } else if (scenario === "escape") {
        const args = argv("disabled", process.execPath, [workload, "result"]);
        args[6] = outside;
        rejected(run(args), "cwd");
      } else if (scenario === "authority") {
        rejected(run(argv("internet", process.execPath, [workload])), "unsupported network authority");
      } else if (scenario === "result") {
        const result = run(argv("disabled", process.execPath, [workload, "result"]));
        check(result.status === 23 && result.stdout === "stdout-exact\n" && result.stderr === "stderr-exact\n", `result propagation differs: ${JSON.stringify(result)}`);
      } else if (scenario === "descendant") {
        const marker = path.join(root, "descendant-marker");
        const result = run(argv("disabled", process.execPath, [workload, "descendant"]), { env: { ...process.env, PEER_MARKER: marker } });
        check(result.status === 0 && result.stdout === "spawned\n", "descendant launch differs");
        await pause(800);
        check(!fs.existsSync(marker), "descendant survived launcher exit");
      } else if (scenario === "signal") {
        check(process.platform !== "win32", "POSIX signal proof must run on Linux/macOS");
        const result = run(argv("disabled", process.execPath, [workload, "signal"]));
        check(result.signal === "SIGTERM", `signal propagation differs: ${JSON.stringify(result)}`);
      } else if (scenario === "argv-env") {
        const words = ["", "space word", "quote\"slash\\", "λ", "--network=internet"];
        const result = run(argv("disabled", process.execPath, [workload, "argv-env", ...words]), { env: { ...process.env, PEER_INHERITED: "ambient-exact" } });
        check(result.status === 0 && result.stdout === `${JSON.stringify(words)}\nambient-exact\n${root}\n`, `argv/env/cwd differs: ${JSON.stringify(result)}`);
      } else if (scenario === "two-signals") {
        check(process.platform !== "win32", "POSIX two-signal proof must run on Linux/macOS");
        const marker = path.join(root, "descendant-marker");
        interruptChild = spawn(launcher, argv("disabled", process.execPath, [workload, "interrupt-worker"]), {
          cwd: root, env: { ...process.env, PEER_MARKER: marker }, stdio: ["ignore", "pipe", "pipe"],
        });
        let ended = false;
        let output = "";
        interruptChild.stdout.on("data", bytes => { output += bytes; });
        interruptChild.stderr.on("data", bytes => { output += bytes; });
        const completion = new Promise((resolve, reject) => {
          interruptChild.once("error", reject);
          interruptChild.once("exit", (code, signal) => { ended = true; resolve({ code, signal }); });
        });
        let deadline = Date.now() + 3000;
        while (!fs.existsSync(path.join(root, "ready")) && !ended && Date.now() < deadline) await pause(5);
        check(fs.existsSync(path.join(root, "ready")), `worker not ready: ${output}`);
        interruptChild.kill("SIGINT");
        deadline = Date.now() + 500;
        while (!fs.existsSync(path.join(root, "first-signal")) && !ended && Date.now() < deadline) await pause(5);
        check(fs.existsSync(path.join(root, "first-signal")), "first signal was not forwarded");
        interruptChild.kill("SIGTERM");
        const result = await Promise.race([completion, pause(500).then(() => { throw new Error("second signal stranded launcher waiting for child"); })]);
        check(result.signal === "SIGINT", `initial signal was not propagated: ${JSON.stringify(result)} ${output}`);
        await pause(1800);
        check(!fs.existsSync(marker), "descendant survived two-signal shutdown");
      } else {
        throw new Error(`unknown scenario ${scenario}`);
      }
    }
    print(`peer-launcher ${scenario}: passed`);
  } finally {
    if (interruptChild && interruptChild.exitCode === null && interruptChild.signalCode === null) {
      interruptChild.kill("SIGKILL");
      await pause(4100);
    }
    fs.rmSync(root, { recursive: true, force: true });
    fs.rmSync(outside, { recursive: true, force: true });
  }
} else {
  throw new Error(`unknown workload mode ${mode}`);
}
