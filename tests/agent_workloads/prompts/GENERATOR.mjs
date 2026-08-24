// #1876 experiment: generate the frozen, model-facing prompt artifact.
// The prompt is derived MECHANICALLY from the frozen manifest so both arms
// receive byte-identical task wording apart from the language and filename.
// No reference adapter source is ever shown to the candidate.
import fs from "node:fs";
import path from "node:path";

const REPO = "/home/nate/Projects/Github/jet";
const CORP = path.join(REPO, "tests/agent_workloads");
const OUT = "/home/nate/.cache/jet-luna/x1876";

const rows = fs.readFileSync(path.join(CORP, "manifest.tsv"), "utf8").trim().split("\n");
const hdr = rows[0].split("\t");
const tasks = rows.slice(1).map((l) => Object.fromEntries(l.split("\t").map((v, i) => [hdr[i], v])));
function peekFile(p, rel) {
  const sz = fs.statSync(p).size;
  let peek = "";
  if (sz < 2000) {
    const t = fs.readFileSync(p, "utf8");
    if (!/\0/.test(t)) peek = "\n" + t.split("\n").map((x) => "      | " + x).join("\n");
  }
  return rel + "  (" + sz + " bytes)" + peek;
}

function tree(dir, base = dir, depth = 0) {
  if (!fs.statSync(dir).isDirectory()) return [peekFile(dir, path.basename(dir))];
  if (depth > 3) return [];
  let out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    const p = path.join(dir, e.name);
    const rel = path.relative(base, p);
    if (e.isDirectory()) { out.push(rel + "/"); out = out.concat(tree(p, base, depth + 1)); }
    else out.push(peekFile(p, rel));
  }
  return out;
}

const ARMS = {
  jet: { file: "candidate.jet", lang: "Jet", check: "scripts/agent/jet-env jet check candidate.jet" },
  node: { file: "candidate.mjs", lang: "JavaScript (Node ESM)", check: "node --check candidate.mjs" },
};

fs.mkdirSync(OUT + "/prompts", { recursive: true });
const index = [];

for (const t of tasks) {
  const inDir = path.join(CORP, t.input);
  const expected = fs.readFileSync(path.join(CORP, t.expected), "utf8");
  const listing = fs.existsSync(inDir) ? tree(inDir).join("\n    ") : "(no input tree)";

  // Language-neutral body. Identical bytes across arms.
  const body = `# Task: ${t.task_id}

Domain: ${t.domain}
Case: ${t.case}
Required outcome: ${t.declared_outcome}

## Input

Your program is given the path to an input directory as its first argument.
Its working directory is a scratch directory you may write to freely; the
input directory itself must be left unchanged.

The input directory contains exactly this, with small files shown inline:

    ${listing}

## Required output

Write to standard output EXACTLY the following bytes, and exit with status 0.
Trailing newline matters. Do not print anything else -- no logging, no
progress, no banner.

----- BEGIN EXPECTED STDOUT -----
${expected}----- END EXPECTED STDOUT -----

## Rules

- Read the input from the directory path given as the first argument.
- Do not hardcode the expected output as a literal string. Compute it from the
  input. A submission that prints a baked-in constant is a failure even though
  the bytes match.
- Do not write into the input directory. Scratch files must go in the working
  directory and must be cleaned up before exit.
- No network access.
`;

  for (const [arm, a] of Object.entries(ARMS)) {
    const p = `${body}
## Language

Write your solution in ${a.lang}, as a single file named \`${a.file}\`.

Check that it compiles or parses with:

    ${a.check}

Fix every error the checker reports and check again. When the checker is
clean, stop and report. Do not run the program against the expected output --
you are not given a way to compare, and guessing from the output is not part
of this task.

## Report format

Your final message must be exactly these lines and nothing else:

ROUNDS: <number of edits you made after the first version; 0 if the first version checked clean>
CLEAN: <yes|no>
DIAGNOSTICS: <total count of distinct checker errors you saw across all rounds>
`;
    fs.writeFileSync(`${OUT}/prompts/${t.task_id}.${arm}.md`, p);
  }
  index.push({ task_id: t.task_id, domain: t.domain, case: t.case, input: t.input, expected: t.expected });
}

fs.writeFileSync(OUT + "/tasks.json", JSON.stringify(index, null, 1));
console.log("generated " + index.length * 2 + " prompts for " + index.length + " tasks");
// prove the two arms differ only in the language section
const a = fs.readFileSync(`${OUT}/prompts/${index[0].task_id}.jet.md`, "utf8");
const b = fs.readFileSync(`${OUT}/prompts/${index[0].task_id}.node.md`, "utf8");
const ai = a.indexOf("## Language"), bi = b.indexOf("## Language");
console.log("shared prefix identical: " + (a.slice(0, ai) === b.slice(0, bi)) + " (" + ai + " bytes)");
