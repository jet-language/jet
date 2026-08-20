#!/usr/bin/env node
// Generate a Luna brief from Tower card JSON.
// usage: node .claude/bdlog/mkbrief.mjs <out-name> <card-ref>... [-- extra note file]
import { execFileSync } from "node:child_process";
import { writeFileSync, readFileSync, existsSync } from "node:fs";

const argv = process.argv.slice(2);
const sep = argv.indexOf("--");
const refs = (sep === -1 ? argv : argv.slice(0, sep)).slice(1);
const outName = (sep === -1 ? argv : argv.slice(0, sep))[0];
const noteFile = sep === -1 ? null : argv[sep + 1];

function card(ref) {
  const raw = execFileSync("node", ["plugins/tower/tower.mjs", "card", "show", ref, "--json"], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  return JSON.parse(raw).card ?? JSON.parse(raw);
}

const cards = refs.map(card);
const titles = cards.map((c) => c.title).join(" + ");

const NON_NEGOTIABLES = `## Non-negotiables

- I2 rustc must never reject generated code. I3 all checking in sema, codegen dumb. I4 every diagnostic needs a registered code with what/why/fix AND a tests/ui fixture. I5 every feature ships an example with golden output. I7 new user-typeable syntax is owner-gated — if the card needs a spelling its decision does not ratify, STOP on that criterion and report a ballot-ready choice. I8 one mechanism. I9 semantics live ONLY in Prelude/CoreLib and every tier marshals to them; nothing closed AOT-only, nothing parked in tests/jit_gaps.txt.
- Greenfield: when you replace a form, delete it and migrate every in-repo consumer in the same change. No alias, no compat branch, no deprecation shim.
- The lesson this epoch keeps teaching: almost every serious defect found was ONE FACT WRITTEN TWICE that later drifted — a residency gate against its lowering, three copies of an enum-path table, three signal queues of which two were dead, five JSON escapers of which two emitted invalid JSON, an AOT matcher disagreeing with its own kernel. If a fact must appear twice, put it in one place and have the other read it.
- You type-check but you do not test. Do not claim a runtime, tier, golden, snapshot or generated-artifact criterion met; name the command that would prove it.

## Return shape

Caveman (terse, drop articles, keep exact identifiers). Give me:

1. The last line of your final \`lane-check.sh\` run, verbatim (\`CHECK OK\` or the errors, with whose files they are in).
2. Per-criterion status: met with a file:line evidence pointer, or open with the smallest decisive reason.
3. Changed files with line ranges.
4. Every consumer you migrated, and anything you deliberately left, with why.
5. The exact commands the orchestrator must run to prove each criterion, one per criterion, runnable as written.
6. Any owner-gated question as a ballot-ready choice, not an invented answer.
7. Anything you did not do, named plainly. A partial honest result beats a complete-sounding one.

Findings go in your single final message. Interrupt only if blocked, or if you find something actively dangerous: a safety diagnostic that stopped firing, a lost effect, generated code rustc rejects, or one program answering differently on two tiers.
`;

const CHECK_CONTRACT = `## Check your own work before you answer

Two rules replace the old "source only, never run anything" rule, because that
rule cost more than it saved: workers shipped patches that did not compile, and
examples the parser rejects, and the orchestrator became a serial repair queue.

You MAY run exactly these, as often as you like:

1. \`scripts/agent/lane-check.sh\` — type-checks the whole workspace. Your patch
   MUST end with \`CHECK OK\`. If it prints errors in files you did not touch,
   another worker is mid-edit in the same checkout: name those files in your
   report and do not fix them.
2. \`./target/debug/jet check <file>\` and \`./target/debug/jet fmt --check <file>\`
   — ONLY to validate a \`.jet\` file you wrote (an example, a fixture). The
   binary may be slightly stale; that is fine for syntax.

You MUST NOT run: \`cargo test\`, \`cargo build --release\`, \`cargo fmt\`, any
generator, any bless or update-expect command, any git write, any Tower write.
The orchestrator owns those.

## Never invent a spelling

Two examples last session were written in syntax that does not exist. Before you
write any \`.jet\` line, find the same construct in \`examples/features/**\` and
copy its shape. Facts that cost real time:

- A list of one type: \`[Fighter.{name: "Ada"}, .{name: "Bo"}]\`, or fully typed
  \`[Fighter].{.{…}, .{…}}\`. A map: \`[String:Int].{"a": 1}\`. A bare \`{ "k": v }\`
  is NOT a Jet expression.
- There is no binding type annotation. \`x: T :: v\` is E0003 — "types ride the
  value". Write \`x :: T.{…}\`.
- A struct reaching a decoder needs \`#Codable\`; without it you get E2411.
  \`crypto.Secret\` implements neither Encode nor Decode.
- A module-level \`#Persist name := 0\` cannot be mutated from a function (E0111).
- Durations print as whole nanoseconds: \`print("{1d}")\` gives \`86400000000000ns\`.
- Every example you add must pass \`jet check\` AND \`jet fmt --check\` before you
  report it. An example the compiler rejects breaks the golden corpus for everyone.

`;

const head = `# Brief: ${titles}

## Where you work

- Repo \`/home/nate/Projects/Github/jet\`, branch \`master\`. That checkout only. NEVER touch \`plugins/tower/**\`, \`.claude/**\`, or any sibling worktree.
- Other workers edit other cards in this same checkout right now. Stay inside the files your card needs. If your fix needs a file another card plainly owns, say so instead of editing around it.
- No commits, no board writes, no branch or worktree operations.

`;

let body = "";
for (const c of cards) {
  body += `## Card ${c.id} — ${c.title}\n\n`;
  body += "```\nnode plugins/tower/tower.mjs card show '" + c.id + "' --json\n```\n\n";
  if (c.body) body += `Body:\n\n${c.body}\n\n`;
  if (c.plan) body += `Plan (verbatim, this is the route):\n\n${c.plan}\n\n`;
  const crit = c.criteria || [];
  body += `Exit criteria — every one is a discrete deliverable:\n`;
  crit.forEach((x, i) => {
    const text = typeof x === "string" ? x : x.text;
    const state = typeof x === "string" ? "" : ` [${x.status || (x.met ? "met" : "open")}]`;
    body += `${i + 1}. ${text}${state}\n`;
  });
  body += "\n";
}

let note = "";
if (noteFile && existsSync(noteFile)) note = `## Orchestrator note\n\n${readFileSync(noteFile, "utf8")}\n\n`;

const brief = head + CHECK_CONTRACT + body + note + NON_NEGOTIABLES;
writeFileSync(`/tmp/luna/${outName}.md`, brief);
console.log(`/tmp/luna/${outName}.md`, brief.length, "bytes");
