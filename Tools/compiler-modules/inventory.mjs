#!/usr/bin/env node
// Cross-file reference inventory for the self-hosted compiler (card #3862).
//
// Reads every Compiler/**/*.jet source through the canonical front end
// (`jet inspect compiler lex|parse --json`), records each file's top-level
// items, and resolves every identifier use to the file that declares it.
// Output: the file graph, its strongly connected components (Jet forbids
// import cycles, E0604), and the package-level graph.
//
//   node Tools/compiler-modules/inventory.mjs [--json out.json] [--dot]
//
// JET_CMD selects the jet launcher (default: Tools/agent/jet-env jet).
// Front-end JSON is cached by content hash under
// ~/.cache/jet-dev/compiler-modules/ast.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const cacheDir = resolve(homedir(), ".cache/jet-dev/compiler-modules/ast");
const jetCmd = (process.env.JET_CMD ?? `${repo}/Tools/agent/jet-env jet`).split(/\s+/);

export const PACKAGES = [
  "JetFoundation", "JetLexer", "JetParser", "JetOptimizer",
  "JetSema", "JetCodegen", "JetEval", "JetDriver", "Bootstrap",
];

export function packageOf(file) {
  return file.split("/")[1];
}

export function compilerFiles() {
  const out = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith(".jet") && entry.name !== "package.jet") out.push(relative(repo, path));
    }
  };
  walk(join(repo, "Compiler"));
  return out.sort();
}

function frontEnd(file, op) {
  const bytes = readFileSync(join(repo, file));
  const hash = createHash("sha256").update(bytes).digest("hex").slice(0, 16);
  const cached = join(cacheDir, `${hash}.${op}.json`);
  if (!existsSync(cached)) {
    mkdirSync(cacheDir, { recursive: true });
    const text = execFileSync(jetCmd[0], [...jetCmd.slice(1), "inspect", "compiler", op, join(repo, file), "--json"], {
      maxBuffer: 1 << 30,
    });
    // Write-then-rename: a concurrent reader never sees a partial file.
    const temporary = `${cached}.${process.pid}.tmp`;
    writeFileSync(temporary, text);
    renameSync(temporary, cached);
  }
  const document = JSON.parse(readFileSync(cached, "utf8"));
  return document.compiler.value;
}

// Identifiers inside `{…}` string interpolation holes.
function interpolatedNames(text) {
  const names = [];
  let depth = 0;
  let hole = "";
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (ch === "\\") { i += 1; continue; }
    if (ch === "{") { if (depth > 0) hole += ch; depth += 1; continue; }
    if (ch === "}" && depth > 0) {
      depth -= 1;
      if (depth === 0) {
        for (const m of hole.matchAll(/(?<![.\w@])@?[A-Za-z_][A-Za-z0-9_]*/g)) names.push(m[0]);
        hole = "";
      } else hole += ch;
      continue;
    }
    if (depth > 0) hole += ch;
  }
  return names;
}

// For every token index, the set of names bound in its enclosing named
// function. A named function runs from its `fn` keyword to the brace closing
// its body; `fn(` function types have no name and no body.
function functionLocals(tokens) {
  const empty = new Set();
  const at = new Array(tokens.length).fill(empty);
  for (let f = 0; f < tokens.length; f += 1) {
    if (tokens[f].kind !== "keyword.fn" || tokens[f + 1]?.kind !== "identifier") continue;
    let k = f + 2;
    let nest = 0;
    for (; k < tokens.length; k += 1) {
      const kind = tokens[k].kind;
      if (kind === "punctuation.left_paren" || kind === "punctuation.left_bracket") nest += 1;
      else if (kind === "punctuation.right_paren" || kind === "punctuation.right_bracket") nest -= 1;
      else if (kind === "punctuation.left_brace" && nest === 0) break;
      else if (kind === "terminator" && nest === 0) { k = -1; break; }
    }
    if (k < 0 || k >= tokens.length) continue; // declaration without a body
    let depth = 0;
    let end = k;
    for (; end < tokens.length; end += 1) {
      if (tokens[end].kind === "punctuation.left_brace") depth += 1;
      else if (tokens[end].kind === "punctuation.right_brace" && --depth === 0) break;
    }
    const locals = new Set();
    const patternParens = [];
    for (let i = f + 2; i <= end && i < tokens.length; i += 1) {
      const t = tokens[i];
      if (t.kind === "punctuation.left_paren") {
        patternParens.push(tokens[i - 1]?.kind === "identifier" && tokens[i - 2]?.kind === "punctuation.dot");
        continue;
      }
      if (t.kind === "punctuation.right_paren") { patternParens.pop(); continue; }
      if (t.kind !== "identifier") continue;
      const next = tokens[i + 1]?.kind;
      const prev = tokens[i - 1]?.kind;
      if (next === "punctuation.colon" || next === "operator.bind_immutable" || next === "operator.bind_mutable") locals.add(t.text);
      else if (prev === "keyword.loop" || (prev === "punctuation.comma" && tokens[i - 3]?.kind === "keyword.loop")) locals.add(t.text);
      else if (patternParens.at(-1) && (prev === "punctuation.left_paren" || prev === "punctuation.comma")
        && (next === "punctuation.comma" || next === "punctuation.right_paren")) locals.add(t.text);
    }
    for (let i = f; i <= end && i < tokens.length; i += 1) at[i] = locals;
  }
  return (k) => at[k];
}

export function fileFacts(file) {
  const lex = frontEnd(file, "lex");
  const parse = frontEnd(file, "parse");
  const tokens = lex.tokens.filter((t) => !t.kind.startsWith("comment"));
  const errors = [...lex.diagnostics, ...parse.diagnostics].filter((d) => d.severity === "error");
  if (errors.length > 0) {
    // A file the front end cannot read yields no trustworthy item list.
    throw new Error(`${file}: ${errors[0].code} ${errors[0].message}`);
  }
  const items = parse.items.map((item) => {
    // `pub` is the token right before the declaring keyword (or before the name for consts).
    const at = tokens.findIndex((t) => t.span.start === item.span.start);
    let pub = false;
    for (let k = at - 1; k >= 0 && k >= at - 3; k -= 1) {
      if (tokens[k].kind === "keyword.pub") { pub = true; break; }
      if (tokens[k].kind === "terminator" || tokens[k].kind.startsWith("punctuation.right")) break;
    }
    return { kind: item.kind, name: item.name, start: item.span.start, pub };
  });
  const declStarts = new Set(items.map((item) => item.start));
  const uses = new Map();
  const note = (name) => uses.set(name, (uses.get(name) ?? 0) + 1);
  // Names bound inside one function (parameters, bindings, loop variables,
  // pattern bindings) shadow top-level items for the rest of that function.
  const localsAt = functionLocals(tokens);
  let depth = 0;
  let paren = 0;
  let enumBody = -1;
  let pendingEnum = false;
  for (let k = 0; k < tokens.length; k += 1) {
    const t = tokens[k];
    if (t.kind === "keyword.use") {
      // An import line names items; it is not a use of them.
      let nest = 0;
      while (k + 1 < tokens.length) {
        const kind = tokens[k + 1].kind;
        if (kind === "punctuation.left_bracket") nest += 1;
        else if (kind === "punctuation.right_bracket") nest -= 1;
        else if ((kind === "terminator" || kind === "eof") && nest === 0) break;
        k += 1;
      }
      continue;
    }
    if (t.kind === "keyword.enum") pendingEnum = true;
    if (t.kind === "punctuation.left_brace") {
      depth += 1;
      if (pendingEnum && paren === 0) { enumBody = depth; pendingEnum = false; }
      continue;
    }
    if (t.kind === "punctuation.right_brace") {
      if (depth === enumBody) enumBody = -1;
      depth -= 1;
      continue;
    }
    if (t.kind === "punctuation.left_paren") { paren += 1; continue; }
    if (t.kind === "punctuation.right_paren") { paren -= 1; continue; }
    if (t.kind === "literal.string") {
      for (const name of interpolatedNames(t.text)) if (!localsAt(k).has(name)) note(name);
      continue;
    }
    if (t.kind !== "identifier" || declStarts.has(t.span.start)) continue;
    if (depth === enumBody && paren === 0) continue; // variant declaration
    const prev = tokens[k - 1];
    const next = tokens[k + 1];
    if (prev && prev.kind === "punctuation.dot") continue; // field, method, variant
    if (next && (next.kind === "punctuation.colon" || next.kind === "operator.bind_immutable" || next.kind === "operator.bind_mutable")) {
      // label, field declaration, or binding — not a use (types after `:` are separate tokens)
      continue;
    }
    if (localsAt(k).has(t.text)) continue;
    note(t.text);
  }
  return { file, items, uses, tokens, lines: lex.source.split("\n").length };
}

export function inventory(files = compilerFiles()) {
  const facts = files.map(fileFacts);
  const owners = new Map();
  for (const fact of facts) {
    for (const item of fact.items) {
      if (item.kind === "impl") continue;
      if (!owners.has(item.name)) owners.set(item.name, []);
      owners.get(item.name).push(fact.file);
    }
  }
  const duplicates = [...owners].filter(([, where]) => new Set(where).size > 1);
  const edges = new Map(); // file -> Map(target file -> [names])
  for (const fact of facts) {
    const out = new Map();
    for (const [name] of fact.uses) {
      const where = owners.get(name);
      if (!where || where.includes(fact.file)) continue;
      for (const target of new Set(where)) {
        if (!out.has(target)) out.set(target, []);
        out.get(target).push(name);
      }
    }
    edges.set(fact.file, out);
  }
  return { facts, owners, duplicates, edges };
}

// Tarjan SCC over a Map(node -> iterable targets).
export function stronglyConnected(nodes, targetsOf) {
  let index = 0;
  const stack = [];
  const on = new Set();
  const idx = new Map();
  const low = new Map();
  const out = [];
  const visit = (v) => {
    idx.set(v, index); low.set(v, index); index += 1;
    stack.push(v); on.add(v);
    for (const w of targetsOf(v)) {
      if (!idx.has(w)) { visit(w); low.set(v, Math.min(low.get(v), low.get(w))); }
      else if (on.has(w)) low.set(v, Math.min(low.get(v), idx.get(w)));
    }
    if (low.get(v) === idx.get(v)) {
      const comp = [];
      let w;
      do { w = stack.pop(); on.delete(w); comp.push(w); } while (w !== v);
      out.push(comp.sort());
    }
  };
  for (const v of nodes) if (!idx.has(v)) visit(v);
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const { facts, duplicates, edges } = inventory();
  const files = facts.map((f) => f.file);
  const lines = new Map(facts.map((f) => [f.file, f.lines]));
  const sccs = stronglyConnected(files, (v) => edges.get(v).keys());
  const cyclic = sccs.filter((c) => c.length > 1).sort((a, b) => b.length - a.length);
  const pkgEdges = new Map(PACKAGES.map((p) => [p, new Map()]));
  for (const [from, out] of edges) {
    for (const [to, names] of out) {
      const a = packageOf(from);
      const b = packageOf(to);
      if (a === b) continue;
      const m = pkgEdges.get(a);
      m.set(b, (m.get(b) ?? 0) + names.length);
    }
  }
  const pkgSccs = stronglyConnected(PACKAGES, (p) => pkgEdges.get(p).keys());
  console.log(`files: ${files.length}; duplicate top-level names: ${duplicates.length}`);
  for (const [name, where] of duplicates) console.log(`  duplicate ${name}: ${where.join(", ")}`);
  console.log(`file SCCs with a cycle: ${cyclic.length}`);
  for (const comp of cyclic) {
    const size = comp.reduce((sum, f) => sum + lines.get(f), 0);
    const pkgs = [...new Set(comp.map(packageOf))].join(",");
    console.log(`  ${comp.length} files, ${size} lines, packages ${pkgs}`);
  }
  console.log("package edges (uses):");
  for (const [p, m] of pkgEdges) console.log(`  ${p} -> ${[...m].map(([q, n]) => `${q}(${n})`).join(" ")}`);
  console.log(`package SCCs: ${pkgSccs.map((c) => c.join("+")).join(" | ")}`);
  const jsonAt = process.argv.indexOf("--json");
  if (jsonAt > 0) {
    writeFileSync(process.argv[jsonAt + 1], JSON.stringify({
      files: facts.map((f) => ({ file: f.file, lines: f.lines, items: f.items })),
      edges: Object.fromEntries([...edges].map(([k, v]) => [k, Object.fromEntries(v)])),
      sccs: cyclic,
      packageEdges: Object.fromEntries([...pkgEdges].map(([k, v]) => [k, Object.fromEntries(v)])),
      packageSccs: pkgSccs,
    }, null, 1));
  }
}
