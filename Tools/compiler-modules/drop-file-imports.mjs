#!/usr/bin/env node
// D-MOD-CYCLE1=A cutover for package files that still import sibling files by
// relative path. The files of one package share one namespace, so the import
// line goes away and `alias.member` becomes `member`. Member-list imports of
// that alias (`use alias.[a, b]`) go away too.
//
//   node Tools/compiler-modules/drop-file-imports.mjs [--apply] <file.jet>...
//
// Token-based: it reads each file through `jet inspect compiler lex --json`
// (JET_CMD selects the launcher) and edits only the import statements and the
// `alias` token plus its following dot, so strings and comments are untouched.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const jetCmd = (process.env.JET_CMD ?? `${repo}/Tools/agent/jet-env jet`).split(/\s+/);
const args = process.argv.slice(2);
const apply = args.includes("--apply");
const files = args.filter((arg) => arg !== "--apply");

function tokens(path) {
  const text = execFileSync(jetCmd[0], [...jetCmd.slice(1), "inspect", "compiler", "lex", path, "--json"], { maxBuffer: 1 << 30 });
  return JSON.parse(text).compiler.value.tokens.filter((token) => !token.kind.startsWith("comment"));
}

// One `use` statement: token range and what it names.
function statements(toks) {
  const out = [];
  for (let k = 0; k < toks.length; k += 1) {
    if (toks[k].kind !== "keyword.use") continue;
    let end = k;
    let nest = 0;
    while (end + 1 < toks.length) {
      const kind = toks[end + 1].kind;
      if (kind === "punctuation.left_bracket") nest += 1;
      else if (kind === "punctuation.right_bracket") nest -= 1;
      else if ((kind === "terminator" || kind === "eof") && nest === 0) break;
      end += 1;
    }
    out.push({ from: k, to: end, toks: toks.slice(k + 1, end + 1) });
    k = end;
  }
  return out;
}

let changed = 0;
for (const file of files) {
  const path = resolve(file);
  const bytes = readFileSync(path);
  const toks = tokens(path);
  const aliases = new Set();
  const drop = [];
  for (const statement of statements(toks)) {
    const [head, ...rest] = statement.toks;
    if (head?.kind === "literal.string") {
      const target = JSON.parse(head.text);
      if (target.endsWith(".h")) continue;
      const asAt = rest.findIndex((t) => t.kind === "identifier" && t.text === "as");
      aliases.add(asAt >= 0 ? rest[asAt + 1].text : basename(target));
      drop.push(statement);
    }
  }
  for (const statement of statements(toks)) {
    const [head, dot] = statement.toks;
    if (head?.kind === "identifier" && aliases.has(head.text) && dot?.kind === "punctuation.dot") drop.push(statement);
  }
  if (aliases.size === 0) continue;
  const edits = [];
  for (const statement of drop) {
    let end = toks[statement.to].span.end;
    if (bytes[end] === 0x0a) end += 1;
    edits.push([toks[statement.from].span.start, end]);
  }
  const dropped = new Set(drop.flatMap((statement) => toks.slice(statement.from, statement.to + 1)));
  for (let k = 0; k + 1 < toks.length; k += 1) {
    const t = toks[k];
    if (dropped.has(t) || t.kind !== "identifier" || !aliases.has(t.text)) continue;
    if (toks[k + 1].kind !== "punctuation.dot" || toks[k - 1]?.kind === "punctuation.dot") continue;
    edits.push([t.span.start, toks[k + 1].span.end]);
  }
  edits.sort((a, b) => b[0] - a[0]);
  let out = bytes;
  for (const [start, end] of edits) out = Buffer.concat([out.subarray(0, start), out.subarray(end)]);
  changed += 1;
  console.log(`${apply ? "rewrote" : "would rewrite"} ${file} (aliases ${[...aliases].join(", ")})`);
  if (apply) writeFileSync(path, out);
}
console.log(`${changed} file(s)`);
