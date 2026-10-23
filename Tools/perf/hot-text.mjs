#!/usr/bin/env node
// Writes Compiler/JetCli/hot-text.list: the compiler's own functions that a
// cold hello `jet run`, `jet check` and `jet build` enter, in first-entry
// order (run's first, then those check adds, then build's). `jet build` of
// Compiler/JetCli lays them out first (Host.jet jet_cli_hot_text, JetIR
// ir_x64_hot_order), so a cold start faults in fewer pages of the compiler's
// text. Names that no longer name a function are skipped by the build
// (`--trace` counter ir.object.hot_missing); regenerate after large renames.
//
//   node Tools/perf/hot-text.mjs --jet <jet> [--out Compiler/JetCli/hot-text.list]
//
// <jet> is a built compiler beside its jet-runtime/ directory. Only the
// compiler's own functions are listed: its object is the link's first input,
// so they are the functions below the first one the runtime pack
// (jet-runtime/<native.key>/jet_runtime.pack.o) defines; libc and libgcc
// come after the pack. The environment names the toolchain as for any run
// (JET_TOOLCHAIN_ROOT, JET_COMPILER_IDENTITY). Needs `cc` and ptrace of a
// child: each command runs under Tools/perf/fntrace.c, which puts a
// breakpoint at every function of <jet> and records each one's first entry.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const COMMANDS = ["run", "check", "build"];
const HELLO = 'fn run() {\n    print("hello")\n}\n';

function parseArgs(argv) {
  const args = { jet: "", out: join(ROOT, "Compiler/JetCli/hot-text.list") };
  for (let i = 2; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    if (!(key in args) || i + 1 >= argv.length) throw new Error(`usage: node Tools/perf/hot-text.mjs --jet <jet> [--out <file>]`);
    args[key] = resolve(argv[i + 1]);
  }
  if (!args.jet) throw new Error("--jet is required");
  return args;
}

// Every sized FUNC symbol of the ELF file `bytes`: { value, name }.
function functionSymbols(bytes) {
  const shoff = Number(bytes.readBigUInt64LE(40));
  const shnum = bytes.readUInt16LE(60);
  const section = (k) => {
    const at = shoff + 64 * k;
    return { type: bytes.readUInt32LE(at + 4), offset: Number(bytes.readBigUInt64LE(at + 24)), size: Number(bytes.readBigUInt64LE(at + 32)), link: bytes.readUInt32LE(at + 40) };
  };
  const symbols = [];
  for (let k = 0; k < shnum; k++) {
    const table = section(k);
    if (table.type !== 2) continue;
    const strings = section(table.link);
    for (let at = table.offset; at < table.offset + table.size; at += 24) {
      if ((bytes[at + 4] & 15) !== 2 || bytes.readBigUInt64LE(at + 16) === 0n) continue;
      const start = strings.offset + bytes.readUInt32LE(at);
      const end = bytes.indexOf(0, start);
      symbols.push({ value: bytes.readBigUInt64LE(at + 8), name: bytes.toString("utf8", start, end) });
    }
  }
  return symbols;
}

function main() {
  const args = parseArgs(process.argv);
  const jetBytes = readFileSync(args.jet);
  const runtime = join(dirname(args.jet), "jet-runtime");
  const key = readFileSync(join(runtime, "native.key"), "utf8").trim();
  const pack = new Set(functionSymbols(readFileSync(join(runtime, key, "jet_runtime.pack.o"))).map((symbol) => symbol.name));
  const byAddress = new Map();
  const symbols = functionSymbols(jetBytes);
  let boundary = -1n;
  for (const symbol of symbols) {
    if (pack.has(symbol.name) && (boundary < 0n || symbol.value < boundary)) boundary = symbol.value;
  }
  if (boundary < 0n) throw new Error(`${args.jet} defines none of the runtime pack's functions`);
  // `main` is the object's first function whatever the list says.
  for (const symbol of symbols) {
    if (symbol.value < boundary && symbol.name !== "main" && !byAddress.has(symbol.value)) byAddress.set(symbol.value, symbol.name);
  }
  const work = mkdtempSync(join(tmpdir(), "hot-text-"));
  try {
    const tracer = join(work, "fntrace");
    execFileSync("cc", ["-O2", "-o", tracer, join(ROOT, "Tools/perf/fntrace.c")], { stdio: "inherit" });
    const addresses = join(work, "addresses");
    writeFileSync(addresses, [...byAddress.keys()].map((value) => value.toString(16)).join("\n") + "\n");
    const project = join(work, "hello");
    execFileSync("mkdir", ["-p", project]);
    writeFileSync(join(project, "hello.jet"), HELLO);
    const names = [];
    const listed = new Set();
    for (const command of COMMANDS) {
      const entered = join(work, `${command}.entered`);
      // A fresh cache each time: the cold path is the one to lay out.
      const env = { ...process.env, XDG_CACHE_HOME: join(work, `cache-${command}`) };
      execFileSync(tracer, [addresses, entered, "--", args.jet, command, "hello.jet"], { cwd: project, env, stdio: ["ignore", "ignore", "inherit"] });
      for (const line of readFileSync(entered, "utf8").split("\n")) {
        if (line === "") continue;
        const name = byAddress.get(BigInt(`0x${line}`));
        if (name !== undefined && !listed.has(name)) {
          listed.add(name);
          names.push(name);
        }
      }
    }
    const digest = createHash("sha256").update(jetBytes).digest("hex").slice(0, 16);
    const header = [
      "# Generated by `node Tools/perf/hot-text.mjs`; do not edit.",
      `# The ${names.length} functions a cold hello \`jet ${COMMANDS.join("`, `jet ")}\` entered, first entry first (jet sha256 ${digest}).`,
    ];
    writeFileSync(args.out, [...header, ...names].join("\n") + "\n");
    console.log(`${args.out}: ${names.length} functions`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

main();
