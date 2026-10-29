#!/usr/bin/env node
// Every public Core module must pass `jet check` when a program imports it
// alone. A whole-program build only checks the Core items its callers reach, so
// a module whose own source no longer checks stays hidden until some program
// imports it. This sweep checks each `module … exports` row of
// Compiler/Prelude/Core.jet in its own one-import probe and reports every
// failing module, not the first one.
//
// JET=/path/to/jet node Tools/agent/core-module-check.mjs [--check|--write] [--jobs N] [--only a,b] [--scratch DIR]
// --check fails when a module outside the ratchet fails, or a ratcheted module
// reports more errors than its ceiling. A ratcheted module that now checks
// cleanly or with fewer errors also fails --check until --write lowers it.
// --write initializes or lowers the committed ceilings; it never raises one.
// --only checks the named modules and gates only them.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const RATCHET = "Tools/agent/core-module-check-ratchet.json";
const MAX_JOBS = 8;

// `module <name> exports { … }` rows name the importable Core modules.
export function coreModules(coreJet) {
  const modules = [];
  for (const line of coreJet.split("\n")) {
    const match = /^module\s+([a-z_][a-z0-9_.]*)\s+exports\b/.exec(line);
    if (match && !modules.includes(match[1])) modules.push(match[1]);
  }
  return modules;
}

export function probeSource(module) {
  return `use ${module}\n\nfn main() {\n    print("ok")\n}\n`;
}

// Errors are the `Error [CODE]` headers of a keep-going check; the first
// header with its location names the module's leading failure.
export function checkSummary(output, status) {
  const lines = output.split("\n");
  let errors = 0;
  let first = "";
  for (let index = 0; index < lines.length; index++) {
    if (!/^Error\b/.test(lines[index])) continue;
    errors++;
    if (!first) {
      const at = lines.slice(index + 1, index + 4).find((line) => /^\s*--> /.test(line));
      first = at ? `${lines[index]} ${at.trim()}` : lines[index];
    }
  }
  if (status !== 0 && errors === 0) {
    errors = 1;
    first = `jet check exited ${status} without an Error header: ${lines.find((line) => line.trim()) ?? ""}`;
  }
  return { errors, first };
}

// Compares measured error counts with the committed ceilings for the modules
// that ran. Returns every problem; an empty list means the gate holds.
export function ratchetProblems(ceilings, results) {
  const problems = [];
  for (const [module, { errors, first }] of results) {
    const ceiling = ceilings[module] ?? 0;
    if (errors > ceiling) problems.push(`${module}: ${errors} errors (ceiling ${ceiling}); ${first}`);
    else if (errors < ceiling) problems.push(`${module}: ${errors} errors, below its ceiling ${ceiling}; lower it with --write`);
  }
  return problems;
}

// --write keeps ceilings that did not run, lowers those that improved and
// drops modules that now check cleanly. A rise is a failure, never a write.
// With no committed ratchet yet, --write records today's failing counts.
export function loweredCeilings(ceilings, results, initialize = false) {
  const next = { ...ceilings };
  const rises = [];
  for (const [module, { errors }] of results) {
    const ceiling = ceilings[module] ?? 0;
    if (errors > ceiling && !initialize) rises.push(`${module}: ${errors} errors exceed ceiling ${ceiling}`);
    else if (errors === 0) delete next[module];
    else next[module] = errors;
  }
  return { next: Object.fromEntries(Object.entries(next).sort(([a], [b]) => a.localeCompare(b))), rises };
}

function runCheck(jet, file, cwd) {
  return new Promise((resolveRun) => {
    const child = spawn(jet, ["check", file], {
      cwd,
      env: { ...process.env, NO_COLOR: "1", JET_TOOLCHAIN_ROOT: ROOT },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    const keep = (chunk) => { if (output.length < 1 << 22) output += chunk; };
    child.stdout.on("data", keep);
    child.stderr.on("data", keep);
    const timer = setTimeout(() => child.kill("SIGKILL"), 300_000);
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolveRun({ output, status: signal ? 128 : code ?? 1 });
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      resolveRun({ output: `spawn failed: ${error.message}`, status: 127 });
    });
  });
}

async function sweep(jet, modules, jobs, scratch) {
  const results = new Map();
  let next = 0;
  async function worker() {
    while (next < modules.length) {
      const module = modules[next++];
      const file = join(scratch, `${module.replaceAll(".", "_")}.jet`);
      writeFileSync(file, probeSource(module));
      const { output, status } = await runCheck(jet, file, scratch);
      results.set(module, checkSummary(output, status));
    }
  }
  await Promise.all(Array.from({ length: Math.min(jobs, modules.length) }, worker));
  return new Map(modules.map((module) => [module, results.get(module)]));
}

function option(args, name) {
  const index = args.indexOf(name);
  return index < 0 ? null : args[index + 1] ?? null;
}

async function main(args) {
  const mode = args.includes("--write") ? "write" : "check";
  const jet = process.env.JET;
  if (!jet || !existsSync(jet)) throw new Error("JET must name the admitted native jet compiler");
  const jobs = Number(option(args, "--jobs") ?? 4);
  if (!Number.isInteger(jobs) || jobs < 1 || jobs > MAX_JOBS) throw new Error(`--jobs takes 1..${MAX_JOBS}`);
  const known = coreModules(readFileSync(join(ROOT, "Compiler/Prelude/Core.jet"), "utf8"));
  const only = option(args, "--only");
  const modules = only ? only.split(",").filter(Boolean) : known;
  const unknown = modules.filter((module) => !known.includes(module));
  if (unknown.length) throw new Error(`not a Core module: ${unknown.join(", ")}`);
  const ratchetPath = join(ROOT, RATCHET);
  const initialize = !existsSync(ratchetPath);
  const ceilings = initialize ? {} : JSON.parse(readFileSync(ratchetPath, "utf8"));
  const scratchRoot = option(args, "--scratch");
  if (scratchRoot) mkdirSync(scratchRoot, { recursive: true });
  const scratch = mkdtempSync(join(scratchRoot ?? tmpdir(), "core-module-check-"));
  let results;
  try {
    results = await sweep(jet, modules, jobs, scratch);
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  const failing = [...results].filter(([, { errors }]) => errors > 0);
  for (const [module, { errors, first }] of failing) console.log(`${module}: ${errors} errors; ${first}`);
  console.log(`core modules: ${results.size} checked, ${results.size - failing.length} clean, ${failing.length} failing`);
  if (mode === "write") {
    const { next, rises } = loweredCeilings(ceilings, results, initialize);
    if (rises.length) {
      for (const rise of rises) console.error(`core-module-check: ${rise}`);
      return 1;
    }
    writeFileSync(ratchetPath, `${JSON.stringify(next, null, 2)}\n`);
    console.log(`wrote ${RATCHET}`);
    return 0;
  }
  const problems = ratchetProblems(ceilings, results);
  for (const problem of problems) console.error(`core-module-check: ${problem}`);
  return problems.length ? 1 : 0;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).then((code) => { process.exitCode = code; }, (error) => {
    console.error(`core-module-check: ${error.message}`);
    process.exitCode = 2;
  });
}
