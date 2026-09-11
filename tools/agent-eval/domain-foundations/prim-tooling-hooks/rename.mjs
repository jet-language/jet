import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";

const root = dirname(new URL(import.meta.url).pathname);
const srcPath = resolve(root, "pkg/input.jet");
const raw = readFileSync(srcPath);
const cmd = resolve("/home/nate/Projects/Github/jet/scripts/agent/jet-env");
const proc = spawnSync(cmd, ["jet", "inspect", "compiler", "lex", srcPath], {
  cwd: "/home/nate/Projects/Github/jet",
  encoding: "utf8",
});
if (proc.status !== 0) throw new Error(proc.stderr);
const facts = JSON.parse(proc.stdout);
const spans = facts.compiler.value.tokens
  .filter((token) => token.kind === "identifier" && token.text === "old_name")
  .map((token) => token.span);
let out = Buffer.from(raw);
for (const span of spans.slice().reverse()) {
  out = Buffer.concat([out.subarray(0, span.start), Buffer.from("new_name"), out.subarray(span.end)]);
}
const outPath = resolve(root, "pkg/renamed.jet");
writeFileSync(outPath, out);
if ((out.toString().match(/new_name/g) || []).length !== 2) throw new Error("rename count");
if (!out.includes("old_name text") || !out.includes('"old_name literal"')) throw new Error("trivia changed");
console.log(`renamed identifiers=${spans.length}; preserved comment/literal=true; output=${outPath}`);
