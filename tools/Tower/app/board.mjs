// Board store — the only owner-input state Tower keeps (cards/notes/scratch/
// questions/ingest answers). References plans by slug; never copies their text.
// board.json is owner-owned: we normalize legacy stage names on load and let the
// file migrate naturally the next time a card is saved — never a bulk rewrite.
import { writeFileSync, existsSync } from "node:fs";
import { P, read, now } from "./paths.mjs";

// The pipeline, in order. Each stage is a distinct holding state:
//   frozen    — parked for later; considered, not pursued (the owner's "freeze")
//   backlog   — wanted; no plan yet
//   deciding  — blocked on an owner decision (pre-plan go/no-go OR a plan's design calls)
//   planning  — decided to proceed; Claude is building the plan
//   ready     — has a vetted plan, no open decision; queued to implement on "go"
//   building  — actively being implemented
//   done      — shipped
export const STAGES = ["frozen", "backlog", "deciding", "planning", "ready", "building", "done"];
export const STAGE_LABELS = {
  frozen: "Frozen", backlog: "Backlog", deciding: "Deciding",
  planning: "Planning", ready: "Ready", building: "Building", done: "Done",
};
export const PRIORITIES = ["P0", "P1", "P2", "P3"];
export const PRIORITY_LABELS = {
  P0: "P0 - Blocker",
  P1: "P1 - High",
  P2: "P2 - Medium",
  P3: "P3 - Later",
};
// Legacy v1 stage names → v2. Applied on load.
const LEGACY = {
  "far-horizon": "frozen", "pre-plan": "backlog", "decisions": "deciding",
  "blocked": "deciding", "planned": "ready", "implementation": "building",
};
export const normalizeStage = (s) => LEGACY[s] || (STAGES.includes(s) ? s : "backlog");
export const normalizePriority = (p) => PRIORITIES.includes(String(p || "").toUpperCase())
  ? String(p).toUpperCase()
  : "P2";
export const normalizeWorkOrder = (n) => {
  if (n === null || n === undefined || n === "") return null;
  const x = Number(n);
  return Number.isFinite(x) && x > 0 ? Math.floor(x) : null;
};

export function compareCards(a, b) {
  const ao = normalizeWorkOrder(a.workOrder), bo = normalizeWorkOrder(b.workOrder);
  if (ao !== null || bo !== null) return (ao ?? 999999) - (bo ?? 999999);
  const ap = PRIORITIES.indexOf(normalizePriority(a.priority));
  const bp = PRIORITIES.indexOf(normalizePriority(b.priority));
  if (ap !== bp) return ap - bp;
  return String(a.created || "").localeCompare(String(b.created || ""));
}

export function loadBoard() {
  if (!existsSync(P.board)) return { scratch: "", cards: [], questions: [], ingest: [] };
  let b;
  try { b = JSON.parse(read(P.board)); } catch { return { scratch: "", cards: [], questions: [], ingest: [] }; }
  b.scratch ??= ""; b.cards ??= []; b.questions ??= []; b.ingest ??= [];
  for (const c of b.cards) {
    c.stage = normalizeStage(c.stage);
    c.type ??= "task";
    c.priority = normalizePriority(c.priority);
    c.workOrder = normalizeWorkOrder(c.workOrder);
    c.notes ??= [];
  }
  return b;
}

export function saveBoard(b) { writeFileSync(P.board, JSON.stringify(b, null, 2) + "\n"); }

export function makeCard(p) {
  const id = p.id || ("c" + Date.now().toString(36) + Math.floor(Math.random() * 1e4).toString(36));
  return {
    id,
    type: ["task", "idea", "bug"].includes(p.type) ? p.type : "task",
    title: (p.title || "").trim(),
    body: (p.body || "").trim(),
    stage: STAGES.includes(p.stage) ? p.stage : normalizeStage(p.stage),
    priority: normalizePriority(p.priority),
    workOrder: normalizeWorkOrder(p.workOrder),
    plan: p.plan || null,
    notes: [],
    created: now(),
    updated: now(),
  };
}
