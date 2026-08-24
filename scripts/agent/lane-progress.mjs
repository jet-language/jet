#!/usr/bin/env node
// Live view of what every dispatched lane is actually doing.
//
// Lanes stream their whole session to ~/.cache/jet-luna/<lane>.out. That file
// carries three useful signals: `codex` narration blocks (the worker saying
// what it is doing), tool/exec output, and the final numbered MET / NOT MET
// report. This renders them as one table so a human has visibility instead of
// guessing from a process count.
//
//   node scripts/agent/lane-progress.mjs            # one snapshot
//   node scripts/agent/lane-progress.mjs --watch    # refresh until all idle
//   node scripts/agent/lane-progress.mjs --lane X   # full narration for one lane
import fs from "node:fs";
import os from "node:os";
import { execFileSync } from "node:child_process";

const DIR = `${os.homedir()}/.cache/jet-luna`;
const args = process.argv.slice(2);
const watch = args.includes("--watch");
const only = args.includes("--lane") ? args[args.indexOf("--lane") + 1] : null;

const live = () => {
  try {
    return execFileSync("pgrep", ["-af", "codex exec"], { encoding: "utf8" })
      .split("\n").filter(Boolean);
  } catch { return []; }
};

function laneState(lane) {
  const path = `${DIR}/${lane}.out`;
  let st; try { st = fs.statSync(path); } catch { return null; }
  const text = fs.readFileSync(path, "utf8");
  const lines = text.split("\n");

  // The worker's own narration: the most honest "what am I doing" signal.
  const narration = [];
  for (let i = 0; i < lines.length; i++) {
    if (lines[i] !== "codex") continue;
    const said = lines.slice(i + 1, i + 12).find((l) => l.trim());
    if (said) narration.push(said.trim());
  }

  // Final report, when it has one.
  const tokIdx = text.lastIndexOf("tokens used");
  const reported = tokIdx >= 0;
  const met = [...text.matchAll(/^\s*(\d+)\.\s*\**(NOT MET|MET)\**/gm)];

  return {
    lane,
    bytes: st.size,
    ageMin: Math.round((Date.now() - st.mtimeMs) / 60000),
    steps: narration.length,
    doing: narration.length ? narration[narration.length - 1] : "(starting)",
    errors: (text.match(/^.*ERROR codex_core/gm) || []).length,
    reported,
    met: met.filter((m) => m[2] === "MET").length,
    notMet: met.filter((m) => m[2] === "NOT MET").length,
  };
}

function render() {
  const running = live();
  const lanes = fs.readdirSync(DIR).filter((f) => f.endsWith(".out")).map((f) => f.slice(0, -4));
  const rows = [];
  for (const lane of lanes) {
    if (only && lane !== only) continue;
    const s = laneState(lane);
    if (!s) continue;
    s.alive = running.some((p) => p.includes(`/${lane}.md`) || p.includes(`${lane}.out`));
    // Only surface lanes that are alive or finished very recently.
    if (!s.alive && s.ageMin > 90) continue;
    rows.push(s);
  }
  rows.sort((a, b) => Number(b.alive) - Number(a.alive) || b.steps - a.steps);

  if (only) {
    const path = `${DIR}/${only}.out`;
    const lines = fs.readFileSync(path, "utf8").split("\n");
    console.log(`=== ${only} narration ===`);
    for (let i = 0; i < lines.length; i++) {
      if (lines[i] !== "codex") continue;
      const said = lines.slice(i + 1, i + 12).filter((l) => l.trim()).slice(0, 3).join(" ");
      if (said) console.log("  • " + said.trim().slice(0, 200));
    }
    return rows.length;
  }

  const alive = rows.filter((r) => r.alive).length;
  console.log(`\nlanes: ${alive} running, ${rows.length - alive} finished   ${new Date().toLocaleTimeString()}`);
  console.log("state  lane            steps  size    idle  err  report        doing");
  for (const r of rows) {
    const bar = "#".repeat(Math.min(12, r.steps)).padEnd(12, ".");
    const rep = r.reported ? `${r.met}met/${r.notMet}not` : "-";
    console.log(
      `${(r.alive ? "RUN " : "done").padEnd(6)} ${r.lane.padEnd(14)} ${bar} ${
        String(Math.round(r.bytes / 1024) + "K").padStart(6)} ${
        String(r.ageMin + "m").padStart(5)} ${String(r.errors).padStart(4)}  ${
        rep.padEnd(12)} ${r.doing.slice(0, 88)}`,
    );
  }
  return alive;
}

if (watch) {
  for (;;) {
    const alive = render();
    if (!alive) break;
    await new Promise((r) => setTimeout(r, 30000));
  }
} else {
  render();
}
