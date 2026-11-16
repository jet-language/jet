// tower-spec-apply.mjs SPEC.json [--go]: applies a spec of Tower card edits.
// Spec: [{num, title?, bodyReplace?:[[from,to]], planReplace?:[[from,to]], body?, plan?,
//         rewrite?:{n:text}, replace?:[[n,from,to]] (n="*" = every open criterion),
//         remove?:[n], add?:[text], log:text, close?:evidence, closeKind?, addTags?:[t]}]
// Without --go: dry run printing the changes.
//
// The original of gauntlet tool entry tower-spec-apply; TowerSpecApply.jet beside
// it is its Jet twin and the program in use, and this file stays as the entry's
// reference. Cards are read with `node Tools/tower/tower.mjs` in
// $TOWER_SPEC_REPO (this checkout unless set) and written through Main's Tower
// writer, $TOWER_SPEC_WRITER ($JET_STATE_ROOT/scratch/Main/tower-dev.mjs unless
// set), which takes the card patches from $TOWER_SPEC_PATCHES
// ($JET_SCRATCH_ROOT/lanes/Main unless set). A relative root names a path in the
// caller's directory.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { stateRoot, scratchRoot } from "../roots.mjs";
const [specPath, go] = [process.argv[2], process.argv.includes("--go")];
const spec = JSON.parse(fs.readFileSync(specPath, "utf8"));
const REPO = path.resolve(process.env.TOWER_SPEC_REPO || fileURLToPath(new URL("../../..", import.meta.url)));
const WRITER = path.resolve(process.env.TOWER_SPEC_WRITER || path.join(stateRoot(), "scratch/Main/tower-dev.mjs"));
const PATCHES = path.resolve(process.env.TOWER_SPEC_PATCHES || path.join(scratchRoot(), "lanes/Main"));
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const towerRead = (...a) => execFileSync("node", ["Tools/tower/tower.mjs", ...a], { cwd: REPO, env, maxBuffer: 1 << 26 }).toString();
const towerWrite = (...a) => execFileSync("node", [WRITER, ...a, "--by", "Main"], { cwd: REPO, env, maxBuffer: 1 << 26 }).toString().trim();
const sub = (t, from, to, where) => {
  if (!t.includes(from)) throw new Error(`${where}: text not found: ${from.slice(0, 80)}`);
  return t.split(from).join(to);
};
let failed = 0;
for (const s of spec) {
  try {
    const j = JSON.parse(towerRead("card", "show", String(s.num), "--json"));
    const c = j.card || j;
    const patch = {};
    const crit = (c.criteria || []).map(k => ({ ...k }));
    const byN = n => { const k = crit.find(k => k.n === Number(n)); if (!k) throw new Error(`#${s.num} no c${n}`); return k; };
    const changes = [];
    for (const [n, text] of Object.entries(s.rewrite || {})) { const k = byN(n); if (k.status !== "open") throw new Error(`#${s.num} c${n} is ${k.status}`); changes.push(`c${n} REWRITE\n   old: ${k.text.slice(0, 400)}\n   new: ${text.slice(0, 400)}`); k.text = text; }
    for (const [n, from, to] of s.replace || []) {
      const ks = n === "*" ? crit.filter(k => k.status === "open" && k.text.includes(from)) : [byN(n)];
      if (n === "*" && !ks.length) throw new Error(`#${s.num} *: no open criterion has ${from.slice(0, 60)}`);
      for (const k of ks) { k.text = sub(k.text, from, to, `#${s.num} c${k.n}`); changes.push(`c${k.n} REPLACE '${from.slice(0, 70)}' -> '${to.slice(0, 70)}'`); }
    }
    const removed = [];
    for (const n of s.remove || []) { const k = byN(n); if (k.status !== "open") throw new Error(`#${s.num} c${n} is ${k.status}, not removable`); removed.push(k); }
    let next = Math.max(0, ...crit.map(k => k.n)) + 1;
    const kept = crit.filter(k => !removed.includes(k));
    for (const text of s.add || []) { kept.push({ n: next, text, status: "open" }); changes.push(`c${next} ADD ${text.slice(0, 300)}`); next++; }
    for (const k of removed) changes.push(`c${k.n} REMOVE ${k.text.slice(0, 160)}`);
    if (changes.length) patch.criteria = kept;
    if (s.title) { patch.title = s.title; changes.push(`TITLE ${c.title}\n   -> ${s.title}`); }
    let body = c.body || "", plan = c.plan || "";
    for (const [f, t] of s.bodyReplace || []) body = sub(body, f, t, `#${s.num} body`);
    for (const [f, t] of s.planReplace || []) plan = sub(plan, f, t, `#${s.num} plan`);
    if (s.body !== undefined) body = s.body;
    if (s.plan !== undefined) plan = s.plan;
    if (body !== (c.body || "")) { patch.body = body; changes.push(`BODY changed (${(c.body || "").length} -> ${body.length} chars)`); }
    if (plan !== (c.plan || "")) { patch.plan = plan; changes.push(`PLAN changed (${(c.plan || "").length} -> ${plan.length} chars)`); }
    if (s.addTags) patch.addTags = s.addTags;
    const removedNote = removed.length ? ` Removed as superseded: ${removed.map(k => `c${k.n} "${k.text.slice(0, 140).replace(/\s+/g, " ")}${k.text.length > 140 ? "…" : ""}"`).join("; ")}.` : "";
    const logText = (s.log || "") + removedNote;
    console.log(`\n#### #${s.num} ${c.title.slice(0, 90)} [${c.phase}]\n  ` + changes.join("\n  ") + (s.close ? `\n  CLOSE: ${s.close.slice(0, 200)}` : "") + `\n  LOG: ${logText.slice(0, 600)}`);
    if (!go) continue;
    if (Object.keys(patch).length) {
      const f = path.join(PATCHES, `obe-patch-${s.num}.json`);
      fs.writeFileSync(f, JSON.stringify(patch));
      console.log("  " + towerWrite("card", "update", String(s.num), "--file", f));
    }
    if (logText.trim()) console.log("  " + towerWrite("card", "log", String(s.num), "--text", logText.trim()));
    if (s.close) {
      const cur = JSON.parse(towerRead("card", "show", String(s.num), "--json")); const cc = cur.card || cur;
      for (const k of cc.criteria || []) if (k.status === "open") console.log("  " + towerWrite("card", "criteria", String(s.num), "--meet", String(k.n), "--evidence", s.close, ...(s.closeKind ? ["--kind", s.closeKind] : [])));
      console.log("  " + towerWrite("card", "update", String(s.num), "--phase", "done"));
    }
  } catch (e) { failed++; console.log(`!!!! #${s.num}: ${e.message.split("\n")[0]}`); }
}
console.log(failed ? `\nFAILED ${failed}` : "\nALL OK");
