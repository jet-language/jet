// node treeonly.mjs TREE WORKTREE : files in the composed integ TREE (minus build output) that git does not track in WORKTREE.
import { readdirSync, lstatSync } from "node:fs";
import { execFileSync } from "node:child_process";
const [tree, wt] = process.argv.slice(2);
const tracked = new Set(execFileSync("git", ["-C", wt, "-c", "core.quotepath=off", "ls-files", "-z"], { maxBuffer: 1 << 30 }).toString().split("\0"));
const skip = new Set([".jet", ".git", "target", "node_modules", ".chain"]);
const out = [];
const walk = (rel) => {
  for (const e of readdirSync(tree + "/" + rel, { withFileTypes: true })) {
    const p = rel ? rel + "/" + e.name : e.name;
    if (e.isDirectory()) { if (!skip.has(e.name)) walk(p); }
    else if (!tracked.has(p)) out.push(p);
  }
};
walk("");
console.log(out.slice(0, 40).join("\n"));
console.log(out.length + " only-in-tree");
