#!/usr/bin/env node
// Streaming JET_DUMP_MIR -> .mird converter for programs whose Rust Debug dump
// is too large for one JS string (the Jet compiler itself). Same semantics as
// run-goldens.mjs stages 2 (prune + convert + encode) with the worktree's
// mir-debug.mjs Converter, applied one top-level row at a time:
//   1. scan: byte offsets of every top-level MirProgram field and, for list
//      fields, of every row (strings, chars and escapes respected);
//   2. prune: functions reachable from the artifact entry through any
//      MirFunctionId they mention; types/fields/prelude/core rows reachable
//      from kept rows; type instances naming only kept types;
//   3. convert + encode every kept row in Jet schema field order.
// A row that does not convert is recorded in <out>.convert-errors (row kind,
// id, message) and left out; every kept function is listed in
// <out>.functions.tsv (id, key, name, source file id, span start, span end,
// converted) and every source file in <out>.sources.tsv (id, path).
// usage: node selfmird.mjs <dump.mir> <out prefix> [repo]
import { closeSync, openSync, readSync, writeFileSync, writeSync } from "node:fs";
import { join, resolve } from "node:path";

const [dumpPath, outPrefix, repoArg] = process.argv.slice(2);
if (!dumpPath || !outPrefix) throw new Error("usage: selfmird.mjs <dump.mir> <out prefix> [repo]");
const repo = resolve(repoArg ?? "/home/nate/Projects/Github/jet/.agent-worktrees/self-host-probe");
const { Converter, encode, loadMirSchema, parseRustDebug } = await import(join(repo, "Compiler/JetBackend/Tests/mir-debug.mjs"));
const schema = loadMirSchema(repo);
const started = Date.now();
const log = (message) => console.log(`[${Math.round((Date.now() - started) / 1000)}s] ${message}`);

// ---------------------------------------------------------------------------
// 1. Structural scan.
const fd = openSync(dumpPath, "r");
const CHUNK = 1 << 26;
const buf = Buffer.allocUnsafe(CHUNK);
const fields = []; // { name, start, end, items: [[s, e]] | null }
let depth = 0;
let inStr = false;
let inChar = false;
let escape = false;
let name = "";
let field = null; // field whose value is being read
let valueStart = -1;
let item = -1; // start of the current depth-2 row
let lastNonSpace = -1;
const isIdent = (b) => (b >= 48 && b <= 57) || (b >= 65 && b <= 90) || (b >= 97 && b <= 122) || b === 95;
const isSpace = (b) => b === 32 || b === 10 || b === 13 || b === 9;
let base = 0;
for (;;) {
  const got = readSync(fd, buf, 0, CHUNK, base);
  if (got === 0) break;
  for (let i = 0; i < got; i++) {
    const b = buf[i];
    const at = base + i;
    if (inStr || inChar) {
      if (escape) escape = false;
      else if (b === 92) escape = true;
      else if ((inStr && b === 34) || (inChar && b === 39)) {
        inStr = false;
        inChar = false;
        lastNonSpace = at;
      }
      continue;
    }
    if (isSpace(b)) continue;
    if (depth === 1 && field === null) {
      // Reading a field name: `name: value`.
      if (isIdent(b)) {
        name += String.fromCharCode(b);
        continue;
      }
      if (b === 58) {
        field = { name, start: -1, end: -1, items: null };
        name = "";
        valueStart = -1;
        continue;
      }
      if (b === 44) continue;
      if (b === 125) {
        depth = 0;
        continue;
      }
      throw new Error(`scan: unexpected byte ${String.fromCharCode(b)} at ${at} reading a field name`);
    }
    if (depth === 1 && valueStart < 0) valueStart = at;
    if (depth === 2 && field?.items && item < 0 && b !== 44 && b !== 93) item = at;
    if (b === 34) inStr = true;
    else if (b === 39) inChar = true;
    else if (b === 91 || b === 40 || b === 123) {
      depth++;
      if (depth === 2 && field && b === 91 && at === valueStart) field.items = [];
    } else if (b === 93 || b === 41 || b === 125) {
      if (depth === 2 && field?.items && item >= 0) {
        field.items.push([item, lastNonSpace + 1]);
        item = -1;
      }
      depth--;
      if (depth === 0) {
        // `}` closing MirProgram ends the last field without a comma.
        if (field) {
          field.start = valueStart;
          field.end = lastNonSpace + 1;
          fields.push(field);
          field = null;
        }
        lastNonSpace = at;
        continue;
      }
    } else if (b === 44) {
      if (depth === 2 && field?.items && item >= 0) {
        field.items.push([item, lastNonSpace + 1]);
        item = -1;
      } else if (depth === 1 && field) {
        field.start = valueStart;
        field.end = lastNonSpace + 1;
        fields.push(field);
        field = null;
      }
    }
    lastNonSpace = at;
  }
  base += got;
}
if (depth !== 0 || inStr || inChar) throw new Error(`scan: dump ends inside a value (depth ${depth})`);
log(`scanned ${base} bytes: ${fields.map((f) => `${f.name}${f.items ? `[${f.items.length}]` : ""}`).join(" ")}`);

const readText = ([s, e]) => {
  const out = Buffer.allocUnsafe(e - s);
  let done = 0;
  while (done < out.length) done += readSync(fd, out, done, out.length - done, s + done);
  return out.toString("utf8");
};
const byName = new Map(fields.map((f) => [f.name, f]));
const rows = (fieldName) => byName.get(fieldName)?.items ?? [];
const OWN_ID = /^\s*[A-Za-z_]\w*\s*\{\s*id:\s*[A-Za-z_]\w*\(\s*(\d+)/;
const ownId = (text, where) => {
  const m = OWN_ID.exec(text);
  if (!m) throw new Error(`${where}: row does not start with its id`);
  return m[1];
};
const refs = (text, idName) => {
  const out = [];
  const re = new RegExp(`\\b${idName}\\(\\s*(\\d+)`, "g");
  let m;
  while ((m = re.exec(text))) out.push(m[1]);
  return out;
};

// ---------------------------------------------------------------------------
// 2. Entry and pruning.
const rustField = (node, key) => (node?.k === "struct" ? node.fields.find(([k]) => k === key)?.[1] : undefined);
const idOf = (node) => (node?.k === "tuple" && node.items.length === 1 ? idOf(node.items[0]) : node?.k === "num" ? node.v : null);
const artifacts = parseRustDebug(readText([byName.get("artifacts").start, byName.get("artifacts").end]));
let entryId = null;
for (const artifact of artifacts.items ?? []) {
  let entry = rustField(artifact, "entry");
  if (entry?.k === "tuple" && entry.name === "Some") entry = entry.items[0];
  let fn = rustField(entry, "function");
  if (fn?.k === "tuple" && fn.name === "Some") fn = fn.items[0];
  const id = idOf(fn);
  if (id) {
    entryId = id;
    break;
  }
}
if (!entryId) throw new Error("no artifact names an entry function");

const functionRows = new Map(); // id -> row index
const functionRefs = [];
rows("functions").forEach((range, index) => {
  const text = readText(range);
  const id = ownId(text, `functions[${index}]`);
  functionRows.set(id, index);
  functionRefs.push(refs(text, "MirFunctionId"));
});
// SELFMIRD_ROOTS=all keeps every function (package slices root anywhere).
const roots = process.env.SELFMIRD_ROOTS === "all" ? [...functionRows.keys()] : [entryId];
const keepFunctions = new Set(roots);
const work = [...roots];
while (work.length) {
  const row = functionRows.get(work.pop());
  if (row === undefined) continue;
  for (const id of functionRefs[row]) {
    if (!keepFunctions.has(id)) {
      keepFunctions.add(id);
      work.push(id);
    }
  }
}
log(`entry ${entryId}: ${keepFunctions.size} of ${functionRows.size} functions reachable`);

const TABLES = [
  ["types", "MirTypeId"],
  ["fields", "MirFieldId"],
  ["prelude_calls", "MirPreludeCallId"],
  ["core_calls", "MirCoreCallId"],
];
const tableRows = new Map(TABLES.map(([table, idName]) => [idName, new Map(rows(table).map((range, index) => [ownId(readText(range), `${table}[${index}]`), range]))]));
const seen = new Map(TABLES.map(([, idName]) => [idName, new Set()]));
const pending = [];
const visit = (text) => {
  for (const [, idName] of TABLES) {
    for (const id of refs(text, idName)) {
      const set = seen.get(idName);
      if (set.has(id)) continue;
      set.add(id);
      const range = tableRows.get(idName).get(id);
      if (range) pending.push(range);
    }
  }
};
for (const id of keepFunctions) {
  const row = functionRows.get(id);
  if (row !== undefined) visit(readText(rows("functions")[row]));
}
while (pending.length) visit(readText(pending.pop()));
const keptTypes = seen.get("MirTypeId");
const noPrune = process.env.SELFMIRD_NOPRUNE === "1";
const keepRow = (table, range) => {
  if (noPrune) return true;
  const pair = TABLES.find(([t]) => t === table);
  if (table === "functions") return keepFunctions.has(ownId(readText(range), table));
  if (pair) return seen.get(pair[1]).has(ownId(readText(range), table));
  if (table === "type_instances") return refs(readText(range), "MirTypeId").every((id) => keptTypes.has(id));
  return true;
};
log(`kept ${TABLES.map(([table, idName]) => `${table} ${[...tableRows.get(idName).keys()].filter((id) => seen.get(idName).has(id)).length}/${tableRows.get(idName).size}`).join(", ")}`);

// ---------------------------------------------------------------------------
// 3. Convert and encode in schema order.
const converter = new Converter(schema);
converter.trapRoutes = new Set();
for (const range of rows("prelude_calls")) {
  const row = parseRustDebug(readText(range));
  const family = rustField(row, "family");
  const member = rustField(row, "member");
  if (family?.name === "Overflow" && /^(?:i|u)(?:8|16|32|64)\.trap\.(?:add|sub|mul|div|pow|shl|shr)$/.test(member?.v ?? "")) converter.trapRoutes.add(idOf(rustField(row, "id")));
}

const outFd = openSync(`${outPrefix}.mird`, "w");
let pendingOut = [];
let pendingBytes = 0;
const flush = () => {
  if (pendingOut.length === 0) return;
  writeSync(outFd, pendingOut.join("\n") + "\n");
  pendingOut = [];
  pendingBytes = 0;
};
const emit = (lines) => {
  for (const line of lines) {
    pendingOut.push(line);
    pendingBytes += line.length + 1;
  }
  if (pendingBytes > 1 << 24) flush();
};
emit([BigInt(entryId).toString()]);
const errors = [];
const functionsTsv = [];
const sourcesTsv = [];
const program = schema.get("MIRProgram");
const fold = (text) => text.toLowerCase().replace(/_/g, "");
for (const f of program.fields) {
  const rust = fields.find((candidate) => fold(candidate.name) === fold(f.name));
  if (!rust) throw new Error(`MIRProgram.${f.name}: missing checked Rust field; regenerate the MIR dump with the current compiler`);
  if (f.type.k === "list" && rust.items) {
    const converted = [];
    for (const [index, range] of rust.items.entries()) {
      if (!keepRow(rust.name, range)) continue;
      const text = readText(range);
      const node = parseRustDebug(text);
      let value = null;
      try {
        value = converter.convert(node, f.type.of, `program.${f.name}`);
      } catch (error) {
        errors.push(`${rust.name}\t${idOf(rustField(node, "id")) ?? index}\t${error.message}`);
      }
      if (rust.name === "functions") {
        const span = rustField(node, "span");
        functionsTsv.push([idOf(rustField(node, "id")), rustField(node, "key")?.v ?? "", rustField(node, "name")?.v ?? "", idOf(rustField(node, "source_file")), idOf(rustField(span, "start")), idOf(rustField(span, "end")), value ? 1 : 0].join("\t"));
      }
      if (rust.name === "source_files") sourcesTsv.push(`${idOf(rustField(node, "id"))}\t${rustField(node, "path")?.v ?? ""}`);
      if (value === null) continue;
      const lines = [];
      encode(value, f.type.of, schema, lines);
      converted.push(lines);
    }
    emit([String(converted.length)]);
    for (const lines of converted) emit(lines);
    log(`${f.name}: ${converted.length} rows`);
  } else {
    const node = parseRustDebug(readText([rust.start, rust.end]));
    const value = converter.convert(node, f.type, `program.${f.name}`);
    const lines = [];
    encode(value, f.type, schema, lines);
    emit(lines);
  }
}
flush();
closeSync(outFd);
closeSync(fd);
writeFileSync(`${outPrefix}.convert-errors`, errors.join("\n") + (errors.length ? "\n" : ""));
writeFileSync(`${outPrefix}.functions.tsv`, functionsTsv.join("\n") + "\n");
writeFileSync(`${outPrefix}.sources.tsv`, sourcesTsv.join("\n") + "\n");
log(`wrote ${outPrefix}.mird; ${functionsTsv.length} functions, ${errors.length} conversion errors`);
