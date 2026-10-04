import { fileURLToPath as jetToolPath } from 'node:url';
const JET_TOOL_REPO = process.env.JET_REPO ?? jetToolPath(new URL('../../../', import.meta.url));
// List tracked Rust files (and line counts) not referenced by any INDEX packet range.
import fs from "node:fs";
import { execSync } from "node:child_process";
const REPO = JET_TOOL_REPO;
const WAVE = process.env.HOME + "/.cache/jet-dev/port/wave";
const files = execSync("git ls-files '*.rs'", { cwd: REPO, encoding: "utf8" }).split("\n").filter(Boolean);
const planned = new Set();
for (const row of fs.readFileSync(WAVE + "/INDEX.tsv", "utf8").split("\n").slice(1)) {
  const col = row.split("\t")[3];
  if (!col) continue;
  for (const r of col.split(";")) planned.add(r.replace(/:\d+-\d+$/, ""));
}
const out = [];
let total = 0;
for (const f of files) {
  if (planned.has(f)) continue;
  const n = fs.readFileSync(REPO + "/" + f, "utf8").split("\n").length;
  total += n;
  out.push(`${n}\t${f}`);
}
out.sort((a, b) => +b.split("\t")[0] - +a.split("\t")[0]);
fs.writeFileSync(WAVE + "/UNPLANNED.tsv", "lines\tfile\n" + out.join("\n") + "\n");
const byTop = {};
for (const l of out) { const [n, f] = l.split("\t"); const p = f.split("/"); const k = p[0] === "crates" ? p[0] + "/" + p[1] : p[0]; byTop[k] = (byTop[k] || 0) + +n; }
console.log(JSON.stringify({ unplannedFiles: out.length, unplannedLines: total, byTop }, null, 1));
