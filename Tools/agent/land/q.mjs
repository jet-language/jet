// node Tools/agent/land/q.mjs Lane base1 base2 ... : queue land jobs in order; each base has .patch and .msg (base without extension).
import fs from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { runRoot, scratchRoot } from "../roots.mjs";
const [lane, ...bases] = process.argv.slice(2);
// Owner order: Pareto work first. Small wins are welcome, but only as a
// by-product of a lane whose main work is an approved big-win design.
// A speed-lane .msg must carry one of:
//   `PARETO: <wall seconds> s`  measured same-run wall saving >= 0.5 s;
//   `PARETO-STAGE: <design>`    a stage of an approved >= 1.5 s design;
//   `SMALL-WIN: <wall seconds> s` a smaller win, accepted only while the lane
//                                 has an approved design in the pareto list (lane state, not repo data).
const SPEED_LANES = new Set(["O0Speed", "RustOut"]);
if (SPEED_LANES.has(lane)) {
  const approvedFile = process.env.PARETO_APPROVED || join(scratchRoot(), "lanes/Main/pareto-approved.txt");
  const approved = fs.existsSync(approvedFile) && fs.readFileSync(approvedFile, "utf8").split("\n").some(l => l.split("\t")[0] === lane);
  for (const b of bases) {
    const msg = fs.existsSync(b + ".msg") ? fs.readFileSync(b + ".msg", "utf8") : "";
    const wall = msg.match(/^PARETO:\s*-?([0-9.]+)\s*s\b/m);
    const stage = /^PARETO-STAGE:\s*\S+/m.test(msg);
    const small = /^SMALL-WIN:\s*-?[0-9.]+\s*s\b/m.test(msg);
    if (stage || (wall && Number(wall[1]) >= 0.5)) continue;
    if (small && approved) continue;
    console.log(small
      ? `NOT QUEUED: ${b} is a small win, but ${lane} has no approved Pareto design yet; send the big-win design first (owner order: Pareto work first)`
      : `NOT QUEUED: ${b}.msg needs a PARETO: >=0.5 s, PARETO-STAGE: or SMALL-WIN: line`);
    process.exit(1);
  }
}
// Refuse to queue a stack that does not apply in order on dev HEAD (temporary index; worktree untouched).
const check = execFileSync("bash", [join(import.meta.dirname, "stackcheck.sh"), ...bases.map(b => b + ".patch")], { encoding: "utf8" });
process.stdout.write(check);
if (/^FAIL/m.test(check)) { console.log("NOT QUEUED: rebase the failing patches onto HEAD"); process.exit(1); }
const queue = join(runRoot(), "land-queue");
let t = Math.floor(Date.now() / 1000);
for (const [i, b] of bases.entries()) {
  for (const ext of [".patch", ".msg"]) if (!fs.existsSync(b + ext)) throw new Error("missing " + b + ext);
  let name = `${t + i}-${lane}${bases.length > 1 ? i + 1 : ""}`;
  // Never overwrite a queued or taken job with the same second-based name.
  while (fs.existsSync(join(queue, `${name}.job`)) || fs.existsSync(join(queue, "taken", `${name}.job`))) { t += 1; name = `${t + i}-${lane}${bases.length > 1 ? i + 1 : ""}`; }
  fs.writeFileSync(join(queue, `${name}.job`), `${lane}\t${b}.msg\t${b}.patch\n`, { flag: "wx" });
  console.log("queued", name);
}
