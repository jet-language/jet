#!/usr/bin/env node

import { mkdirSync } from "node:fs";
import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const fixtureArgument = process.argv[2];
const fixture = fixtureArgument ? resolve(process.cwd(), fixtureArgument) : null;
const scratchRoot = process.env.JET_TEST_SCRATCH_DIR
  ?? `${process.env.HOME ?? "~"}/.cache/jet-test-scratch`;
const scratch = resolve(scratchRoot, "syntax-roundtrip-canvas");
const jet = process.env.JET_SYNTAX_CENSUS_CANVAS_JET
  ?? resolve(ROOT, "target/debug/jet");
const START_TIMEOUT_MS = 6_000;
const FETCH_TIMEOUT_MS = 3_000;

function fail(message) {
  throw new Error(`Canvas probe: ${message}`);
}

if (!fixture) fail("a fixture path is required");

mkdirSync(scratch, { recursive: true });

function readUrl(output) {
  return output.match(/^Canvas: (https?:\/\/[^\r\n]+)$/m)?.[1]?.trim() ?? null;
}

const child = spawn(jet, ["dev", fixture, "--canvas"], {
  cwd: ROOT,
  env: {
    ...process.env,
    TMPDIR: scratch,
    JET_TEST_SCRATCH_DIR: scratch,
    JET_CANVAS_BROWSER: process.env.JET_CANVAS_BROWSER
      ?? "/definitely/missing/canvas-browser",
  },
  stdio: ["ignore", "pipe", "pipe"],
});

let stdout = "";
let stderr = "";
let exit = null;
let spawnError = null;
child.stdout.setEncoding("utf8");
child.stderr.setEncoding("utf8");
child.stdout.on("data", (chunk) => { stdout += chunk; });
child.stderr.on("data", (chunk) => { stderr += chunk; });
child.once("error", (error) => { spawnError = error; });
const closed = new Promise((resolveClose) => {
  child.once("close", (code, signal) => {
    exit = { code, signal };
    resolveClose(exit);
  });
});

let stopping = false;
async function stop() {
  if (exit || spawnError) return;
  child.kill("SIGINT");
  await Promise.race([closed, delay(1_000)]);
  if (!exit) child.kill("SIGKILL");
  await Promise.race([closed, delay(1_000)]);
}

async function stopOnSignal() {
  if (stopping) return;
  stopping = true;
  await stop();
  process.exit(1);
}

process.once("SIGINT", stopOnSignal);
process.once("SIGTERM", stopOnSignal);

async function canvasUrl() {
  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (spawnError) throw spawnError;
    const url = readUrl(stdout);
    if (url) return url;
    if (exit) fail(`jet dev exited before publishing Canvas URL (${exit.code ?? exit.signal})`);
    await delay(25);
  }
  fail(`timed out waiting for Canvas URL${stderr ? `: ${stderr}` : ""}`);
}

async function graphResponse(url) {
  const canvas = new URL(url);
  const token = canvas.searchParams.get("session");
  if (!token) fail("Canvas URL did not contain a session token");
  const graph = new URL("/__jet_canvas/graph", canvas);
  graph.searchParams.set("session", token);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetch(graph, {
      headers: {
        authorization: `Bearer ${token}`,
        origin: canvas.origin,
      },
      signal: controller.signal,
    });
    const text = await response.text();
    let body;
    try {
      body = JSON.parse(text);
    } catch {
      body = { raw: text };
    }
    return {
      canvas_http_status: response.status,
      canvas_response: body,
    };
  } finally {
    clearTimeout(timeout);
  }
}

let output = null;
try {
  output = await graphResponse(await canvasUrl());
} catch (error) {
  const reason = error instanceof Error ? error.message : String(error);
  output = {
    canvas_probe_status: "unavailable",
    canvas_probe_reason: reason.slice(0, 1_000),
  };
} finally {
  await stop();
}
if (output) process.stdout.write(`${JSON.stringify(output)}\n`);
