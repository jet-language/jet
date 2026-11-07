// gsel.mjs TREE [EXCLUDE...] : the run-feature-examples.mjs filter arguments that select every feature example of
// TREE except the EXCLUDE stems (e.g. io/os_process_control), one per line. The runner keeps an example when its
// shown path contains a filter, so each kept example is named by its full shown path; refuses when a kept path is
// a substring of an excluded one (the exclusion could not hold).
import { execFileSync } from "node:child_process";
import { join, resolve } from "node:path";

const [treeArg, ...exclude] = process.argv.slice(2);
const tree = resolve(treeArg);
const listed = execFileSync(process.execPath, [join(tree, "Tools/agent/run-feature-examples.mjs"), "--list"], { cwd: tree, encoding: "utf8", maxBuffer: 1 << 26 })
  .split("\n").filter((l) => l && !/^\d+ examples$/.test(l));
const isExcluded = (shown) => exclude.some((stem) => shown === `Examples/${stem}.jet` || shown === `Examples/${stem}` || shown.startsWith(`Examples/${stem}/`));
const dropped = listed.filter(isExcluded);
const kept = listed.filter((s) => !isExcluded(s));
if (dropped.length !== exclude.length) throw new Error(`exclusions ${exclude.join(" ")} matched ${dropped.length} examples: ${dropped.join(" ")}`);
const clash = kept.filter((k) => dropped.some((d) => d.includes(k)));
if (clash.length) throw new Error(`kept paths that would re-select an excluded example: ${clash.join(" ")}`);
console.log(kept.join("\n"));
