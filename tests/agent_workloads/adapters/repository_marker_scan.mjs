import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";

if (process.argv.length !== 3) {
  throw new Error("usage: repository_marker_scan INPUT_ROOT");
}

const root = process.argv[2];
const files = [];
function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path);
    else if (entry.isFile()) files.push(path);
  }
}

walk(root);
const rows = [];
for (const file of files) {
  const count = readFileSync(file, "utf8")
    .split(/\r?\n/)
    .filter((line) => line.includes("agent_workload:")).length;
  if (count) {
    rows.push(`/${relative(root, file).split(sep).join("/")}|${count}`);
  }
}
console.log(rows.sort().join("\n"));
