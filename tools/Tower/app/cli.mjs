// Console surface: `status` snapshot and `new` scaffold.
import { writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { P, rel, C, out, die, truncate } from "./paths.mjs";
import { STAGES, STAGE_LABELS } from "./board.mjs";
import { buildState } from "./state.mjs";

export function status() {
  const s = buildState();
  const dec = s.ballot.filter((x) => x.kind === "decision");
  const open = s.ballot.filter((x) => x.kind === "open");
  const cards = s.board.cards;
  const line = "─".repeat(66);
  out(`${C.b}Tower${C.rst}  ${C.dim}${STAGES.map((x) => STAGE_LABELS[x]).join(" → ")}${C.rst}`);
  out(line);
  out(`${C.cyn}BOARD${C.rst}  ${cards.length} cards (${cards.filter((c) => c.type === "bug").length} bugs)`);
  for (const st of STAGES) {
    const n = cards.filter((c) => c.stage === st);
    if (n.length) out(`  ${C.dim}${STAGE_LABELS[st].padEnd(9)}${C.rst} ${n.map((c) => c.title).slice(0, 4).join("; ")}`);
  }
  out("");
  if (s.worklist.length) {
    out(`${C.cyn}READY FOR CLAUDE${C.rst}  ${s.worklist.length} queued`);
    for (const w of s.worklist) {
      const tag = w.auto ? `${C.grn}auto${C.rst}` : `${C.yel}gated${C.rst}`;
      out(`  ${tag}  ${C.b}${w.id}${C.rst} ${w.text} — ${C.dim}${truncate(w.title, 44)}${C.rst}`);
    }
    out("");
  }
  if (s.ingestCount) out(`${C.cyn}INGEST${C.rst}  ${s.ingestCount} item(s) awaiting digest (ingest-queue.md)\n`);
  out(`${C.cyn}DECISIONS${C.rst}  ${dec.length} carded · ${open.length} open (ask to expand)`);
  dec.forEach((d) => out(`  • ${C.b}${d.id}${C.rst} ${truncate(d.title, 50)}  ${/no rec/i.test(d.rec) ? C.yel + "NO REC" : C.grn + (d.rec || "").toUpperCase()}${C.rst}`));
  out(line);
  out(`${C.grn}▸ node tools/Tower/Tower.mjs serve --open${C.rst}`);
}

export function scaffold(slug, title) {
  if (!slug) die('usage: Tower.mjs new <slug> "Title"');
  if (!/^[a-z0-9][a-z0-9-]*$/.test(slug)) die(`bad slug "${slug}" — use kebab-case`);
  const file = join(P.sidequests, `${slug}.md`);
  if (existsSync(file)) die(`already exists: ${rel(file)}`);
  const t = title || slug.replace(/-/g, " ");
  writeFileSync(file, `# ${t}

**Status:** plan, awaiting owner sign-off.

## Goal

<one paragraph: what changes for the user, and why>

## Current state

<verified findings — cite code by SYMBOL (e.g. src/sema/checker_infer.rs \`check_call\`), not line number>

## Approach

<across the pipeline: parser → sema → codegen → fmt → diagnostics → examples/tests>

## Decisions for the owner

<each decision card, in tools/Tower/docs/ballots/decision-ballots.md, carries — in this order:
**Gist:** one plain sentence (what's being chosen).
**Story.** a real person with an American-traditional name and what they're doing.
**In the wild:** a fenced \`\`\`jet block of real-ish project code where this bites.
**Other languages:** short fenced blocks showing how Rust/TS/etc. spell it (when relevant).
**Tradeoffs:** a compact table, one row per option, columns that actually differ — subagent-reviewed.
- **Option A — Name.** worked \`\`\`jet example.
- **Option B — Name (recommended).** worked \`\`\`jet example.
**Recommendation:** one line why.>

## Acceptance checklist

- [ ] failing test/example written first
- [ ] spec updated (docs/spec/spec.md)
- [ ] all tests green, zero unintended snapshot reblessing
- [ ] docs touched match behavior
`);
  out(`${C.grn}created${C.rst} ${rel(file)}`);
}
