#!/usr/bin/env node
// Source coverage, not execution coverage: a call site is evidence of use, not
// proof that a test passed. Comments, expected output and ordinary strings do
// not count. Jet interpolation expressions and host-embedded Jet programs do.
// Every public declaration (including methods) has its own row. Unresolved
// dynamic receivers remain uncovered rather than crediting same-named methods.
//
// node Tools/agent/core-coverage-census.mjs [--check|--write] [--json]
// --check gates only increases in the total uncovered count, not report drift.
// --write initializes or lowers the committed ceiling; it never raises it.
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const RATCHET = "Tools/agent/core-coverage-ratchet.json";
const identifier = /^[A-Za-z_][A-Za-z_0-9]*$/;

export function moduleName(path) {
  const parts = path.replaceAll("\\", "/").replace(/^Core\//, "").replace(/\.jet$/, "").split("/");
  if (parts.at(-1) === parts.at(-2)) parts.pop();
  return parts.join(".") === "app" ? "app" : `core.${parts.join(".")}`;
}

// A small lexer keeps prose and literals out of both the inventory and uses.
// Offsets are retained so the report can point to the actual evidence.
function lex(source, base = 0) {
  const tokens = [];
  let i = 0;
  while (i < source.length) {
    if (/\s/.test(source[i])) { i++; continue; }
    if (source.startsWith("//", i)) {
      const end = source.indexOf("\n", i);
      i = end < 0 ? source.length : end;
      continue;
    }
    if (source.startsWith("/*", i)) {
      i += 2;
      let depth = 1;
      while (i < source.length && depth) {
        if (source.startsWith("/*", i)) { depth++; i += 2; }
        else if (source.startsWith("*/", i)) { depth--; i += 2; }
        else i++;
      }
      continue;
    }
    if (source[i] === '"' || source[i] === "'") {
      const quote = source[i++];
      while (i < source.length && source[i] !== quote) {
        if (source[i] === "\\") { i += 2; continue; }
        if (quote === '"' && source[i] === "{" && source[i + 1] !== "{") {
          const start = ++i;
          let depth = 1;
          let innerQuote = null;
          while (i < source.length && depth) {
            const c = source[i++];
            if (c === "\\") { i++; continue; }
            if (innerQuote) { if (c === innerQuote) innerQuote = null; }
            else if (c === '"' || c === "'") innerQuote = c;
            else if (c === "{") depth++;
            else if (c === "}") depth--;
          }
          tokens.push(...lex(source.slice(start, i - 1), base + start));
        } else if (source.startsWith("{{", i)) i += 2;
        else i++;
      }
      i++;
      continue;
    }
    const match = source.slice(i).match(/^[A-Za-z_][A-Za-z_0-9]*|^(?:::|:=|->)|^./s)[0];
    tokens.push({ value: match, offset: base + i });
    i += match.length;
  }
  return tokens;
}

function lineAt(source, offset) { return source.slice(0, offset).split("\n").length; }

function resultType(source, tokens, at, module, localTypes) {
  const start = at;
  if (!tokens[at]) return null;
  if (["&", "^"].includes(tokens[at].value)) at++;
  const first = tokens[at]?.value;
  if (identifier.test(first ?? "")) {
    const named = chain(tokens, at);
    at = afterGenerics(tokens, named.end);
  } else if (["[", "("].includes(first)) {
    const closers = [];
    const pairs = { "[": "]", "(": ")", "<": ">" };
    do {
      const value = tokens[at++]?.value;
      if (pairs[value]) closers.push(pairs[value]);
      else if (value === closers.at(-1)) closers.pop();
    } while (at < tokens.length && closers.length);
  } else return null;
  if (tokens[at]?.value === "?") at++;
  const spelling = source.slice(tokens[start].offset, tokens[at - 1].offset + tokens[at - 1].value.length);
  // Only declarations in this module justify qualification. Builtins,
  // prelude names, type parameters and compound types keep their spelling.
  return localTypes.has(first) && start === at - 1 ? `${module}.${spelling}` : spelling;
}

export function publicFunctions(path, source) {
  const module = moduleName(path);
  const tokens = lex(source);
  const localTypes = new Set(tokens.flatMap((token, index) =>
    ["struct", "enum", "trait"].includes(token.value) ? [tokens[index + 1]?.value] : []));
  const rows = [];
  const scopes = [];
  let pendingOwner = null;
  for (let i = 0; i < tokens.length; i++) {
    const value = tokens[i].value;
    if (["struct", "enum", "impl", "trait"].includes(value)) pendingOwner = tokens[i + 1]?.value;
    if (value === "{") { scopes.push(pendingOwner); pendingOwner = null; }
    if (value === "}") scopes.pop();
    if (value !== "pub" || tokens[i + 1]?.value !== "fn") continue;
    const name = tokens[i + 2]?.value;
    if (!identifier.test(name ?? "")) throw new Error(`unreadable public function in ${path}:${lineAt(source, tokens[i].offset)}`);
    const owner = scopes.at(-1);
    let at = afterGenerics(tokens, i + 3);
    let depth = 0;
    do {
      if (tokens[at]?.value === "(") depth++;
      if (tokens[at]?.value === ")") depth--;
      at++;
    } while (at < tokens.length && depth > 0);
    let result = null;
    if (tokens[at]?.value === "->") result = resultType(source, tokens, at + 1, module, localTypes);
    else if (tokens[at]?.value === "-" && tokens[at + 1]?.value === "[") {
      while (at < tokens.length && tokens[at].value !== "]") at++;
      if (tokens[at + 1]?.value === ">") result = resultType(source, tokens, at + 2, module, localTypes);
    }
    rows.push({ module, name: owner ? `${owner}.${name}` : name, id: `${module}.${owner ? `${owner}.` : ""}${name}`,
      file: path, line: lineAt(source, tokens[i].offset), returnType: result,
      doctests: [], examples: [], tests: [] });
  }
  return rows;
}

function chain(tokens, start) {
  if (!identifier.test(tokens[start]?.value ?? "")) return null;
  const parts = [tokens[start].value];
  let end = start + 1;
  while (tokens[end]?.value === "." && identifier.test(tokens[end + 1]?.value ?? "")) {
    parts.push(tokens[end + 1].value); end += 2;
  }
  return { parts, end };
}

function afterGenerics(tokens, at) {
  if (tokens[at]?.value !== "<") return at;
  let depth = 0;
  for (let i = at; i < tokens.length; i++) {
    if (tokens[i].value === "<") depth++;
    if (tokens[i].value === ">" && --depth === 0) return i + 1;
  }
  return at;
}

export function callSites(source, functions, context = null) {
  const byId = new Map(functions.map((row) => [row.id, row]));
  const types = new Set(functions.filter((row) => row.name.includes(".")).map((row) => row.id.slice(0, row.id.lastIndexOf("."))));
  const tokens = lex(source);
  const initial = new Map([["core", "core"], ["app", "app"]]);
  for (const row of functions) {
    if (row.module === "core.prelude" && !row.name.includes(".")) initial.set(row.name, row.id);
    if (context && row.module === context) initial.set(row.name.split(".")[0], `${context}.${row.name.split(".")[0]}`);
  }
  const scopes = [{ aliases: initial, receivers: new Map() }];
  const calls = [];
  const returns = new Map();
  const expand = (parts, aliases) => [aliases.get(parts[0]) ?? parts[0], ...parts.slice(1)].join(".");
  for (let i = 0; i < tokens.length; i++) {
    const scope = scopes.at(-1);
    const value = tokens[i].value;
    if (value === "{") { scopes.push({ aliases: new Map(scope.aliases), receivers: new Map(scope.receivers) }); continue; }
    if (value === "}") { if (scopes.length > 1) scopes.pop(); continue; }
    if (value === "use") {
      const imported = chain(tokens, i + 1);
      if (!imported) continue;
      const path = expand(imported.parts, scope.aliases);
      let at = imported.end;
      if (tokens[at]?.value === "." && tokens[at + 1]?.value === "[") {
        at += 2;
        while (at < tokens.length && tokens[at].value !== "]") {
          const member = tokens[at++].value;
          if (!identifier.test(member)) continue;
          let alias = member;
          if (tokens[at]?.value === "as") { alias = tokens[at + 1].value; at += 2; }
          scope.aliases.set(alias, `${path}.${member}`);
        }
        at++;
      } else {
        const alias = tokens[at]?.value === "as" ? tokens[at + 1]?.value : imported.parts.at(-1);
        scope.aliases.set(alias, path);
        if (tokens[at]?.value === "as") at += 2;
      }
      i = at - 1;
      continue;
    }
    // Explicit parameter/local types and constructor/function initializers make
    // receiver calls attributable without guessing by the method's spelling.
    if (identifier.test(value) && [":", "::", ":="].includes(tokens[i + 1]?.value)) {
      let at = i + 2;
      while (["&", "^", "~", "prep"].includes(tokens[at]?.value)) at++;
      const rhs = chain(tokens, at);
      if (rhs) {
        const target = expand(rhs.parts, scope.aliases);
        const type = types.has(target) ? target : byId.get(target)?.returnType;
        if (type) scope.receivers.set(value, type);
      }
    }
    if (tokens[i - 1]?.value === "." || tokens[i - 1]?.value === "fn") continue;
    const candidate = chain(tokens, i);
    if (!candidate) continue;
    const at = afterGenerics(tokens, candidate.end);
    if (tokens[at]?.value !== "(") continue;
    const { parts } = candidate;
    const target = scope.receivers.has(parts[0]) && parts.length > 1
      ? [scope.receivers.get(parts[0]), ...parts.slice(1)].join(".") : expand(parts, scope.aliases);
    const row = byId.get(target);
    if (row) calls.push({ id: row.id, offset: tokens[i].offset });
    let close = at + 1;
    let depth = 1;
    while (close < tokens.length && depth) {
      if (tokens[close].value === "(") depth++;
      if (tokens[close].value === ")") depth--;
      close++;
    }
    if (row?.returnType || types.has(target)) returns.set(close - 1, row?.returnType ?? target);
    i = candidate.end - 1;
  }
  for (const [close, type] of returns) {
    if (tokens[close + 1]?.value !== ".") continue;
    const member = tokens[close + 2]?.value;
    const at = afterGenerics(tokens, close + 3);
    const row = byId.get(`${type}.${member}`);
    if (row && tokens[at]?.value === "(") calls.push({ id: row.id, offset: tokens[close + 2].offset });
  }
  return calls;
}

function doctestPrograms(source) {
  const programs = [];
  const docs = /^\s*\/\/\/ ?(.*)$/gm;
  let block = "";
  let previousEnd = -1;
  const flush = () => {
    for (const match of block.matchAll(/```(?:jet)?[ \t]*\n([\s\S]*?)```/g)) programs.push(match[1]);
    block = "";
  };
  for (const match of source.matchAll(docs)) {
    if (previousEnd >= 0 && source.slice(previousEnd, match.index).trim()) flush();
    block += `${match[1]}\n`;
    previousEnd = match.index + match[0].length;
  }
  flush();
  for (const match of source.matchAll(/\/\*\*([\s\S]*?)\*\//g)) {
    block = match[1].replace(/^\s*\* ?/gm, ""); flush();
  }
  return programs;
}

// Host tests often contain several independent Jet programs. Each literal is
// scanned separately so a reused alias in one test cannot cover another API.
export function embeddedPrograms(source) {
  const programs = [];
  const literals = /\/\/[^\n]*|\/\*[\s\S]*?\*\/|r(#+)?"([\s\S]*?)"\1|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\])*`/g;
  for (const match of source.matchAll(literals)) {
    const literal = match[0];
    if (literal.startsWith("/")) continue;
    let text;
    if (literal.startsWith("r")) text = match[2];
    else if (literal.startsWith("`")) text = literal.slice(1, -1);
    else {
      try { text = JSON.parse(literal); } catch { continue; }
    }
    if (/\bfn\s+\w+|\buse\s+(?:core|app)\b/.test(text)) programs.push(text);
  }
  return programs;
}

function filesUnder(root, path) {
  return readdirSync(resolve(root, path), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name)).flatMap((entry) => {
    if (entry.name.startsWith(".") || ["expected", "node_modules", "target"].includes(entry.name)) return [];
    const child = `${path}/${entry.name}`;
    return entry.isDirectory() ? filesUnder(root, child) : entry.isFile() ? [child] : [];
  });
}

export function census(root = ROOT) {
  const core = filesUnder(root, "Core").filter((path) => path.endsWith(".jet"));
  const sources = new Map(core.map((path) => [path, readFileSync(resolve(root, path), "utf8")]));
  const functions = core.flatMap((path) => publicFunctions(path, sources.get(path)));
  const byId = new Map(functions.map((row) => [row.id, row]));
  if (!functions.length || byId.size !== functions.length) throw new Error("Core census is empty or contains duplicate public function identities");
  const record = (program, file, kind, context = null) => {
    for (const call of callSites(program, functions, context)) {
      const evidence = `${file}:${lineAt(program, call.offset)}`;
      const bucket = byId.get(call.id)[kind];
      if (!bucket.includes(evidence)) bucket.push(evidence);
    }
  };
  for (const [path, source] of sources) {
    doctestPrograms(source).forEach((program, index) => record(program, `${path}#doctest-${index + 1}`, "doctests", moduleName(path)));
  }
  for (const [directory, kind] of [["Examples/features", "examples"], ["tests", "tests"]]) {
    for (const path of filesUnder(root, directory)) {
      const extension = extname(path);
      if (![".jet", ".rs", ".mjs", ".js"].includes(extension)) continue;
      const source = readFileSync(resolve(root, path), "utf8");
      if (extension === ".jet") record(source, path, kind);
      else embeddedPrograms(source).forEach((program, index) => record(program, `${path}#program-${index + 1}`, kind));
    }
  }
  const modules = [...new Set(functions.map((row) => row.module))].sort().map((module) => {
    const rows = functions.filter((row) => row.module === module);
    const count = (kind) => rows.filter((row) => row[kind].length).length;
    return { module, public: rows.length, doctests: count("doctests"), examples: count("examples"), tests: count("tests"),
      uncovered: rows.filter((row) => !row.doctests.length && !row.examples.length && !row.tests.length).length };
  });
  return { modules, total: { public: functions.length, uncovered: modules.reduce((sum, row) => sum + row.uncovered, 0) }, functions };
}

export function ratchetDecision(uncovered, previous) {
  if (!Number.isSafeInteger(uncovered) || uncovered < 0) throw new Error("invalid uncovered count");
  if (previous !== null && (previous.version !== 1 || !Number.isSafeInteger(previous.uncovered) || previous.uncovered < 0)) throw new Error("invalid Core coverage ratchet");
  return { passes: previous !== null && uncovered <= previous.uncovered,
    next: { version: 1, uncovered: Math.min(uncovered, previous?.uncovered ?? uncovered) } };
}

export function main(argv = process.argv.slice(2), root = ROOT) {
  try {
    if (argv.includes("--help") || argv.includes("-h")) {
      console.log("usage: node Tools/agent/core-coverage-census.mjs [--check|--write] [--json]\n--check: reject an uncovered-count regression\n--write: initialize or lower the ratchet\n--json: include each declaration and its source evidence");
      return 0;
    }
    if (argv.some((arg) => !["--check", "--write", "--json"].includes(arg)) || (argv.includes("--check") && argv.includes("--write"))) throw new Error("choose --check or --write, optionally --json");
    const report = census(root);
    if (argv.includes("--json")) console.log(JSON.stringify(report, null, 2));
    else {
      console.log("module\tpublic\tdoctests\texamples\ttests\tuncovered");
      report.modules.forEach((row) => console.log([row.module, row.public, row.doctests, row.examples, row.tests, row.uncovered].join("\t")));
      console.log(`total: ${report.total.public} public functions; ${report.total.uncovered} uncovered`);
    }
    if (!argv.includes("--check") && !argv.includes("--write")) return 0;
    const path = resolve(root, RATCHET);
    const previous = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null;
    const decision = ratchetDecision(report.total.uncovered, previous);
    if (argv.includes("--write") && (previous === null || decision.passes)) {
      writeFileSync(path, `${JSON.stringify(decision.next, null, 2)}\n`);
      console.error(`Core coverage ratchet: ${decision.next.uncovered} uncovered`);
      return 0;
    }
    if (!decision.passes) {
      console.error(previous === null ? "Core coverage ratchet missing; initialize with --write" : `Core coverage regressed: ${report.total.uncovered} uncovered > ceiling ${previous.uncovered}; add call-site coverage (the ratchet cannot be raised with --write)`);
      return 1;
    }
    console.error(`Core coverage ratchet OK: ${report.total.uncovered} <= ${previous.uncovered} uncovered`);
    return 0;
  } catch (error) {
    console.error(`Core coverage census: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) process.exitCode = main();
