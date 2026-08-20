#!/usr/bin/env node
// prove.mjs — run a card's named proof commands and record what they actually
// printed as criterion evidence.
//
// Why: recording evidence by hand was the slowest administrative step, and it
// invites the worst failure mode this project has — evidence prose that does
// not match a command anyone ran. This runs the command, keeps its exit code
// and first output lines, and marks a criterion met ONLY on exit 0. A failing
// command records nothing and prints why.
//
// Proof map: .claude/bdlog/proofmap.json
//   { "<cardId>": { "<criterionNumber>": { "cmd": "...", "note": "..." }, … } }
// `cmd` runs through scripts/agent/jet-env from the repo root. `note` is
// prepended to the recorded evidence so the row reads as a sentence.
//
// Usage:
//   node .claude/bdlog/prove.mjs <cardId>...        # run + record
//   node .claude/bdlog/prove.mjs --dry <cardId>...  # run, show, record nothing

import { execFileSync } from "node:child_process";
import { readFileSync, mkdirSync } from "node:fs";

const BY = "fable-e3-burndown";
const args = process.argv.slice(2);
const dry = args.includes("--dry");
const cards = args.filter((a) => a !== "--dry");
const map = JSON.parse(readFileSync(".claude/bdlog/proofmap.json", "utf8"));
const scratch = process.env.JET_TEST_SCRATCH ?? `${process.env.HOME}/.cache/jet-test-scratch`;
mkdirSync(scratch, { recursive: true });

const tower = (a) =>
  execFileSync("node", ["plugins/tower/tower.mjs", ...a], {
    encoding: "utf8",
    maxBuffer: 1e9,
  });

const run = (cmd) => {
  try {
    const out = execFileSync("sh", ["-c", `scripts/agent/jet-env ${cmd}`], {
      encoding: "utf8",
      maxBuffer: 1e9,
      env: {
        ...process.env,
        JET_NIX_TMP_CLEANED: "1",
        // /tmp is RAM-backed here and cargo defaults to one rustc per thread.
        TMPDIR: scratch,
        TMP: scratch,
        TEMP: scratch,
        CARGO_INCREMENTAL: "0",
        CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS ?? "8",
      },
      timeout: 2_400_000,
    });
    return { code: 0, out };
  } catch (e) {
    return { code: e.status ?? 1, out: `${e.stdout ?? ""}${e.stderr ?? ""}` };
  }
};

const digest = (out) => {
  const lines = out.split("\n").filter((l) => l.trim());
  const results = lines.filter((l) => /test result:/.test(l));
  if (results.length) return results.join(" · ").slice(0, 400);
  return lines.slice(-4).join(" · ").slice(0, 400);
};

for (const card of cards) {
  const plan = map[card];
  if (!plan) {
    console.log(`${card}: no proof map entry`);
    continue;
  }
  const shown = JSON.parse(tower(["card", "show", card, "--json"]));
  const meta = shown.card ?? shown;
  console.log(`\n#### ${card} #${meta.num} ${meta.title ?? ""}`);
  for (const [n, spec] of Object.entries(plan)) {
    const already = (meta.criteria ?? []).find((c) => String(c.n) === String(n));
    if (already && already.status !== "open") {
      console.log(`  c${n}: already ${already.status}`);
      continue;
    }
    process.stdout.write(`  c${n}: ${spec.cmd} … `);
    const { code, out } = run(spec.cmd);
    if (code !== 0) {
      console.log(`FAILED (exit ${code})`);
      console.log(`      ${digest(out)}`);
      continue;
    }
    const evidence = `${spec.note ? spec.note + " " : ""}PROOF \`${spec.cmd}\` exited 0 at HEAD: ${digest(out)}`;
    console.log("ok");
    if (dry) {
      console.log(`      would record: ${evidence.slice(0, 200)}…`);
      continue;
    }
    tower(["card", "criteria", card, "--meet", String(n), "--evidence", evidence, "--by", BY]);
    console.log(`      recorded`);
  }
}
