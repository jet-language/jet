import { fileURLToPath as jetToolPath } from 'node:url';
const JET_TOOL_REPO = process.env.JET_REPO ?? jetToolPath(new URL('../../../', import.meta.url));
// Place every not-yet-ported Rust range into its Jet target file as `//RS ` comment lines,
// so 1:1 coverage is complete and measurable; writers then translate in place.
// Usage: node place-rust.mjs [--dry]
import fs from "node:fs";
import path from "node:path";

const HOME = process.env.HOME;
const WAVE = path.join(HOME, ".cache/jet-dev/port/wave");
const REPO = JET_TOOL_REPO;
const dry = process.argv.includes("--dry");

const started = new Set(
  fs.readFileSync(path.join(WAVE, "STARTED.txt"), "utf8")
    .split("\n").filter((l) => !l.startsWith("#"))
    .flatMap((l) => l.split(/\s+/)).map((t) => t.replace(/\[.*\]$/, "")).filter((t) => /^\d{4}$/.test(t)),
);
for (const f of fs.readdirSync(path.join(WAVE, "handoffs"))) {
  const m = f.match(/(\d{4})/);
  if (m) started.add(m[1]);
}

const rows = fs.readFileSync(path.join(WAVE, "INDEX.tsv"), "utf8").split("\n").slice(1).filter(Boolean);
const placed = [];
let skippedStarted = 0, skippedExists = 0, lines = 0;
const written = new Map(); // abs target -> content (accumulate multiple ranges)

for (const row of rows) {
  const cols = row.split("\t");
  const [id, slug, wt, rustCol, , targetCol] = cols;
  if (started.has(id)) { skippedStarted++; continue; }
  const ranges = rustCol.split(";").filter(Boolean).map((r) => {
    const m = r.match(/^(.*):(\d+)-(\d+)$/);
    return { file: m[1], start: +m[2], end: +m[3] };
  });
  const targets = targetCol.split(";").filter(Boolean);
  const root = path.join(REPO, ".agent-worktrees", wt);
  ranges.forEach((r, i) => {
    const rel = targets.length >= 2 * ranges.length ? targets[2 * i] : targets[0];
    const abs = path.join(root, rel);
    if (!written.has(abs) && fs.existsSync(abs) && fs.statSync(abs).size > 0) { skippedExists++; return; }
    const src = fs.readFileSync(path.join(REPO, r.file), "utf8").split("\n").slice(r.start - 1, r.end);
    let body = written.get(abs) ?? "";
    body += `// PORT-PENDING ${id} ${slug}: ${r.file}:${r.start}-${r.end}\n`;
    body += `// Translate in place: replace the //RS lines below with the 1:1 Jet port (keep \`// Rust: <file>:<line>\` anchors).\n`;
    for (const line of src) body += `//RS ${line}\n`;
    body += `// END-PORT-PENDING ${id} ${r.file}:${r.start}-${r.end}\n\n`;
    written.set(abs, body);
    lines += src.length;
    placed.push([id, wt, rel, `${r.file}:${r.start}-${r.end}`, src.length].join("\t"));
  });
}

if (!dry) {
  for (const [abs, body] of written) {
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, body);
  }
  fs.writeFileSync(path.join(WAVE, "PLACED.tsv"), "id\tworktree\ttarget\trust\tlines\n" + placed.join("\n") + "\n");
}
console.log(JSON.stringify({ dry, files: written.size, ranges: placed.length, lines, skippedStarted, skippedExists }));
