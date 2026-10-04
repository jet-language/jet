#!/usr/bin/env node
// Prints the reports of a generated-compiler receipt, one per line:
//   CODE<TAB>location<TAB>message
// usage: reports.mjs RECEIPT [compiler.map.json]
// With the assembler's source map, a span in the aggregate unit
// (src/compiler.jet) is mapped back to Compiler/<file>.jet:<line>.
import { readFileSync } from "node:fs";

const [receiptPath, mapPath] = process.argv.slice(2);
if (!receiptPath) {
  console.error("usage: reports.mjs RECEIPT [compiler.map.json]");
  process.exit(64);
}
const map = mapPath ? JSON.parse(readFileSync(mapPath, "utf8")) : null;
const isUnit = (file) => map && String(file).endsWith("src/compiler.jet");

// A byte offset in the aggregate unit -> Compiler/<file>.jet:<line>.
function mappedOffset(file, offset) {
  if (!isUnit(file)) return null;
  const segment = map.source_segments.find((s) => offset >= s.generated.start && offset <= s.generated.end);
  if (!segment) return null;
  const local = offset - segment.generated.start;
  let line = 0;
  while (line + 1 < segment.line_starts.length && segment.line_starts[line + 1] <= local) line += 1;
  return `${segment.source_file}:${line + 1}`;
}

// A 1-based line of the aggregate unit -> Compiler/<file>.jet:<line>.
function mappedLine(file, line) {
  if (!isUnit(file)) return null;
  const source = map.files.find((f) => line >= f.generated_start_line && line <= f.generated_end_line);
  return source ? `${source.path}:${line - source.generated_start_line + 1}` : null;
}

for (const line of readFileSync(receiptPath, "utf8").split("\n")) {
  if (!line.startsWith("report_json=")) continue;
  const json = line.slice("report_json=".length);
  let report;
  try {
    report = JSON.parse(json);
  } catch {
    console.log(`?\t?\t${json}`);
    continue;
  }
  const file = report.file ?? "";
  const span = report.span;
  const location = span
    ? (mappedOffset(file, span.start) ?? `${file}:${span.start}..${span.end}`)
    : report.line ? (mappedLine(file, report.line) ?? `${file}:${report.line}`) : (file || "-");
  const message = report.message ?? report.title ?? report.what ?? "";
  console.log(`${report.code ?? "?"}\t${location}\t${String(message).replaceAll("\n", " ").slice(0, 300)}`);
}
