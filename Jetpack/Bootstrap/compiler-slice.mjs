// The Jetpack unit references a slice of the Jet compiler's sources (the
// `Compiler hash` and `Compiler front end` sections of sources.list). That
// slice must stay closed under the compiler's own manifest: every top-level
// name the unit uses that only an unlisted Compiler/Bootstrap/sources.list
// file of a slice package declares is drift, and that file must be listed.
// A use whose only declaration lives in a compiler package outside the slice
// is drift too: Jetpack must not reach past the front end.
//
//   node Jetpack/Bootstrap/compiler-slice.mjs          # report drift, exit 1 on any
//   import { compilerSliceDrift } from "./compiler-slice.mjs"
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DECLARATION = /^(?:pub\s+)?(fn|struct|enum|trait|type)\s+([A-Za-z_][A-Za-z0-9_]*)|^([A-Z][A-Z0-9_]*)\s*::/gm;

function listed(text) {
  return text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line !== "" && !line.startsWith("#"));
}

// Code only: comments and string contents never name a dependency.
function code(text) {
  return text.replace(/\/\/[^\n]*/g, "").replace(/"""[\s\S]*?"""/g, '""').replace(/"(?:[^"\\\n]|\\.)*"/g, '""');
}

function declarations(text) {
  return [...code(text).matchAll(DECLARATION)].map((match) => ({ name: match[2] ?? match[3], fn: match[1] === "fn" }));
}

function declared(text) {
  return declarations(text).map((row) => row.name);
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
  const compilerList = listed(read("Compiler/Bootstrap/sources.list")).filter((path) => path.startsWith("Compiler/"));
  const jetpackList = [...listed(read("Jetpack/Bootstrap/sources.list")), ...listed(read("Jetpack/Bootstrap/tests.list"))];
  const slice = jetpackList.filter((path) => path.startsWith("Compiler/"));
  const problems = [];
  for (const path of slice) {
    if (!compilerList.includes(path) && !path.includes("/Tests/")) problems.push(`${path} is not in Compiler/Bootstrap/sources.list`);
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
  if (problems.length === 0) console.log("compiler-slice: the Jetpack compiler slice is closed under Compiler/Bootstrap/sources.list");
  process.exitCode = problems.length === 0 ? 0 : 1;
}
