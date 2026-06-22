// Ballot parser. Single source of truth: docs/ballots/decision-ballots.md.
// Cards are parsed out of the markdown so there is no duplicated card data.
import { renderMd } from "./markdown.mjs";
import { P, read } from "./paths.mjs";

export const DECISION_ID = /^(D-[A-Z0-9-]+|S\d+-[A-Z]+|S\d+|N\d+|U\d+)$/;

// The "## Open decisions" region: everything from that heading to EOF.
function openSection(md) {
  const m = md.match(/^## Open decisions\s*$/m);
  if (!m) return "";
  return md.slice(m.index + m[0].length);
}

// Map each board card id -> the decision ids open beneath it. Group sections are
// headed "## <name> — board card cXX"; the "### <ID> — …" cards beneath belong to
// that card. This is the link the board's status + worklist ride on.
export function cardDecisionLinks(md = read(P.ballotMd)) {
  const body = openSection(md);
  const links = {};
  const sections = body.split(/^## /m).slice(1);
  for (const s of sections) {
    const idm = s.match(/board card (c\w+)/);
    if (!idm) continue;
    const decs = [...s.matchAll(/^###\s+(D-[A-Z0-9-]+|S\d+[A-Z-]*|N\d+|U\d+)\s+—/gm)].map((m) => m[1]);
    if (decs.length) links[idm[1]] = (links[idm[1]] || []).concat(decs);
  }
  return links;
}

// Parse the open region into a flat, ordered list of entries.
//   • Group section "## <name> — board card cXX" → group label for cards beneath.
//   • Full card "### <ID> — <title> (rec X)" → kind:"decision", selectable.
//   • Group head "### <name>" with "- **<ID>** — …" bullets → kind:"open".
//   • Loose prose → kind:"explainer".
export function parseBallot(md) {
  const body = openSection(md);
  if (!body) return [];
  const blocks = [];
  let pre = null, cur = null, group = "";
  const flushPre = () => { if (pre && pre.join("").trim()) blocks.push({ header: null, group, lines: pre }); pre = null; };
  for (const line of body.split("\n")) {
    if (line.startsWith("### ")) {
      if (cur) blocks.push(cur); else flushPre();
      cur = { header: line.slice(4).trim(), group, lines: [] };
    } else if (line.startsWith("## ")) {
      if (cur) { blocks.push(cur); cur = null; } else flushPre();
      group = line.slice(3).trim();
    } else if (cur) cur.lines.push(line);
    else (pre ??= []).push(line);
  }
  if (cur) blocks.push(cur); else flushPre();

  const out = [];
  for (const blk of blocks) {
    if (blk.header === null) {
      const html = renderMd(blk.lines.join("\n"));
      if (html.trim()) out.push({ kind: "explainer", group: blk.group, title: "", html });
      continue;
    }
    const dash = blk.header.indexOf(" — ");
    const maybeId = dash > 0 ? blk.header.slice(0, dash).trim() : "";
    if (dash > 0 && DECISION_ID.test(maybeId)) {
      let title = blk.header.slice(dash + 3).trim();
      let rec = "";
      const rm = title.match(/\(([^)]*)\)\s*$/);
      if (rm) { rec = rm[1].trim(); title = title.slice(0, rm.index).trim(); }
      out.push({ kind: "decision", id: maybeId, group: blk.group, title, rec, card: groupCardId(blk.group), ...splitCard(blk.lines) });
    } else if (bulletItems(blk.lines).length) {
      for (const item of bulletItems(blk.lines)) {
        const m = item.match(/^\*\*([^*]+)\*\*\s*(?:—|-)?\s*([\s\S]*)$/);
        const id = m ? m[1].trim() : "";
        const rest = m ? m[2].trim() : item;
        out.push({ kind: "open", group: blk.group, id, html: renderMd("- " + rest) });
      }
    } else {
      const html = renderMd(blk.lines.join("\n"));
      if (html.trim()) out.push({ kind: "explainer", group: blk.group, title: blk.header, html });
    }
  }
  return out;
}

const groupCardId = (g) => (g.match(/board card (c\w+)/) || [, ""])[1];

function bulletItems(lines) {
  const items = [];
  for (const raw of lines) {
    if (/^- /.test(raw)) items.push(raw.replace(/^- /, ""));
    else if (items.length && /^\s+\S/.test(raw)) items[items.length - 1] += " " + raw.trim();
  }
  return items;
}

// Rich card schema. Labeled sections (any order, before the options) are pulled
// into named fields; options + recommendation parse as before. Unlabeled prose
// before the first option falls into `intro`, so v1 cards still render.
//   **Gist:** one plain sentence.        → gist
//   **Story.** / **User story.** …        → story   (American-name persona)
//   **In the wild:** ```code```           → inWild  (real-project example)
//   **Other languages:** ```code```       → otherLangs (cross-language compare)
//   **Tradeoffs:** | table |              → tradeoffs (subagent-reviewed)
//   - **Option X — Name.** …              → options[]
//   **Recommendation:** …                 → recommendation
const SECTION_LABELS = {
  "gist": "gist", "story": "story", "user story": "story",
  "in the wild": "inWild", "in practice": "inWild",
  "other languages": "otherLangs", "elsewhere": "otherLangs",
  "tradeoffs": "tradeoffs", "trade-offs": "tradeoffs",
  "recommendation": "rec",
};

export function splitCard(lines) {
  // Some option headers hard-wrap before their closing **; rejoin them.
  const merged = [];
  for (let i = 0; i < lines.length; i++) {
    let l = lines[i];
    if (/^- \*\*Option /.test(l)) {
      while ((l.match(/\*\*/g) || []).length < 2 && i + 1 < lines.length) l += " " + lines[++i].trim();
    }
    merged.push(l);
  }
  lines = merged;

  const isOpt = (l) => /^- \*\*Option \S+ —/.test(l);
  const labelOf = (l) => {
    const m = l.match(/^\*\*([^*]+?)[:.]\*\*\s*(.*)$/);
    if (!m) return null;
    const key = SECTION_LABELS[m[1].trim().toLowerCase()];
    return key ? { key, rest: m[2] } : null;
  };

  const isQA = (l) => /^\*\*Owner Q\b/i.test(l) || /^\*\*Q[:.\s]/i.test(l);
  const isHr = (l) => /^\s*([-*_])\1{2,}\s*$/.test(l); // --- *** ___ separators

  const sec = { gist: [], intro: [], story: [], inWild: [], otherLangs: [], tradeoffs: [], rec: [], qa: [] };
  const options = [];
  let mode = "intro", optBuf = null;
  const flushOpt = () => { if (optBuf) { options.push(finishOption(optBuf)); optBuf = null; } };
  for (const line of lines) {
    if (isHr(line)) continue; // drop in-card rules so sections stay clean
    if (isOpt(line)) {
      flushOpt(); mode = "opt";
      const m = line.match(/^- \*\*Option (\S+) — (.+?)\*\*(.*)$/);
      const name = m[2].replace(/\.\s*$/, "").replace(/\s*\(recommended\)\s*$/i, "").trim();
      optBuf = { key: m[1], name, recommended: /\(recommended\)/i.test(m[2]), lines: [m[3].trim()] };
      continue;
    }
    // Owner Q&A — keep it, but out of the recommendation/option text (kills clutter).
    if (isQA(line)) { if (mode !== "qa") flushOpt(); mode = "qa"; sec.qa.push(line); continue; }
    const lab = mode !== "opt" || optionsClosed(line) ? labelOf(line) : null;
    if (lab) { flushOpt(); mode = lab.key; if (lab.rest) sec[mode].push(lab.rest); continue; }
    if (mode === "opt") optBuf.lines.push(line);
    else sec[mode].push(line);
  }
  flushOpt();

  const md = (arr) => renderMd(arr.join("\n").trim());
  return {
    gist: sec.gist.join(" ").trim(),
    intro: md(sec.intro),
    story: md(sec.story),
    inWild: md(sec.inWild),
    otherLangs: md(sec.otherLangs),
    tradeoffs: md(sec.tradeoffs),
    options,
    recommendation: md(sec.rec),
    qa: md(sec.qa),
  };
}
// A label after options only counts if it's the recommendation (closes the deck).
function optionsClosed(l) { return /^\*\*Recommendation[:.]\*\*/i.test(l); }
function finishOption(o) {
  return { key: o.key, name: o.name, recommended: o.recommended, html: renderMd(o.lines.join("\n").trim()) };
}

// ---- ballot-results parser (the merge target) ------------------------------

export function parseResults(md) {
  const map = new Map();
  let cur = null;
  for (const raw of md.split("\n")) {
    const idm = raw.match(/^\*\*([^*]+)\*\*\s*—\s*(.*)$/);
    if (idm) { cur = { id: idm[1].trim(), title: idm[2].trim(), choice: "", comment: "" }; map.set(cur.id, cur); continue; }
    if (!cur) continue;
    const dm = raw.match(/^Decision:\s*\*\*(.+?)\*\*\s*$/);
    if (dm) { cur.choice = dm[1].trim(); continue; }
    const cm = raw.match(/^Comment:\s*(.*)$/);
    if (cm) { cur.comment = cm[1].trim(); continue; }
  }
  return map;
}

export function answeredIds() { return new Set([...parseResults(read(P.results)).keys()]); }
