import assert from "node:assert/strict";
import { promises as fs, chmodSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { probeAxisTools, stageEntry } from "./run.mjs";

const scratchRoot = path.join(os.homedir(), ".cache", "jet-test-scratch");
const stagedEntryTestOptions = { timeout: 120_000 };

async function withScratch(name, callback) {
  await fs.mkdir(scratchRoot, { recursive: true });
  const root = await mkdtemp(path.join(scratchRoot, `gauntlet-${name}-`));
  try {
    return await callback(root);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const port = server.address().port;
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  return port;
}

function traceSource() {
  return [
    "#!/usr/bin/env node",
    "const fs = require('node:fs');",
    "if (process.env.JET_TRACE_TIERS_PATH) fs.writeFileSync(process.env.JET_TRACE_TIERS_PATH, JSON.stringify({ rows: [{ function: 'fixture', tier: 'native', reason: null, millis: 0 }], native_rows: 1, interp_rows: 0, whole_program_deopt: false }));",
  ].join("\n");
}

function artifactSource(runBody, peerPort = null) {
  const peerCheck = peerPort === null ? "" : [
    "const { spawnSync } = require('node:child_process');",
    `if (spawnSync('python3', ['-c', "import socket,sys; s=socket.create_connection(('127.0.0.1', int(sys.argv[1])), 1); s.close()", '${peerPort}'], { stdio: 'ignore' }).status !== 0) process.exit(1);`,
  ].join("\n");
  return [
    "#!/usr/bin/env node",
    "const fs = require('node:fs');",
    "const path = require('node:path');",
    peerCheck,
    runBody,
    traceSource().split("\n").slice(2).join("\n"),
    "",
  ].join("\n");
}

function fakeJetSource({ expected, peerPort = null, stateful = false, failOn = null }) {
  const peerCheck = peerPort === null ? "" : [
    "const { spawnSync } = require('node:child_process');",
    `if (spawnSync('python3', ['-c', "import socket,sys; s=socket.create_connection(('127.0.0.1', int(sys.argv[1])), 1); s.close()", '${peerPort}'], { stdio: 'ignore' }).status !== 0) process.exit(1);`,
  ].join("\n");
  const runBody = stateful ? [
    "const statePath = path.resolve('state.txt');",
    "const value = (fs.existsSync(statePath) ? Number(fs.readFileSync(statePath, 'utf8')) : 0) + 1;",
    "fs.writeFileSync(statePath, String(value));",
    "process.stdout.write(`${value}\\n`);",
  ].join("\n") : `process.stdout.write(${JSON.stringify(expected)});`;
  return [
    "#!/usr/bin/env node",
    "const fs = require('node:fs');",
    "const path = require('node:path');",
    "const args = process.argv.slice(2);",
    `const artifact = ${JSON.stringify(artifactSource(runBody, peerPort))};`,
    "if (args[0] === 'build') {",
    "  fs.mkdirSync('build', { recursive: true });",
    "  fs.writeFileSync('build/run', artifact);",
    "  fs.chmodSync('build/run', 0o755);",
    "  process.exit(0);",
    "}",
    "if (args[0] !== 'run' && args[0] !== 'dev') process.exit(1);",
    `if (args[0] === ${JSON.stringify(failOn)}) process.exit(1);`,
    peerCheck,
    runBody,
    traceSource().split("\n").slice(2).join("\n"),
    "",
  ].join("\n");
}

async function writeEntry(root, name, { expected, spec = {}, fakeJet }) {
  const entryDir = path.join(root, "entry");
  await fs.mkdir(path.join(entryDir, "jet"), { recursive: true });
  await fs.writeFile(path.join(entryDir, "entry.json"), JSON.stringify({ name, mode: spec.mode ?? "batch", languages: ["jet"], spec }));
  await fs.writeFile(path.join(entryDir, "expected.out"), expected);
  await fs.writeFile(path.join(entryDir, "jet", "run.jet"), "// behavior fixture\n");
  const jetBin = path.join(root, "fake-jet");
  await fs.writeFile(jetBin, fakeJet);
  chmodSync(jetBin, 0o755);
  return { entryDir, jetBin };
}

function peerSource(logPath) {
  return [
    "import signal",
    "import socket",
    `LOG = ${JSON.stringify(logPath)}`,
    "server = socket.socket()",
    "server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)",
    "server.bind(('127.0.0.1', int(__import__('sys').argv[1])))",
    "server.listen()",
    "running = True",
    "def stop(*_args):",
    "    global running",
    "    running = False",
    "    server.close()",
    "    with open(LOG, 'a', encoding='utf8') as log: log.write('stop\\n')",
    "signal.signal(signal.SIGTERM, stop)",
    "with open(LOG, 'a', encoding='utf8') as log: log.write('start\\n')",
    "server.settimeout(0.2)",
    "while running:",
    "    try:",
    "        client, _ = server.accept()",
    "        client.close()",
    "    except socket.timeout:",
    "        pass",
    "    except OSError:",
    "        break",
  ].join("\n");
}

test("peer fixture spans all Jet tiers with one tier lifetime", stagedEntryTestOptions, async () => {
  await withScratch("peer-lifetime", async (root) => {
    const port = await freePort();
    const logPath = path.join(root, "peer-events.log");
    const expected = "ok\n";
    const files = await writeEntry(root, "peer-lifetime-fixture", {
      expected,
      fakeJet: fakeJetSource({ expected, peerPort: port }),
      spec: { args: [], peer: { script: "peer.py", port } },
    });
    await fs.writeFile(path.join(files.entryDir, "peer.py"), peerSource(logPath));
    const result = await stageEntry(files.entryDir, {
      name: "peer-lifetime-fixture",
      mode: "batch",
      languages: ["jet"],
      spec: { args: [], peer: { script: "peer.py", port } },
    }, path.join(root, "run"), files.jetBin, 1, true);

    assert.equal(result.status, "ok");
    assert.equal(result.jet_tiers.aot.status, "ok");
    assert.equal(result.jet_tiers.run.status, "ok");
    assert.equal(result.jet_tiers.dev.status, "ok");
    assert.equal(result.jet_tiers.run.trace.status, "passed");
    assert.deepEqual((await fs.readFile(logPath, "utf8")).trim().split("\n"), ["start", "stop", "start", "stop"]);
  });
});

test("batch-step reset is reused by measured runs and tier trace", stagedEntryTestOptions, async () => {
  await withScratch("batch-reset", async (root) => {
    const expected = "1\n2\n";
    const files = await writeEntry(root, "batch-reset-fixture", {
      expected,
      fakeJet: fakeJetSource({ expected, stateful: true }),
      spec: { mode: "batch-steps", args: [], steps: [[], []] },
    });
    const result = await stageEntry(files.entryDir, {
      name: "batch-reset-fixture",
      mode: "batch-steps",
      languages: ["jet"],
      spec: { args: [], steps: [[], []] },
    }, path.join(root, "run"), files.jetBin, 1, true);

    assert.equal(result.status, "ok");
    assert.equal(result.jet_tiers.run.status, "ok");
    assert.equal(result.jet_tiers.dev.status, "ok");
    assert.equal(result.jet_tiers.run.trace.status, "passed");
    assert.equal(result.jet_tiers.run.trace.invocations[0].exit_code, 0);
  });
});

test("peer fixture stops when a Jet tier fails", stagedEntryTestOptions, async () => {
  await withScratch("peer-failure", async (root) => {
    const port = await freePort();
    const logPath = path.join(root, "peer-events.log");
    const expected = "ok\n";
    const files = await writeEntry(root, "peer-failure-fixture", {
      expected,
      fakeJet: fakeJetSource({ expected, peerPort: port, failOn: "run" }),
      spec: { args: [], peer: { script: "peer.py", port } },
    });
    await fs.writeFile(path.join(files.entryDir, "peer.py"), peerSource(logPath));
    const result = await stageEntry(files.entryDir, {
      name: "peer-failure-fixture",
      mode: "batch",
      languages: ["jet"],
      spec: { args: [], peer: { script: "peer.py", port } },
    }, path.join(root, "run"), files.jetBin, 1, true);

    assert.equal(result.status, "broken");
    assert.equal(result.jet_tiers.run.status, "broken");
    assert.deepEqual((await fs.readFile(logPath, "utf8")).trim().split("\n"), ["start", "stop", "start", "stop"]);
  });
});

test("Zig axis probe uses the rail's version subcommand", async () => {
  await withScratch("zig-probe", async (root) => {
    const bin = path.join(root, "bin");
    await fs.mkdir(bin, { recursive: true });
    const zig = path.join(bin, "zig");
    await fs.writeFile(zig, "#!/bin/sh\nif [ \"${1:-}\" = version ]; then printf '0.16.0\\n'; exit 0; fi\nprintf 'use zig version subcommand\\n' >&2\nexit 1\n");
    chmodSync(zig, 0o755);
    const previousPath = process.env.PATH;
    process.env.PATH = `${bin}${path.delimiter}${previousPath ?? ""}`;
    try {
      const [probe] = await probeAxisTools(root, ["zig"], path.join(root, "missing-jet"));
      assert.equal(probe.status, "available");
    } finally {
      if (previousPath === undefined) delete process.env.PATH;
      else process.env.PATH = previousPath;
    }
  });
});
