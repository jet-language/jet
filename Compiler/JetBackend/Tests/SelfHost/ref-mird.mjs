#!/usr/bin/env node
// Reference for selfmird.mjs: run-goldens.mjs stage 2 (entry, prune,
// pruneTables, convert, encode) verbatim on one small dump.
// usage: node ref-mird.mjs <dump.mir> <out.mird> [repo]
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [dump, out, repoArg] = process.argv.slice(2);
const repo = resolve(repoArg ?? "/home/nate/Projects/Github/jet/.agent-worktrees/self-host-probe");
const { Converter, encode, loadMirSchema, parseRustDebug } = await import(join(repo, "Compiler/JetBackend/Tests/mir-debug.mjs"));
const schema = loadMirSchema(repo);
function field(node, name) {
  return node?.k === "struct" ? node.fields.find(([f]) => f === name)?.[1] : undefined;
}
function idOf(node) {
  if (node?.k === "tuple" && node.items.length === 1) return idOf(node.items[0]);
  return node?.k === "num" ? node.v : null;
}
function entryFunction(program) {
  for (const artifact of field(program, "artifacts")?.items ?? []) {
    let entry = field(artifact, "entry");
    if (entry?.k === "tuple" && entry.name === "Some") entry = entry.items[0];
    let fn = field(entry, "function");
    if (fn?.k === "tuple" && fn.name === "Some") fn = fn.items[0];
    const id = idOf(fn);
    if (id) return id;
  }
  return null;
}
function prune(program, entryId) {
  const functions = field(program, "functions");
  const byId = new Map(functions.items.map((fn) => [idOf(field(fn, "id")), fn]));
  const keep = new Set([entryId]);
  const work = [entryId];
  while (work.length) {
    const fn = byId.get(work.pop());
    if (!fn) continue;
    const visit = (node) => {
      if (!node || typeof node !== "object") return;
      if (node.k === "tuple" && node.name === "MirFunctionId") {
        const id = idOf(node);
        if (id && !keep.has(id)) {
          keep.add(id);
          work.push(id);
        }
        return;
      }
      if (node.k === "struct") node.fields.forEach(([, value]) => visit(value));
      else if (node.k === "tuple" || node.k === "list") node.items.forEach(visit);
      else if (node.k === "map") node.entries.forEach(([key, value]) => (visit(key), visit(value)));
    };
    visit(fn);
  }
  functions.items = functions.items.filter((fn) => keep.has(idOf(field(fn, "id"))));
  pruneTables(program);
}
const TABLES = [
  ["types", "MirTypeId"],
  ["fields", "MirFieldId"],
  ["prelude_calls", "MirPreludeCallId"],
  ["core_calls", "MirCoreCallId"],
];
function typeIds(node, acc = []) {
  if (!node || typeof node !== "object") return acc;
  if (node.k === "tuple" && node.name === "MirTypeId") acc.push(idOf(node));
  else if (node.k === "struct") node.fields.forEach(([, value]) => typeIds(value, acc));
  else if (node.k === "tuple" || node.k === "list") node.items.forEach((item) => typeIds(item, acc));
  return acc;
}
function pruneTables(program) {
  const refs = new Map(TABLES.map(([, idName]) => [idName, new Set()]));
  const rows = new Map(TABLES.map(([table, idName]) => [idName, new Map((field(program, table)?.items ?? []).map((row) => [idOf(field(row, "id")), row]))]));
  const work = [...field(program, "functions").items];
  while (work.length) {
    const visit = (node) => {
      if (!node || typeof node !== "object") return;
      if (node.k === "tuple" && refs.has(node.name)) {
        const id = idOf(node);
        const seen = refs.get(node.name);
        if (id && !seen.has(id)) {
          seen.add(id);
          const row = rows.get(node.name).get(id);
          if (row) work.push(row);
        }
        return;
      }
      if (node.k === "struct") node.fields.forEach(([, value]) => visit(value));
      else if (node.k === "tuple" || node.k === "list") node.items.forEach(visit);
      else if (node.k === "map") node.entries.forEach(([key, value]) => (visit(key), visit(value)));
    };
    visit(work.pop());
  }
  for (const [table, idName] of TABLES) {
    const list = field(program, table);
    if (list) list.items = list.items.filter((row) => refs.get(idName).has(idOf(field(row, "id"))));
  }
  const instances = field(program, "type_instances");
  if (instances) instances.items = instances.items.filter((ty) => typeIds(ty).every((id) => refs.get("MirTypeId").has(id)));
}
const program = parseRustDebug(readFileSync(dump, "utf8"));
const entryId = entryFunction(program);
prune(program, entryId);
const value = new Converter(schema).convert(program, { k: "named", name: "MIRProgram" }, "program");
const lines = [BigInt(entryId).toString()];
encode(value, { k: "named", name: "MIRProgram" }, schema, lines);
writeFileSync(out, `${lines.join("\n")}\n`);
