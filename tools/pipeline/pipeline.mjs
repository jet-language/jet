#!/usr/bin/env node
// jet pipeline — a tiny devops view over the task pipeline:
//   inbox  ->  plan  ->  ballot  ->  ratified  ->  implemented
// No dependencies; pure node + the markdown the team already keeps.
//
// Usage:
//   node tools/pipeline/pipeline.mjs            # status (default)
//   node tools/pipeline/pipeline.mjs status
//   node tools/pipeline/pipeline.mjs new <slug> "Title"   # scaffold a sidequest plan
//
// It reads only the canonical docs and never writes outside docs/plans/sidequests/.

import { readFileSync, writeFileSync, readdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const P = {
  inbox: join(ROOT, "docs/plans/owner-todo.md"),
  sidequests: join(ROOT, "docs/plans/sidequests"),
  ballotMd: join(ROOT, "docs/spec/decision-ballots.md"),
  ratified: join(ROOT, "docs/spec/syntax-decisions.md"),
};

const read = (p) => (existsSync(p) ? readFileSync(p, "utf8") : "");
const C = { dim: "\x1b[2m", b: "\x1b[1m", grn: "\x1b[32m", yel: "\x1b[33m", cyn: "\x1b[36m", rst: "\x1b[0m" };

// ---- readers ---------------------------------------------------------------

// Inbox: the actionable "## Next Tasks" bullets, plus a count of "## Considerations".
function readInbox() {
  const md = read(P.inbox);
  const section = (name) => {
    const m = md.match(new RegExp(`^##\\s+${name}\\s*$([\\s\\S]*?)(?=^##\\s|\\Z)`, "m"));
    return m ? m[1] : "";
  };
  const bullets = (txt) =>
    txt.split("\n").filter((l) => /^\s*-\s+\S/.test(l)).map((l) => l.replace(/^\s*-\s+/, "").trim());
  return {
    nextTasks: bullets(section("Next Tasks")),
    considerations: bullets(section("Considerations")).length,
  };
}

// Plans: every sidequest md (the slug + first heading title).
function readPlans() {
  if (!existsSync(P.sidequests)) return [];
  return readdirSync(P.sidequests)
    .filter((f) => f.endsWith(".md") && f.toLowerCase() !== "readme.md" && !/implementation-plan/i.test(f))
    .sort()
    .map((f) => {
      const md = read(join(P.sidequests, f));
      const title = (md.match(/^#\s+(.+)$/m) || [, f.replace(/\.md$/, "")])[1].trim();
      return { slug: f.replace(/\.md$/, ""), title };
    });
}

// Ballot: open decisions are `### <ID> — <title>` rows in the md.
function readBallot() {
  const md = read(P.ballotMd);
  const open = [...md.matchAll(/^###\s+([A-Z0-9-]+)\s+—\s+(.+?)\s*(?:\(([^)]*)\))?\s*$/gm)].map((m) => ({
    id: m[1],
    title: m[2].trim(),
    rec: (m[3] || "").trim(),
  }));
  return open;
}

function ratifiedCount() {
  const md = read(P.ratified);
  // Count decision-id headings/rows in the ratified log (rough, stable enough for a dashboard).
  return new Set([...md.matchAll(/\b([DSU]-?[A-Z]*\d+[A-Z]*)\b/g)].map((m) => m[1])).size;
}

// ---- commands --------------------------------------------------------------

function status() {
  const inbox = readInbox();
  const plans = readPlans();
  const ballot = readBallot();

  const line = "─".repeat(64);
  out(`${C.b}Jet task pipeline${C.rst}  ${C.dim}inbox → plan → ballot → ratified → implemented${C.rst}`);
  out(line);

  out(`${C.cyn}INBOX${C.rst}  ${P.inbox.replace(ROOT + "/", "")}`);
  if (inbox.nextTasks.length === 0) out(`  ${C.dim}(no Next Tasks)${C.rst}`);
  inbox.nextTasks.forEach((t) => out(`  • ${truncate(t, 72)}`));
  out(`  ${C.dim}+ ${inbox.considerations} considerations parked${C.rst}`);
  out("");

  out(`${C.cyn}PLANS${C.rst}  ${plans.length} sidequest${plans.length === 1 ? "" : "s"}`);
  plans.forEach((p) => out(`  • ${C.b}${p.slug}${C.rst} ${C.dim}— ${truncate(p.title, 56)}${C.rst}`));
  out("");

  out(`${C.cyn}BALLOT${C.rst}  ${ballot.length} open decision${ballot.length === 1 ? "" : "s"} awaiting owner`);
  ballot.forEach((d) => {
    const rec = d.rec ? `  ${/no rec/i.test(d.rec) ? C.yel + "NO REC" : C.grn + d.rec.toUpperCase()}${C.rst}` : "";
    out(`  • ${C.b}${d.id}${C.rst} ${truncate(d.title, 50)}${rec}`);
  });
  out("");

  out(`${C.cyn}RATIFIED${C.rst}  ~${ratifiedCount()} decisions logged in syntax-decisions.md`);
  out(line);
  const blocked = ballot.length;
  out(
    blocked
      ? `${C.yel}▸ ${blocked} decision${blocked === 1 ? "" : "s"} need your call before their plans can ship.${C.rst}  Open docs/spec/decision-ballots.html`
      : `${C.grn}▸ No decisions pending. Plans are clear to implement.${C.rst}`,
  );
}

function scaffold(slug, title) {
  if (!slug) die("usage: pipeline new <slug> \"Title\"");
  if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) die(`bad slug "${slug}" — use kebab-case`);
  const file = join(P.sidequests, `${slug}.md`);
  if (existsSync(file)) die(`already exists: ${file.replace(ROOT + "/", "")}`);
  const t = title || slug.replace(/-/g, " ");
  const tmpl = `# ${t}

**Status:** plan, awaiting owner sign-off.

## Goal

<one paragraph: what changes for the user, and why>

## Current state

<verified findings — cite code by SYMBOL (e.g. src/sema/checker_infer.rs \`check_call\`), not line number>

## Approach

<across the pipeline: parser → sema → codegen → fmt → diagnostics → examples/tests>

## Decisions for the owner

<each needs a before/after Jet example per option + a recommendation; these
feed docs/spec/decision-ballots.md per the house rule>

## Acceptance checklist

- [ ] failing test/example written first
- [ ] spec updated (docs/spec/spec.md)
- [ ] all tests green, zero unintended snapshot reblessing
- [ ] docs touched match behavior
`;
  writeFileSync(file, tmpl);
  out(`${C.grn}created${C.rst} ${file.replace(ROOT + "/", "")}`);
  out(`${C.dim}next: fill it in, then surface its decisions into docs/spec/decision-ballots.{md,html}${C.rst}`);
}

// ---- util ------------------------------------------------------------------
const out = (s = "") => process.stdout.write(s + "\n");
const die = (s) => { process.stderr.write(s + "\n"); process.exit(1); };
const truncate = (s, n) => (s.length > n ? s.slice(0, n - 1) + "…" : s);

const [cmd, ...rest] = process.argv.slice(2);
switch (cmd) {
  case undefined:
  case "status": status(); break;
  case "new": scaffold(rest[0], rest.slice(1).join(" ")); break;
  default: die(`unknown command "${cmd}". commands: status | new <slug> "Title"`);
}
