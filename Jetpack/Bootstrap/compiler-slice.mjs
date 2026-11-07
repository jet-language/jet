// The Jetpack unit references a slice of the Jet compiler's sources (the
// `Compiler hash` and `Compiler front end` sections of Jetpack's sources.list).
// That slice must stay closed under the compiler's packages: every top-level
// name the unit uses that only an unlisted member file of a slice package
// declares is drift, and that file must be listed.
// A use whose only declaration lives in a compiler package outside the slice
// is drift too: Jetpack must not reach past the front end.
//
//   node Jetpack/Bootstrap/compiler-slice.mjs          # report drift, exit 1 on any
//   import { compilerSliceDrift } from "./compiler-slice.mjs"
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { toolJet } from "../../Tools/agent/roots.mjs";

const TOOL = resolve(dirname(fileURLToPath(import.meta.url)), "../../Tools/agent/CompilerPackages.jet");

const DECLARATION = /^(?:pub\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(fn|struct|enum|trait|type)\b|^([A-Z][A-Z0-9_]*)\s*:(?!=)/gm;

function listed(text) {
  return text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line !== "" && !line.startsWith("#"));
}

// Code only: comments and the contents of string and raw string literals never
// name a dependency. A run of N backticks opens a raw string and the next run of
// exactly N closes it; a raw string's text keeps its quotes and slashes.
function code(text) {
  let out = "";
  let at = 0;
  const run = (from, mark) => { let end = from; while (text[end] === mark) end += 1; return end - from; };
  while (at < text.length) {
    if (text.startsWith("//", at)) {
      const end = text.indexOf("\n", at);
      at = end < 0 ? text.length : end;
    } else if (text[at] === "`") {
      const count = run(at, "`");
      let close = at + count;
      while (close < text.length && (text[close] !== "`" || run(close, "`") !== count)) close += text[close] === "`" ? run(close, "`") : 1;
      out += '""';
      at = Math.min(text.length, close + count);
    } else if (text.startsWith('"""', at)) {
      const end = text.indexOf('"""', at + 3);
      out += '""';
      at = end < 0 ? text.length : end + 3;
    } else if (text[at] === '"') {
      let end = at + 1;
      while (end < text.length && text[end] !== '"' && text[end] !== "\n") end += text[end] === "\\" ? 2 : 1;
      out += '""';
      at = text[end] === '"' ? end + 1 : end;
    } else {
      out += text[at];
      at += 1;
    }
  }
  return out;
}

function declarations(text) {
  return [...code(text).matchAll(DECLARATION)].map((match) => ({ name: match[1] ?? match[3], fn: match[2] === "fn" }));
}

// An enum's variants: the names that open its body's top-level entries.
function variants(source) {
  const names = [];
  for (const body of source.matchAll(/^(?:pub\s+)?[A-Za-z_][A-Za-z0-9_]*:\s*enum\b[^{\n]*\{\n([\s\S]*?)^\}/gm)) {
    let depth = 0;
    let entry = true;
    for (const token of body[1].matchAll(/[A-Za-z_][A-Za-z0-9_]*|[(){}[\],\n]/g)) {
      const text = token[0];
      if ("({[".includes(text)) depth += 1;
      else if (")}]".includes(text)) depth -= 1;
      else if (text === "," || text === "\n") entry = entry || depth === 0;
      else if (entry && depth === 0) {
        if (/^[A-Z]/.test(text)) names.push(text);
        entry = false;
      }
    }
  }
  return names;
}

// The names a unit file declares: its top-level declarations and its enums' variants.
function declared(text) {
  return [...declarations(text).map((row) => row.name), ...variants(code(text))];
}

// A function is used only when called; a same-named local binding is not a use.
function referenced(text) {
  const source = code(text);
  const names = new Set(source.match(/\b[A-Za-z_][A-Za-z0-9_]*\b/g) ?? []);
  const called = new Set([...source.matchAll(/\b([A-Za-z_][A-Za-z0-9_]*)\s*\(/g)].map((match) => match[1]));
  return { names, called };
}

export function compilerSliceDrift(root) {
  const read = (path) => readFileSync(resolve(root, path), "utf8");
  // Library members only: a package's entry (its root run.jet) is not part of a slice
  // (Tools/agent/CompilerPackages.jet sources, run by roots.mjs toolJet: `NAME<TAB>DIRECTORY<TAB>FILE`).
  const compilerList = execFileSync(toolJet(), [TOOL, "--root", root, "sources"], { maxBuffer: 1 << 30 }).toString("utf8").split("\n").filter(Boolean)
    .map((row) => row.split("\t")).filter(([, directory, path]) => path !== `${directory}/run.jet`).map(([, , path]) => path).filter((path) => path.startsWith("Compiler/"));
  const jetpackList = [...listed(read("Jetpack/Bootstrap/sources.list")), ...listed(read("Jetpack/Bootstrap/tests.list"))];
  const slice = jetpackList.filter((path) => path.startsWith("Compiler/"));
  const problems = [];
  for (const path of slice) {
    if (!compilerList.includes(path) && !path.includes("/Tests/")) problems.push(`${path} is not a member of the compiler's packages`);
  }
  const packageOf = (path) => path.split("/")[1];
  const slicePackages = new Set(slice.map(packageOf));
  // Names declared by the unit itself, and the unlisted compiler files that
  // alone declare a name.
  const unitDeclared = new Set(jetpackList.flatMap((path) => declared(read(path))));
  const owners = new Map();
  for (const path of compilerList) {
    for (const row of declarations(read(path))) owners.set(row.name, owners.has(row.name) ? null : { path, fn: row.fn });
  }
  const listedSet = new Set(slice);
  const missing = new Map();
  const queue = [...jetpackList];
  while (queue.length > 0) {
    const path = queue.shift();
    const uses = referenced(read(path));
    for (const name of uses.names) {
      if (unitDeclared.has(name)) continue;
      const declaration = owners.get(name);
      if (!declaration || listedSet.has(declaration.path) || (declaration.fn && !uses.called.has(name))) continue;
      const owner = declaration.path;
      if (!slicePackages.has(packageOf(owner))) {
        problems.push(`${path} uses ${name}, which only ${owner} declares, outside the compiler slice`);
        continue;
      }
      if (!missing.has(owner)) { missing.set(owner, new Set()); queue.push(owner); for (const n of declared(read(owner))) unitDeclared.add(n); }
      missing.get(owner).add(`${name} (used by ${path})`);
    }
  }
  const order = new Map(compilerList.map((path, index) => [path, index]));
  for (const [path, uses] of [...missing].sort(([a], [b]) => order.get(a) - order.get(b))) {
    problems.push(`${path} must be listed: ${[...uses].slice(0, 3).join(", ")}${uses.size > 3 ? `, +${uses.size - 3}` : ""}`);
  }
  return problems;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const problems = compilerSliceDrift(resolve(dirname(fileURLToPath(import.meta.url)), "../.."));
  for (const problem of problems) console.error(`compiler-slice: ${problem}`);
  if (problems.length === 0) console.log("compiler-slice: the Jetpack compiler slice is closed under the compiler's packages");
  process.exitCode = problems.length === 0 ? 0 : 1;
}
