#!/usr/bin/env node
// Map diagnostics in an assembled Jetpack unit back to the original source files.
//   locate.mjs syntax <map.json>          reads the syntax probe's JSON lines on stdin
//   locate.mjs check  <map.json> <log>    reads a `jet check` log
// Prints `path:line:col: CODE message` per error, then error counts per file.
import { readFileSync } from "node:fs";

const [mode, mapPath, logPath] = process.argv.slice(2);
const map = JSON.parse(readFileSync(mapPath, "utf8"));

function fromByte(byte) {
  for (const segment of map.source_segments) {
    if (byte < segment.generated.start || byte >= segment.generated.end) continue;
    const local = byte - segment.generated.start;
    let line = 0;
    while (line + 1 < segment.line_starts.length && segment.line_starts[line + 1] <= local) line += 1;
    return { path: segment.source_file, line: line + 1, column: local - segment.line_starts[line] + 1 };
  }
  return null;
}

function fromLine(generatedLine, column) {
  for (const file of map.files) {
    if (generatedLine < file.generated_start_line || generatedLine > file.generated_end_line) continue;
    return { path: file.path, line: generatedLine - file.generated_start_line + 1, column };
  }
  return null;
}

const found = [];
if (mode === "syntax") {
  for (const raw of readFileSync(0, "utf8").split("\n")) {
    if (!raw.startsWith("{")) continue;
    const row = JSON.parse(raw);
    if (row.severity !== "error" || !row.span) continue;
    const where = fromByte(row.span.start) ?? { path: "(generated)", line: 0, column: 0 };
    found.push({ ...where, code: row.code, what: row.what });
  }
} else if (mode === "check") {
  const lines = readFileSync(logPath, "utf8").split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const head = /^Error \[(E\d+)\]: (.*)$/.exec(lines[index]);
    if (!head) continue;
    for (let next = index + 1; next < Math.min(index + 4, lines.length); next += 1) {
      const at = /-->\s+(\S+?):(\d+):(\d+)/.exec(lines[next]);
      if (!at) continue;
      const where = at[1] === "src/jetpack.jet" ? fromLine(Number(at[2]), Number(at[3])) : { path: at[1], line: Number(at[2]), column: Number(at[3]) };
      found.push({ ...(where ?? { path: "(generated)", line: Number(at[2]), column: 0 }), code: head[1], what: head[2] });
      break;
    }
  }
} else {
  console.error("usage: locate.mjs syntax <map.json> < probe-output | locate.mjs check <map.json> <check.log>");
  process.exit(64);
}

for (const error of found) console.log(`${error.path}:${error.line}:${error.column}: ${error.code} ${error.what}`);
const perFile = new Map();
for (const error of found) perFile.set(error.path, (perFile.get(error.path) ?? 0) + 1);
if (perFile.size > 0) {
  console.log("errors per file:");
  for (const [path, count] of [...perFile].sort((a, b) => b[1] - a[1])) console.log(`  ${String(count).padStart(5)}  ${path}`);
}
console.log(`total errors: ${found.length}`);
