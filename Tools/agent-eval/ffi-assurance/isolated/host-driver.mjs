#!/usr/bin/env node

import { createHash } from "node:crypto";
import { promises as fs } from "node:fs";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PROTOCOL = Object.freeze({
  magic: Buffer.from("FAIS"),
  version: 1,
  headerBytes: 40,
  maxPayload: 4096,
  kinds: Object.freeze({ request: 1, response: 2, error: 3 }),
  statuses: Object.freeze({
    ok: 0,
    badMagic: 1,
    badVersion: 2,
    badKind: 3,
    badLength: 4,
    badChecksum: 5,
    badRequest: 6,
    internal: 7,
  }),
});

const DEFAULT_TIMEOUT_MS = 10_000;

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function fnv64(payload) {
  let value = 0xcbf29ce484222325n;
  for (const byte of payload) {
    value ^= BigInt(byte);
    value = BigInt.asUintN(64, value * 0x100000001b3n);
  }
  return value;
}

function putHeader({ kind, requestId, payloadLength, status, value, checksum }) {
  const header = Buffer.alloc(PROTOCOL.headerBytes);
  PROTOCOL.magic.copy(header, 0);
  header.writeUInt16LE(PROTOCOL.version, 4);
  header.writeUInt16LE(kind, 6);
  header.writeBigUInt64LE(BigInt(requestId), 8);
  header.writeUInt32LE(payloadLength, 16);
  header.writeUInt32LE(status, 20);
  header.writeBigUInt64LE(BigInt(value), 24);
  header.writeBigUInt64LE(BigInt.asUintN(64, BigInt(checksum)), 32);
  return header;
}

export function encodeRequest(payload, { requestId = 1n } = {}) {
  const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
  if (bytes.length > PROTOCOL.maxPayload) throw new Error("isolated payload exceeds declared maximum");
  const header = putHeader({
    kind: PROTOCOL.kinds.request,
    requestId,
    payloadLength: bytes.length,
    status: PROTOCOL.statuses.ok,
    value: 0,
    checksum: fnv64(bytes),
  });
  return Buffer.concat([header, bytes]);
}

export function adversarialRequest(caseId, payload = Buffer.from("ffi-assurance")) {
  const request = encodeRequest(payload);
  switch (caseId) {
    case "valid":
      return { bytes: request, expected: { kind: PROTOCOL.kinds.response, status: PROTOCOL.statuses.ok, exitCode: 0 } };
    case "bad-magic":
      request[0] ^= 0xff;
      return { bytes: request, expected: { kind: PROTOCOL.kinds.error, status: PROTOCOL.statuses.badMagic, exitCode: 2 } };
    case "bad-version":
      request.writeUInt16LE(PROTOCOL.version + 1, 4);
      return { bytes: request, expected: { kind: PROTOCOL.kinds.error, status: PROTOCOL.statuses.badVersion, exitCode: 2 } };
    case "bad-kind":
      request.writeUInt16LE(0xffff, 6);
      return { bytes: request, expected: { kind: PROTOCOL.kinds.error, status: PROTOCOL.statuses.badKind, exitCode: 2 } };
    case "bad-length":
      request.writeUInt32LE(PROTOCOL.maxPayload + 1, 16);
      return { bytes: request.subarray(0, PROTOCOL.headerBytes), expected: { kind: PROTOCOL.kinds.error, status: PROTOCOL.statuses.badLength, exitCode: 2 } };
    case "bad-checksum":
      request[32] ^= 0x01;
      return { bytes: request, expected: { kind: PROTOCOL.kinds.error, status: PROTOCOL.statuses.badChecksum, exitCode: 2 } };
    case "wrong-result":
      return { bytes: request, expected: { kind: PROTOCOL.kinds.response, status: PROTOCOL.statuses.ok, exitCode: 0, wrongValue: true } };
    default:
      throw new Error(`unknown isolated case ${caseId}`);
  }
}

export function decodeMessage(bytes) {
  if (!Buffer.isBuffer(bytes) || bytes.length !== PROTOCOL.headerBytes) {
    throw new Error(`isolated response must be exactly ${PROTOCOL.headerBytes} bytes`);
  }
  if (!bytes.subarray(0, 4).equals(PROTOCOL.magic)) throw new Error("isolated response magic mismatch");
  return {
    version: bytes.readUInt16LE(4),
    kind: bytes.readUInt16LE(6),
    requestId: bytes.readBigUInt64LE(8),
    payloadLength: bytes.readUInt32LE(16),
    status: bytes.readUInt32LE(20),
    value: bytes.readBigUInt64LE(24),
    checksum: bytes.readBigUInt64LE(32),
  };
}

async function processTreeRssBytes(rootPid) {
  if (!Number.isInteger(rootPid)) return null;
  let entries;
  try {
    entries = await fs.readdir("/proc", { withFileTypes: true });
  } catch {
    return null;
  }
  const rows = new Map();
  await Promise.all(entries.filter((entry) => entry.isDirectory() && /^\d+$/.test(entry.name)).map(async (entry) => {
    try {
      const status = await fs.readFile(`/proc/${entry.name}/status`, "utf8");
      const parent = status.match(/^PPid:\s+(\d+)$/m);
      const rss = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
      if (parent && rss) rows.set(Number(entry.name), { parent: Number(parent[1]), rss: Number(rss[1]) * 1024 });
    } catch {}
  }));
  const tree = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const [pid, row] of rows) {
      if (tree.has(row.parent) && !tree.has(pid)) {
        tree.add(pid);
        changed = true;
      }
    }
  }
  let total = 0;
  let found = false;
  for (const pid of tree) {
    const row = rows.get(pid);
    if (row) {
      total += row.rss;
      found = true;
    }
  }
  return found ? total : null;
}

function monotonicNs() {
  return process.hrtime.bigint();
}

function parseOutput(result, expected, payload) {
  const message = decodeMessage(result.stdout);
  if (message.version !== PROTOCOL.version) throw new Error("isolated response version mismatch");
  if (message.requestId !== 1n && message.kind !== PROTOCOL.kinds.error) throw new Error("isolated response request id mismatch");
  if (message.payloadLength !== 0) throw new Error("isolated response advertised an unreturned payload");
  if (message.kind !== expected.kind || message.status !== expected.status) {
    throw new Error(`isolated response ${message.kind}/${message.status}; expected ${expected.kind}/${expected.status}`);
  }
  if (result.code !== expected.exitCode) throw new Error(`isolated worker exited ${result.code}; expected ${expected.exitCode}`);
  if (expected.kind === PROTOCOL.kinds.response) {
    const expectedSum = [...payload].reduce((sum, byte) => sum + byte, 0);
    const expectedChecksum = fnv64(payload);
    if (message.value !== BigInt(expected.wrongValue ? expectedSum + 1 : expectedSum)) {
      throw new Error(`isolated result value ${message.value}; expected ${expected.wrongValue ? expectedSum + 1 : expectedSum}`);
    }
    if (message.checksum !== expectedChecksum) throw new Error("isolated result checksum mismatch");
  }
  return message;
}

export async function runIsolated({
  launcher,
  worker,
  root,
  taskId = "ffi-assurance-isolated",
  caseId = "valid",
  payload = Buffer.from("ffi-assurance"),
  timeoutMs = DEFAULT_TIMEOUT_MS,
}) {
  if (!launcher || !worker || !root) throw new Error("launcher, worker, and root are required");
  const absoluteLauncher = path.resolve(launcher);
  const absoluteWorker = path.resolve(worker);
  const absoluteRoot = path.resolve(root);
  await fs.mkdir(absoluteRoot, { recursive: true });
  const stagedWorker = path.join(absoluteRoot, "ffi-assurance-worker");
  const copyStarted = monotonicNs();
  await fs.copyFile(absoluteWorker, stagedWorker);
  await fs.chmod(stagedWorker, 0o755);
  const copyNs = Number(monotonicNs() - copyStarted);
  const workerBytes = await fs.readFile(stagedWorker);
  const request = adversarialRequest(caseId, payload);
  const start = monotonicNs();
  const child = spawn(absoluteLauncher, [
    "--contract", "compiled-workload-peer-isolation-v1",
    "--task-id", taskId,
    "--root", absoluteRoot,
    "--cwd", absoluteRoot,
    "--network", "disabled",
    "--external-write", "disabled",
    "--host", "ambient",
    "--", stagedWorker,
  ], {
    cwd: absoluteRoot,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  let firstOutputNs = null;
  let timedOut = false;
  let peakRssBytes = null;
  processTreeRssBytes(child.pid).then((rss) => {
    if (rss !== null) peakRssBytes = Math.max(peakRssBytes ?? 0, rss);
  }).catch(() => {});
  const rssTimer = setInterval(() => {
    processTreeRssBytes(child.pid).then((rss) => {
      if (rss !== null) peakRssBytes = Math.max(peakRssBytes ?? 0, rss);
    }).catch(() => {});
  }, 10);
  child.stdout.on("data", (chunk) => {
    if (firstOutputNs === null) firstOutputNs = monotonicNs();
    stdout.push(chunk);
  });
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  child.stdin.end(request.bytes);
  const result = await new Promise((resolve) => {
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.on("error", (error) => {
      clearTimeout(timer);
      resolve({ code: 127, signal: null, error, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolve({ code: timedOut ? 124 : (code ?? 128), signal, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr) });
    });
  });
  clearInterval(rssTimer);
  if (result.error) throw result.error;
  const finished = monotonicNs();
  let message;
  let hostValidation = { status: "accepted", error: null };
  try {
    message = parseOutput(result, request.expected, payload);
  } catch (error) {
    if (caseId !== "wrong-result") throw error;
    message = decodeMessage(result.stdout);
    hostValidation = { status: "rejected-untrusted-result", error: error.message };
  }
  const messageRecord = {
    version: message.version,
    kind: message.kind,
    request_id: message.requestId.toString(),
    payload_length: message.payloadLength,
    status: message.status,
    value: message.value.toString(),
    checksum: message.checksum.toString(16),
  };
  return {
    request_bytes: request.bytes.length,
    response_bytes: result.stdout.length,
    wire_output_sha256: sha256(result.stdout),
    validated_output_sha256: sha256(JSON.stringify(messageRecord)),
    stderr: result.stderr.toString("utf8"),
    exit_code: result.code,
    signal: result.signal,
    timed_out: timedOut,
    host_validation: hostValidation,
    message: messageRecord,
    authority: {
      launcher: absoluteLauncher,
      contract: "compiled-workload-peer-isolation-v1",
      task_id: taskId,
      root: absoluteRoot,
      cwd: absoluteRoot,
      network: "disabled",
      external_write: "disabled",
      host: "ambient",
      fallback: "none",
      process_io: ["stdin", "stdout", "stderr"],
    },
    artifact: {
      path: absoluteWorker,
      staged_path: stagedWorker,
      bytes: workerBytes.length,
      sha256: sha256(workerBytes),
    },
    metrics: {
      startup_ns: firstOutputNs === null ? null : Number(firstOutputNs - start),
      runtime_ns: Number(finished - start),
      peak_rss_bytes: peakRssBytes,
      copy_ns: copyNs,
      copy_count: 1,
      copy_bytes: workerBytes.length + request.bytes.length + result.stdout.length,
      artifact_size_bytes: workerBytes.length,
    },
  };
}

function takeOption(args, name) {
  const index = args.indexOf(name);
  if (index < 0 || index + 1 >= args.length) return null;
  return args[index + 1];
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help")) {
    console.log("usage: host-driver.mjs --launcher PATH --worker PATH --root DIR --case valid|bad-magic|bad-version|bad-kind|bad-length|bad-checksum|wrong-result");
    return;
  }
  const launcher = takeOption(args, "--launcher") ?? process.env.JET_FFI_ASSURANCE_LAUNCHER;
  const worker = takeOption(args, "--worker");
  const root = takeOption(args, "--root");
  const caseId = takeOption(args, "--case") ?? "valid";
  const payload = Buffer.from(takeOption(args, "--payload") ?? "ffi-assurance", "utf8");
  if (!launcher || !worker || !root) throw new Error("--launcher, --worker, and --root are required (no containment fallback exists)");
  const receipt = await runIsolated({ launcher, worker, root, caseId, payload });
  process.stdout.write(`${JSON.stringify(receipt)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(`ffi-assurance isolated driver: ${error.message}`);
    process.exitCode = 1;
  });
}
