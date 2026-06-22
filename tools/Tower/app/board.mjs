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
// Legacy v1 stage names → v2. Applied on load.
const LEGACY = {
  "far-horizon": "frozen", "pre-plan": "backlog", "decisions": "deciding",
  "blocked": "deciding", "planned": "ready", "implementation": "building",
};
export const normalizeStage = (s) => LEGACY[s] || (STAGES.includes(s) ? s : "backlog");

export function loadBoard() {
  if (!existsSync(P.board)) return { scratch: "", cards: [], questions: [], ingest: [] };
  let b;
  try { b = JSON.parse(read(P.board)); } catch { return { scratch: "", cards: [], questions: [], ingest: [] }; }
  b.scratch ??= ""; b.cards ??= []; b.questions ??= []; b.ingest ??= [];
  for (const c of b.cards) {
    c.stage = normalizeStage(c.stage);
    c.type ??= "task";
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
    plan: p.plan || null,
    notes: [],
    created: now(),
    updated: now(),
  };
}
