// Write-backs. The boundary: Tower records and queues; it never edits code or
// ratifies. Ratifying/implementing stay Claude steps, gated on the owner's word.
import { writeFileSync, appendFileSync, existsSync } from "node:fs";
import { P, read, rel, stamp, now, newId } from "./paths.mjs";
import { loadBoard, saveBoard } from "./board.mjs";
import { parseResults } from "./ballot.mjs";

// Submit MERGES into ballot-results.md — never overwrites. Existing decisions not
// in this submission are preserved; incoming ones add or replace by id.
export function writeResults(payload) {
  const map = parseResults(read(P.results));
  for (const r of payload.results || []) {
    if (!r.id || !r.choice) continue;
    map.set(r.id, { id: r.id, title: r.title || r.id, choice: r.choice, comment: (r.comment || "").trim() });
  }
  const lines = [
    "# Owner ballot results", "", `_submitted ${stamp()}_`, "",
    'Decisions captured from Tower. Tell Claude **"go"** to ratify these',
    "into syntax-decisions.md, strip the cards, and implement the plans.", "", "## Decisions", "",
  ];
  for (const r of map.values()) {
    lines.push(`**${r.id}** — ${r.title}`);
    lines.push(`Decision: **${r.choice}**`);
    if (r.comment) lines.push(`Comment: ${r.comment}`);
    lines.push("");
  }
  writeFileSync(P.results, lines.join("\n"));
  return rel(P.results);
}

export function queueRegen(id, title) {
  if (!existsSync(P.regenQueue)) {
    writeFileSync(P.regenQueue,
      "# Example-regeneration queue\n\nClaude reviews each open item against the example criteria " +
      "(human voice, plain language, a gist, a user-story scenario with American names, real in-the-wild code, " +
      "inline cross-language comparison, subagent-reviewed tradeoffs) and improves the ballot card before the " +
      "owner re-reads it. Checked = done.\n\n");
  }
  appendFileSync(P.regenQueue, `- [ ] ${id} — ${title}  _(requested ${stamp()})_\n`);
  return rel(P.regenQueue);
}

export function recordQuestion(decisionId, text) {
  const b = loadBoard();
  const q = { id: newId(), decisionId, text, status: "open", answer: "", created: now() };
  b.questions.push(q);
  saveBoard(b);
  if (!existsSync(P.askQueue)) {
    writeFileSync(P.askQueue,
      "# Owner questions queue\n\nQuestions the owner asked from the dashboard. Claude answers each — either " +
      "replying in board.json (shown back on the card) or updating the ballot card itself — then marks it done.\n\n");
  }
  appendFileSync(P.askQueue, `- [ ] **${decisionId}**: ${text}  _(asked ${stamp()}, id ${q.id})_\n`);
  return q;
}

// Ingest: the owner hands Claude a file path or pasted text; Claude digests it
// into candidate idea/feature/syntax cards (frozen by default) for triage.
export function queueIngest({ source, note, kind }) {
  const b = loadBoard();
  const item = { id: newId(), source: (source || "").trim(), note: (note || "").trim(),
    kind: kind || "auto", status: "open", created: now() };
  b.ingest.push(item);
  saveBoard(b);
  if (!existsSync(P.ingestQueue)) {
    writeFileSync(P.ingestQueue,
      "# Ingest queue\n\nFiles/text the owner handed Tower for Claude to digest. For each open item Claude reads " +
      "the source, extracts candidate ideas / features / syntax, files them as **frozen** cards (and drafts ballot " +
      "decisions where a real choice exists), then marks the item done. Checked = done.\n\n");
  }
  const where = item.source ? `\`${item.source}\`` : "(pasted text — see board.json ingest)";
  appendFileSync(P.ingestQueue, `- [ ] ${where}${item.note ? " — " + item.note : ""}  _(filed ${stamp()}, id ${item.id})_\n`);
  return item;
}
