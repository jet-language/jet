// Mark queue packets deferred (owner 03:50: CoreLib/Prelude to be discussed; JIT retiring per D-EXEC1).
import fs from "node:fs";
const WAVE = process.env.HOME + "/.cache/jet-dev/port/wave";
const placed = fs.readFileSync(WAVE + "/PLACED.tsv", "utf8").split("\n").slice(1).filter(Boolean);
const claims = new Map();
for (const l of fs.readFileSync(WAVE + "/CLAIMS.tsv", "utf8").split("\n").filter(Boolean)) { const [id, , st] = l.split("\t"); claims.set(id, st); }
const re = /^(crates\/jet-codegen\/src\/Prelude\/|crates\/jet-jit\/)/;
const ids = new Set();
for (const l of placed) { const [id, , , rust] = l.split("\t"); if (re.test(rust)) ids.add(id); }
let n = 0; const t = new Date().toTimeString().slice(0, 8);
for (const id of ids) {
  const st = claims.get(id);
  if (st === "done" || st === "claimed") continue;
  fs.appendFileSync(WAVE + "/CLAIMS.tsv", `${id}\tMain\tdeferred\t${t}\towner 03:50: CoreLib/Prelude discussion pending; jet-jit retiring (D-EXEC1)\n`);
  n++;
}
console.log(JSON.stringify({ matchedPackets: ids.size, deferredNow: n }));
