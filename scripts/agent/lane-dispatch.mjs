#!/usr/bin/env node
// lane-dispatch.mjs — the high-throughput burndown loop, as a tool rather than a habit.
//
// This encodes the cadence that moved epoch 3 from 752 to 793 closed cards in a
// single session, after several days at a far lower rate. Every rule below is
// here because its absence cost real time, and the cost is named.
//
//   brief   <card…|--auto N>   write a lane brief per card, straight from Tower
//   launch  <lane…>            start lanes DETACHED, staggered, under the cap
//   status                     what is running, what finished, memory headroom
//   harvest                    print the final message of each unread lane
//
// The five rules that made the difference:
//
//  1. PARALLELISM IS THE WHOLE GAME. Run 25-30 lanes, not 5. Cards are mostly
//     independent; the tree is shared, so there is no merge to do afterwards.
//     Serial lanes were the single largest source of elapsed time.
//
//  2. LAUNCH DETACHED. `(sh run.sh x &)` inside a tool call loses most of the
//     batch: the parent shell exits and takes the not-yet-established children
//     with it. Measured once: 9 of 27 lanes survived, and the other 18 were
//     silently absent for twenty minutes. Always `setsid nohup … < /dev/null`.
//
//  3. NEVER HAND-WRITE A BRIEF. Generate it from the card. The card already has
//     the title, body, plan and exit criteria; retyping them is slow and drifts
//     from the board. Hand-write only for a defect with no card yet.
//
//  4. WORKERS TYPE-CHECK, THEY DO NOT TEST. `scripts/agent/lane-check.sh` and
//     nothing heavier. Before this rule the orchestrator became a serial repair
//     queue — nine build breaks in one session from source-only patches. After
//     it, breaks are rare and small. Tests are batched (rule 5).
//
//  5. CLOSE ON IMPLEMENTATION; BATCH THE PROOF. A card closes when its criteria
//     have concrete implementation evidence and the patch is integrated. Every
//     deferred test run, every found defect, every owner gate goes into ONE
//     sweep ledger, resolved after the cards are closed: targeted tests at a
//     milestone boundary, the full suite once at epoch end.
//
// Two things that are easy to miss and cost hours:
//
//   * STALE BLOCKERS. Check `blockedBy` against actual phase. On this board 21
//     of 31 "blocked" cards had blockers that were already closed.
//   * BUILD BREAKS ARE YOURS. Lanes share one tree, so a break blocks every
//     lane's self-check. Fix it immediately; do not wait for the lane that
//     caused it.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const REPO = "/home/nate/Projects/Github/jet";
const HOME = process.env.HOME;
const DIR = `${HOME}/.cache/jet-luna`;
const CAP = Number(process.env.LANE_CAP ?? 30); // concurrent lanes; the harness allows 32
const MIN_FREE_GB = 12;    // refuse to launch under this
const BY = process.env.LANE_BY ?? "fable-e3-burndown";
const EPOCH = process.env.LANE_EPOCH ?? "e3";

const tower = (args) =>
  execFileSync("node", ["plugins/tower/tower.mjs", ...args], { cwd: REPO, encoding: "utf8", maxBuffer: 1 << 28 });

const cards = () => {
  // LANE_EPOCH=all lists the whole board — required for sidequest cards,
  // which carry no epoch and are invisible to an --epoch filter.
  const filter = EPOCH === "all" ? [] : ["--epoch", EPOCH];
  const raw = JSON.parse(tower(["card", "list", ...filter, "--json"]));
  return Array.isArray(raw) ? raw : raw.cards;
};

const freeGb = () =>
  Number(execFileSync("free", ["-g"], { encoding: "utf8" }).split("\n")[1].trim().split(/\s+/)[6]);

const alive = (name) => {
  // Liveness is a PROCESS question, not a log question. A lane that hit its
  // timeout and a lane still thinking both leave a log with no completion
  // marker, so reading logs alone reports ghosts. Measured once: 27 logs looked
  // busy while 10 processes existed, and two thirds of the cap sat idle behind
  // that misreading. run.sh writes a pidfile and clears it on exit.
  const pf = join(DIR, `${name}.pid`);
  if (!existsSync(pf)) return false;
  const pid = Number(readFileSync(pf, "utf8").trim());
  if (!pid) return false;
  try { process.kill(pid, 0); return true; } catch { return false; }
};

const lanes = () => {
  if (!existsSync(DIR)) return [];
  return readdirSync(DIR)
    .filter((f) => f.endsWith(".out"))
    .map((f) => {
      const name = f.replace(/\.out$/, "");
      const st = statSync(join(DIR, f));
      const body = readFileSync(join(DIR, f), "utf8");
      const running = alive(name);
      return {
        name,
        ageMin: Math.round((Date.now() - st.mtimeMs) / 60000),
        running,
        done: !running,                       // not alive => finished or dead
        yielded: /tokens used/.test(body),    // finished cleanly with a report
        kb: Math.round(st.size / 1024),
      };
    });
};

// ---------------------------------------------------------------- brief

const CONTRACT = `
## Rules

- Repo \`${REPO}\`, branch \`master\`, that checkout only. NEVER touch \`plugins/tower/**\`, \`.claude/**\`, or a sibling worktree.
- No commits, no board writes, no branch or worktree operations. Many other workers share this tree right now.
- \`scripts/agent/lane-check.sh\` must end with \`CHECK OK\`. Quote that line verbatim as the FIRST line of your report. If a file you did not touch fails, name it and continue — someone else owns it.
- You MAY run \`./target/debug/jet check|run|fmt --check\` on files you touched. Do NOT run \`cargo test\`, a generator, a bless command, \`cargo fmt\`, or the full suite. Verification is batched at the end of the epoch, not per card.
- Build a complete vertical slice. NO stubs, placeholders, mocks, no-ops, \`TODO: implement\`, or "foundation for later". If you cannot finish a criterion, leave it untouched and say so.

## Invariants

- **I1** generated Rust carries \`unsafe\` only inside a user-written \`#Unsafe("reason")\` region or a vetted std/mem internal.
- **I2** rustc never rejects generated code; if it would, that is an internal compiler error, not a user diagnostic.
- **I3** every check lives in sema, never "emit it and see whether rustc complains".
- **I4** a diagnostic needs a registered code, what/why/fix text, and a UI fixture.
- **I5** a feature ships with an example and golden-tested output.
- **I7** new user-typeable syntax is owner-gated. If your card needs it and \`docs/spec/syntax-decisions.md\` does not ratify it, STOP that slice and report it.
- **I8** one mechanism. Extend the existing path; never add a second beside it.
- **I9** semantics live in \`crates/jet-codegen/src/Prelude/**\`. AOT emit, Cranelift hosts and interpreter ambient are marshalling adapters that call the same Prelude symbol. Re-encoding policy, defaults or error text in an engine is a violation.

## Never invent a spelling

Read \`docs/spec/syntax-decisions.md\` when unsure — it carries every ratified decision, including a generated index of all of them. Never guess a spelling from another language.

## Return shape

1. \`CHECK OK\`, verbatim.
2. One line per criterion: number, done or open, \`file:line\`, and the command output that proves it.
3. Changed files with line ranges.
4. Anything unfinished, and why. An honest partial beats a claimed pass.

Findings in your single final message. Interrupt only if blocked.
`;

function writeBrief(card) {
  const cr = card.criteria ?? [];
  if (!cr.length) return null;
  const big =
    cr.length >= 8
      ? `\n## This card is large (${cr.length} criteria)\n\nDo NOT try to finish all of it. Pick the smallest coherent vertical slice that makes ONE criterion genuinely true end to end, build it properly through every layer that criterion names, and leave the rest untouched. Report exactly which criteria you reached. A real slice beats a broad half-implementation, and a stub fails this brief outright.\n`
      : "";
  const text = `# Brief: card #${card.num} — ${card.title}

${(card.body ?? "(no body)").trim()}

## Exit criteria (the definition of done)

${cr.map((x) => `${x.n}. [${x.status}] ${x.text}`).join("\n")}
${big}${card.plan ? `\n## Plan recorded on the card\n\n${card.plan}\n` : ""}
${CONTRACT}`;
  mkdirSync(DIR, { recursive: true });
  const name = `c${card.num}`;
  writeFileSync(join(DIR, `${name}.md`), text);
  return name;
}

function cmdBrief(args) {
  const all = cards();
  const autoIdx = args.indexOf("--auto");
  let picked;
  if (autoIdx >= 0) {
    const n = Number(args[autoIdx + 1] ?? 10);
    const byId = new Map(all.map((c) => [c.id, c]));
    const byNum = new Map(all.map((c) => [c.num, c]));
    const openNow = (c) => c.phase !== "done" && c.phase !== "frozen";
    picked = all
      .filter((c) => {
        if (!openNow(c) || c.phase === "deciding" || undecided(c)) return false;
        const cr = c.criteria ?? [];
        if (!cr.length || !cr.every((x) => x.status === "open")) return false;
        // stale-blocker aware: a blocker that is already closed does not block
        const live = (c.blockedBy ?? c.blocked_by ?? []).filter((b) => {
          const tgt = byId.get(b) ?? byNum.get(Number(String(b).replace(/\D/g, "")));
          return tgt && tgt.phase !== "done" && tgt.phase !== "frozen";
        });
        return live.length === 0 && !existsSync(join(DIR, `c${c.num}.out`));
      })
      .sort((a, b) => a.criteria.length - b.criteria.length)
      .slice(0, n);
  } else {
    const nums = args.filter((a) => /^\d+$/.test(a)).map(Number);
    picked = all.filter((c) => nums.includes(c.num));
  }
  const made = [];
  for (const c of picked) {
    const full = JSON.parse(tower(["card", "show", c.id, "--json"]));
    const name = writeBrief(full.card ?? full);
    if (name) made.push(name);
  }
  console.log(made.length ? made.join(" ") : "(nothing to brief)");
}

// ---------------------------------------------------------------- launch

function cmdLaunch(args) {
  const secs = Number(process.env.LANE_SECS ?? 1500);
  const running = lanes().filter((l) => l.running).length;
  let room = CAP - running;
  if (room <= 0) return console.log(`at cap: ${running} running`);
  const free = freeGb();
  if (free < MIN_FREE_GB) return console.log(`refusing: only ${free}G free, floor ${MIN_FREE_GB}G`);

  const started = [];
  for (const name of args) {
    if (room <= 0) break;
    if (!existsSync(join(DIR, `${name}.md`))) { console.log(`${name}: no brief`); continue; }
    // Detached, or the batch dies with the parent shell. This is rule 2.
    execFileSync("setsid", ["nohup", "sh", `${DIR}/run.sh`, name, "max", String(secs)], {
      cwd: DIR, stdio: "ignore", detached: true,
    });
    started.push(name);
    room -= 1;
    execFileSync("sleep", ["1"]);
  }
  console.log(`launched ${started.length}: ${started.join(" ")}  (was ${running} running, ${free}G free)`);
}

// ---------------------------------------------------------------- status

function cmdStatus() {
  const ls = lanes();
  const run = ls.filter((l) => l.running);
  const yielded = ls.filter((l) => !l.running && l.yielded);
  const died = ls.filter((l) => !l.running && !l.yielded);
  const all = cards();
  const open = all.filter((c) => c.phase !== "done" && c.phase !== "frozen");
  console.log(`${EPOCH}: done=${all.length - open.length} open=${open.length}`);
  console.log(`lanes: ${run.length} running (cap ${CAP}), ${yielded.length} yielded, ${died.length} died or timed out, ${freeGb()}G free`);
  if (run.length) console.log(`  running: ${run.map((l) => l.name).join(" ")}`);
  if (died.length) console.log(`  no report (re-brief smaller): ${died.map((l) => l.name).join(" ")}`);
  const room = CAP - run.length;
  console.log(room > 0 ? `  ROOM FOR ${room} MORE — launch them` : "  at cap");
}

// ---------------------------------------------------------------- harvest

function cmdHarvest() {
  const readFile = join(DIR, "harvested.txt");
  const seen = new Set(existsSync(readFile) ? readFileSync(readFile, "utf8").split("\n").filter(Boolean) : []);
  const fresh = lanes().filter((l) => l.yielded && !seen.has(l.name));
  if (!fresh.length) return console.log("(nothing new)");
  for (const l of fresh) {
    const s = readFileSync(join(DIR, `${l.name}.out`), "utf8").replace(/\u001b\[[0-9;]*m/g, "");
    const i = s.lastIndexOf("] codex");
    let tail = i < 0 ? s.slice(-1200) : s.slice(i);
    const c = tail.search(/CHECK (OK|FAILED)/);
    if (c > 0) tail = tail.slice(c);
    console.log(`\n===== ${l.name} =====\n${tail.slice(0, 1200)}`);
    seen.add(l.name);
  }
  writeFileSync(readFile, [...seen].join("\n"));
}

function undecided(c) {
  if (/^\s*ballot\b/i.test(c.title || "")) return true;
  return (c.decisions || []).some((d) => !d.outcome && d.status !== "ratified");
}

function cmdRecycle(args) {
  const want = Number(args[0] ?? 30);
  const all = cards();
  const open = all.filter((c) => c.phase !== "done" && c.phase !== "frozen" && c.phase !== "deciding" && !undecided(c));
  const live = new Set(lanes().filter((l) => l.running).map((l) => l.name));
  const rows = [];
  for (const c of open) {
    const name = `c${c.num}`;
    if (live.has(name)) continue;
    if (!existsSync(join(DIR, `${name}.md`))) continue;
    const log = join(DIR, `${name}.out`);
    rows.push([name, existsSync(log) ? statSync(log).mtimeMs : 0]);
  }
  rows.sort((a, b) => a[1] - b[1]);
  console.log(rows.slice(0, want).map((r) => r[0]).join(" "));
}

const [, , cmd, ...rest] = process.argv;
({ brief: cmdBrief, launch: cmdLaunch, status: cmdStatus, harvest: cmdHarvest, recycle: cmdRecycle }[cmd] ??
  (() => console.log("usage: lane-dispatch.mjs brief|launch|status|harvest")))(rest);
