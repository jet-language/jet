#!/usr/bin/env node
// D-ONCE-LAW1=A: report dispositions name the home of each finding; this
// guard reads the live and retired Tower ledgers instead of trusting prose.

"use strict";

import fs from "node:fs";
import path from "node:path";

function usage() {
  console.error("usage: check-audit-dispositions.mjs [--root PATH]");
  process.exitCode = 2;
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`${file}: ${error.message}`);
  }
}

function markdownCells(line) {
  let value = line.trim();
  if (value.startsWith("|")) value = value.slice(1);
  if (value.endsWith("|")) value = value.slice(0, -1);
  return value.split("|").map((cell) => cell.trim());
}

function refs(text, pattern) {
  return [...text.matchAll(pattern)].map((match) => match[0]);
}

function cardRefs(text) {
  return refs(text, /#\d+\b/g);
}

function decisionRefs(text) {
  return refs(text, /D-[A-Z0-9]+(?:-[A-Z0-9]+)*(?:=[A-Z0-9]+)?/g);
}

function boardIndex(root) {
  const tower = readJson(path.join(root, "plugins/tower", ".tower", "tower.json"));
  const history = readJson(path.join(root, "plugins/tower", ".tower", "history.json"));
  const cards = new Map();
  for (const card of [...(history.cards ?? []), ...(tower.cards ?? [])]) {
    cards.set(String(card.num), card);
  }
  const decisions = new Map();
  for (const decision of [...(history.decisions ?? []), ...(tower.decisions ?? [])]) {
    decisions.set(String(decision.id), decision);
  }
  return { cards, decisions };
}

function parseReport(file, index) {
  const relative = path.relative(index.root, file).replaceAll(path.sep, "/");
  const text = fs.readFileSync(file, "utf8");
  const start = text.indexOf("<!-- audit-dispositions:v1 -->");
  const end = text.indexOf("<!-- /audit-dispositions -->", start + 1);
  const errors = [];
  if (start < 0 || end < 0 || end < start) {
    return {
      relative,
      rows: 0,
      errors: [`${relative}: missing audit-dispositions:v1 table`],
    };
  }

  const block = text.slice(start + "<!-- audit-dispositions:v1 -->".length, end);
  const lines = block.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const headerIndex = lines.findIndex((line) => line.startsWith("|"));
  if (headerIndex < 0) {
    return { relative, rows: 0, errors: [`${relative}: disposition table has no header`] };
  }
  const header = markdownCells(lines[headerIndex]).map((cell) => cell.toLowerCase());
  const expected = ["finding", "disposition", "target or reason"];
  if (header.length !== expected.length || header.some((cell, i) => cell !== expected[i])) {
    errors.push(`${relative}: disposition header must be | finding | disposition | target or reason |`);
  }

  const rows = [];
  const ids = new Set();
  for (const line of lines.slice(headerIndex + 1)) {
    if (!line.startsWith("|")) continue;
    const cells = markdownCells(line);
    if (cells.every((cell) => /^-+$/.test(cell))) continue;
    if (cells.length !== 3) {
      errors.push(`${relative}: disposition row must have three columns: ${line}`);
      continue;
    }
    const [finding, disposition, target] = cells;
    if (!finding || !ids.add(finding)) {
      errors.push(`${relative}: finding IDs must be non-empty and unique: ${finding || "<empty>"}`);
    }
    const kind = disposition.toLowerCase();
    if (!["card", "decision", "no-action"].includes(kind)) {
      errors.push(`${relative}: ${finding} has unknown disposition ${disposition}`);
      continue;
    }
    if (!target || ["—", "-", "none", "tbd", "todo"].includes(target.toLowerCase())) {
      errors.push(`${relative}: ${finding} needs a target or no-action reason`);
      continue;
    }
    if (kind === "card") {
      const found = cardRefs(target);
      if (found.length === 0) {
        errors.push(`${relative}: ${finding} card disposition names no Tower card`);
      }
      for (const ref of found) {
        const card = index.cards.get(ref.slice(1));
        if (!card) errors.push(`${relative}: ${finding} names missing Tower card ${ref}`);
      }
    } else if (kind === "decision") {
      const found = decisionRefs(target);
      if (found.length === 0) {
        errors.push(`${relative}: ${finding} decision disposition names no decision`);
      }
      for (const ref of found) {
        const id = ref.split("=")[0];
        const decision = index.decisions.get(id);
        if (!decision) {
          errors.push(`${relative}: ${finding} names missing Tower decision ${ref}`);
        } else if (decision.status !== "ratified") {
          errors.push(`${relative}: ${finding} names non-ratified Tower decision ${ref}`);
        }
      }
    } else if (target.length < 12) {
      errors.push(`${relative}: ${finding} no-action disposition needs a concrete reason`);
    }
    rows.push({ finding, disposition: kind, target });
  }
  if (rows.length === 0) errors.push(`${relative}: disposition table has no finding rows`);
  return { relative, rows: rows.length, errors };
}

function auditSkillCheck(root) {
  const skillsRoot = path.join(root, ".agents", "skills");
  const required = ".agents/skills/_shared/audit-dispositions.md";
  const skills = fs.readdirSync(skillsRoot, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name.endsWith("-audit"))
    .map((entry) => path.join(skillsRoot, entry.name, "SKILL.md"))
    .filter((file) => fs.existsSync(file))
    .sort();
  const errors = [];
  for (const file of skills) {
    const relative = path.relative(root, file).replaceAll(path.sep, "/");
    if (!fs.readFileSync(file, "utf8").includes(required)) {
      errors.push(`${relative}: missing shared audit disposition contract`);
    }
  }
  return { skills, errors };
}

function main() {
  const args = process.argv.slice(2);
  let root = process.cwd();
  for (let i = 0; i < args.length; i += 1) {
    if (args[i] === "--root" && args[i + 1]) {
      root = path.resolve(args[++i]);
    } else if (args[i] === "--help" || args[i] === "-h") {
      usage();
      return;
    } else {
      usage();
      return;
    }
  }
  const audits = path.join(root, "docs", "audits");
  const index = { ...boardIndex(root), root };
  const skillCheck = auditSkillCheck(root);
  const reports = fs.readdirSync(audits)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .sort()
    .map((name) => path.join(audits, name));
  const results = reports.map((file) => parseReport(file, index));
  const errors = [...skillCheck.errors, ...results.flatMap((result) => result.errors)];

  console.log(`audit disposition contract: ${skillCheck.skills.length} audit skills require the shared table`);
  console.log(`audit dispositions: ${reports.length} reports`);
  for (const result of results) {
    console.log(`  ${result.relative}: ${result.rows} finding dispositions`);
  }
  if (errors.length) {
    console.error(`audit dispositions: ${errors.length} unresolved ledger errors`);
    for (const error of errors) console.error(`  ${error}`);
    process.exitCode = 1;
  } else {
    console.log("audit dispositions: 0 unresolved findings");
  }
}

main();
