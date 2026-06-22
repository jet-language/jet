// State assembly: read plans/proposals/ideas, compute per-card status from the
// ballot linkage, build the "Ready for Claude" worklist, and bundle it for /api/state.
import { readdirSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { P, read } from "./paths.mjs";
import { renderMd } from "./markdown.mjs";
import { loadBoard, STAGES, STAGE_LABELS } from "./board.mjs";
import { parseBallot, answeredIds, cardDecisionLinks } from "./ballot.mjs";

function readDocList(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".md") && f.toLowerCase() !== "readme.md" && !/implementation-plan/i.test(f))
    .sort()
    .map((f) => {
      const md = read(join(dir, f));
      const title = (md.match(/^#\s+(.+)$/m) || [, f.replace(/\.md$/, "")])[1].trim();
      const status = (md.match(/^\*\*Status:\*\*\s*(.+)$/m) || [, ""])[1].trim();
      return { slug: f.replace(/\.md$/, ""), file: f, title, status };
    });
}

export const DOC_DIRS = { sidequest: P.sidequests, proposal: P.proposals, plan: P.plansDir, idea: P.ideasDir };
export function resolveDoc(kind, slug) {
  const base = DOC_DIRS[kind];
  if (!base || !slug) return null;
  const clean = String(slug).replace(/\.md$/, "");
  const file = resolve(base, clean + ".md");
  if (file !== base && !file.startsWith(base + "/")) return null; // containment
  return file;
}

function ratifiedCount() {
  const md = read(P.ratified);
  return new Set([...md.matchAll(/\b([DSU]-?[A-Z]*\d+[A-Z]*)\b/g)].map((m) => m[1])).size;
}

// Per-card computed status. tone drives the UI color; `action`/`auto` feed the
// worklist; `owner:true` means the next move is the owner's (don't queue it).
function cardStatus(card, linked, answered) {
  const open = linked.filter((id) => !answered.has(id));
  const decided = linked.filter((id) => answered.has(id));
  const s = (o) => ({ blockedBy: open, decided, ...o });
  switch (card.stage) {
    case "done": return s({ label: "Done", tone: "done" });
    case "building": return s({ label: "Building", tone: "hot", inFlight: true });
    case "frozen": return s({ label: "Frozen", tone: "frozen" });
    case "ready": return s({ label: "Ready for Claude", tone: "go", action: "implement", auto: false });
    case "planning": return card.plan
      ? s({ label: "Plan ready", tone: "go", action: "implement", auto: false })
      : s({ label: "Planning", tone: "plan", action: "build-plan", auto: true });
    case "backlog": return card.plan
      ? s({ label: "Plan ready", tone: "go", action: "implement", auto: false })
      : s({ label: "Needs plan", tone: "plan", action: "build-plan", auto: true });
    case "deciding": {
      if (open.length) return s({ label: `Blocked on ${open.length} decision${open.length > 1 ? "s" : ""}`, tone: "wait", owner: true });
      if (!linked.length) return s({ label: "Decision not drafted", tone: "wait", action: "draft-decision", auto: true });
      if (!card.plan) return s({ label: "Decided — build plan", tone: "plan", action: "build-plan", auto: true });
      return s({ label: "Decided — ready for Claude", tone: "go", action: "implement", auto: false });
    }
    default: return s({ label: card.stage, tone: "" });
  }
}

// Human label + whether the owner has to act first, per action.
const ACTION_TEXT = {
  "build-plan": "Build the implementation plan",
  "draft-decision": "Draft the ballot decision(s)",
  "implement": "Implement",
};

export function buildState() {
  const md = read(P.ballotMd);
  const answered = answeredIds();
  const links = cardDecisionLinks(md);
  const board = loadBoard();

  for (const c of board.cards) {
    const linked = links[c.id] || [];
    c.linked = linked;
    c.status = cardStatus(c, linked, answered);
  }

  // Worklist: cards with a Claude action that isn't blocked on the owner.
  const worklist = board.cards
    .filter((c) => c.status.action && !c.status.owner)
    .map((c) => ({
      id: c.id, title: c.title, type: c.type, stage: c.stage, plan: c.plan,
      action: c.status.action, auto: !!c.status.auto,
      text: ACTION_TEXT[c.status.action] || c.status.action,
      decided: c.status.decided,
    }));

  // ballot: hide already-submitted decisions/open items.
  const ballot = parseBallot(md).filter((x) => !(x.id && answered.has(x.id)));

  return {
    board,
    stages: STAGES,
    stageLabels: STAGE_LABELS,
    ballot,
    links,
    worklist,
    plans: readDocList(P.sidequests),
    proposals: readDocList(P.proposals),
    ideas: readDocList(P.ideasDir),
    ratified: ratifiedCount(),
    ingestCount: (board.ingest || []).filter((i) => i.status !== "done").length,
    lastSubmit: existsSync(P.results) ? (read(P.results).match(/_submitted (.+?)_/) || [, ""])[1] : "",
  };
}
