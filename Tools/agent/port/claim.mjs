import { fileURLToPath as jetToolPath } from 'node:url';
const JET_TOOL_REPO = process.env.JET_REPO ?? jetToolPath(new URL('../../../', import.meta.url));
// Port queue for in-place translation of placed Rust (//RS lines).
//   node claim.mjs claim <agent> [worktree]   -> prints one packet: id, worktree, targets, rust ranges, queue file
//   node claim.mjs done <agent> <id> <note...>
//   node claim.mjs release <agent> <id>         (give a packet back unfinished)
//   node claim.mjs status                       (counts + remaining //RS lines)
import fs from "node:fs";
import path from "node:path";

const WAVE = path.join(process.env.HOME, ".cache/jet-dev/port/wave");
const REPO = JET_TOOL_REPO;
const CLAIMS = path.join(WAVE, "CLAIMS.tsv");
const LOCK = path.join(WAVE, "CLAIMS.lock");
const [cmd, agent, arg, ...rest] = process.argv.slice(2);

function locked(fn) {
  const until = Date.now() + 20000;
  for (;;) {
    try { fs.closeSync(fs.openSync(LOCK, "wx")); break; } catch {
      if (Date.now() > until) { try { fs.unlinkSync(LOCK); } catch {} }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
    }
  }
  try { return fn(); } finally { try { fs.unlinkSync(LOCK); } catch {} }
}

const placed = fs.readFileSync(path.join(WAVE, "PLACED.tsv"), "utf8").split("\n").slice(1).filter(Boolean)
  .map((l) => { const [id, wt, target, rust, lines] = l.split("\t"); return { id, wt, target, rust, lines: +lines }; });
const packets = new Map();
for (const p of placed) {
  if (!packets.has(p.id)) packets.set(p.id, { id: p.id, wt: p.wt, targets: new Set(), rust: [], lines: 0 });
  const e = packets.get(p.id); e.targets.add(p.target); e.rust.push(p.rust); e.lines += p.lines;
}
function readClaims() {
  const state = new Map();
  if (!fs.existsSync(CLAIMS)) return state;
  for (const l of fs.readFileSync(CLAIMS, "utf8").split("\n").filter(Boolean)) {
    const [id, who, status] = l.split("\t");
    state.set(id, { who, status });
  }
  return state;
}
const append = (cols) => fs.appendFileSync(CLAIMS, cols.join("\t") + "\n");
const now = () => new Date().toTimeString().slice(0, 8);

function remainingRS(p) {
  let n = 0;
  for (const t of p.targets) {
    const abs = path.join(REPO, ".agent-worktrees", p.wt, t);
    if (!fs.existsSync(abs)) continue;
    for (const line of fs.readFileSync(abs, "utf8").split("\n")) if (line.startsWith("//RS")) n++;
  }
  return n;
}

if (cmd === "claim") {
  locked(() => {
    const st = readClaims();
    const prioFile = path.join(WAVE, "PRIORITY.txt");
    const prio = fs.existsSync(prioFile) ? fs.readFileSync(prioFile, "utf8").split(/\s+/).filter((t) => packets.has(t)) : [];
    for (const p of [...prio.map((id) => packets.get(id)), ...packets.values()]) {
      if (arg && p.wt !== arg && !prio.includes(p.id)) continue;
      const s = st.get(p.id);
      if (s && (s.status === "claimed" || s.status === "done" || s.status === "deferred")) continue;
      append([p.id, agent, "claimed", now()]);
      const queue = fs.readdirSync(path.join(WAVE, "queue")).find((f) => f.startsWith(p.id + "-"));
      console.log(JSON.stringify({ id: p.id, worktree: `${REPO}/.agent-worktrees/${p.wt}`, targets: [...p.targets],
        rust: p.rust, rustLines: p.lines, packet: queue ? path.join(WAVE, "queue", queue) : null }, null, 1));
      return;
    }
    console.log(JSON.stringify({ id: null, message: "queue empty" }));
  });
} else if (cmd === "done" || cmd === "release") {
  locked(() => append([arg, agent, cmd === "done" ? "done" : "released", now(), rest.join(" ")]));
  console.log("ok");
} else if (cmd === "status") {
  const st = readClaims();
  let done = 0, claimed = 0, open = 0, rsLeft = 0, total = 0;
  for (const p of packets.values()) {
    total += p.lines; rsLeft += remainingRS(p);
    const s = st.get(p.id)?.status;
    if (s === "done") done++; else if (s === "claimed") claimed++; else open++;
  }
  console.log(JSON.stringify({ packets: packets.size, done, claimed, open, placedLines: total, rsLinesLeft: rsLeft,
    translatedPct: +(100 * (1 - rsLeft / total)).toFixed(2) }));
} else {
  console.log("usage: claim|done|release|status");
}
