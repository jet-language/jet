// A stand-in for Main's linemerge.mjs: merges ours (argv 2), base (argv 3) and
// theirs (argv 4) line by line where every conflict block has one length on
// all three sides and each line changed on at most one side (or the same way
// on both); prints the merge, else exits 1.
import { spawnSync } from "node:child_process";
const [ours, base, theirs] = process.argv.slice(2);
const merged = spawnSync("git", ["merge-file", "--diff3", "-p", ours, base, theirs], { encoding: "utf8" }).stdout;
const out = [];
let part = null;
const block = { ours: [], base: [], theirs: [] };
for (const line of merged.split("\n").slice(0, -1)) {
  if (line.startsWith("<<<<<<< ")) { part = "ours"; continue; }
  if (part && line.startsWith("||||||| ")) { part = "base"; continue; }
  if (part && line === "=======") { part = "theirs"; continue; }
  if (part && line.startsWith(">>>>>>> ")) {
    const { ours: o, base: b, theirs: t } = block;
    if (o.length !== b.length || t.length !== b.length) process.exit(1);
    for (let i = 0; i < b.length; i++) {
      if (o[i] === b[i]) out.push(t[i]);
      else if (t[i] === b[i] || t[i] === o[i]) out.push(o[i]);
      else process.exit(1);
    }
    block.ours = []; block.base = []; block.theirs = [];
    part = null;
    continue;
  }
  if (part) block[part].push(line); else out.push(line);
}
process.stdout.write(out.map((line) => line + "\n").join(""));
