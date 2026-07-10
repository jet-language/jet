#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const LEDGER_PATH = join(ROOT, "docs/plans/epoch-3/capability-ledger.json");
const DEFAULT_TOWER_PATH = join(ROOT, ".tower/tower.json");

const CLASSES = Object.freeze({
  reserved: "Public shape is recognized but does not execute.",
  facade: "Public shape executes through a mock, transcript, placeholder, silent omission, or non-production backend.",
  partial: "A named subset executes and unsupported behavior fails loudly with a Jet diagnostic.",
  implemented: "Complete documented behavior executes on one supported path.",
  proven: "Implemented behavior passes every applicable live, hostile, tier, platform, scale, recovery, and dogfood lane.",
});

const SOURCE_PREFIXES = ["Source/", "crates/", "corelib/", "examples/"];
const TEST_PREFIXES = ["tests/", "tools/ci/", "scripts/"];
const DOC_PREFIXES = ["docs/", "README.md", "AGENTS.md"];
const PATH_PATTERN = /(?:Source|crates|corelib|examples|tests|tools|scripts|docs)\/[A-Za-z0-9_./-]+|(?:README|AGENTS)\.md/g;
const COMMAND_PATTERN = /(?:nix develop -c )?(?:cargo (?:test|check|run)[^\n.;]*|scripts\/agent\/verify-[A-Za-z0-9_.\/-]+[^\n.;]*)/g;
const DECISION_PATTERN = /\bD-[A-Z0-9][A-Z0-9-]*\b/g;
const RESOLUTION_PATTERN = /\b(?:merged into|superseded|decline|declined|rejected|deferred|deferral|no implementation needed|duplicate of|absorbed into|moved to|split into|tracked by|canonical card|already shipped|note recovery)\b/i;
const STRONG_PROOF_PATTERN = /\b(?:done|verified|verification|passed|green|shipped|implemented|full suite|cargo test|verify-full\.sh)\b/i;
const FACADE_PATTERN = /\b(?:facade|mock|transcript|placeholder|silent(?:ly)?|schema-only|fake|no-op|stub)\b/i;
const PARTIAL_PATTERN = /\b(?:partial|subset|staged|missing|incomplete|gap|not yet|reopened)\b/i;
const PLAN_ONLY_PATTERN = /\b(?:plan added|plan attached|plan integrated|implementation-ready plan|advanced to ready|all decisions ratified|decision cleared)\b/i;
const NON_CAPABILITY_PATTERN = /\b(?:audit|decision record|research|status matrix|inventory|explain|ratchet)\b/i;
const ASSURANCE_PATTERN = /(?:audit|ratchet|test harness|unblock full suite|reconcile|reconciliation|truthfulness)/i;
const AUDIT_REOPEN = new Map([
  [25, ["partial", "Canonical static-guarantees plan has no executable facts/policy engine proof."]],
  [37, ["reserved", "Ratified discard surface has planning only; no parser, sema, diagnostics, example, or test proof."]],
  [63, ["reserved", "Ratified prelude opt-out has planning only; no loader/sema behavior or executable proof."]],
  [79, ["reserved", "Docs-only maturity convention was advanced to ready without a completion/proof log."]],
  [136, ["reserved", "Adaptive runtime contains a ratified plan only; providers and policy runtime are not implemented."]],
  [142, ["reserved", "Logic-programming subset contains a ratified plan only; no executable solver surface exists."]],
  [143, ["reserved", "Structural merge contains a ratified plan only; no semantic merge implementation exists."]],
  [240, ["reserved", "Progressive proof/replay card records a plan seed only; `jet prove` is not implemented."]],
  [241, ["reserved", "Typed budget card records a plan only; budget commands and enforcement are not implemented."]],
  [346, ["facade", "JetPlay proof is a headless transcript/workbench slice, not the playable game/editor/runtime promised by the card."]],
]);
const EVIDENCE_HINTS = new Map([
  [94, [
    "crates/jet-sema/src/Sema/CheckerInfer/expr.rs",
    "tests/ui/E2712_splice_outside_comptime.jet",
    "docs/spec/diagnostics.md",
  ]],
  [159, ["Source/main.rs", "tests/cli.rs", "docs/design/frontends/DESIGN-BRIEF.md"]],
  [266, [
    "crates/jet-parser/src/Parser/Statements.rs",
    "tests/syntax_reconciliation.rs",
    "docs/reference/syntax-surface.jet",
  ]],
  [269, [
    "crates/jet-foundation/src/Syntax/predicates.rs",
    "tests/corelib.rs",
    "docs/spec/syntax-decisions.md",
  ]],
]);

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stable(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

function boardShape(card) {
  return {
    id: card.id,
    num: card.num,
    title: card.title,
    body: card.body ?? "",
    kind: card.kind,
    track: card.track,
    epoch: card.epoch,
    milestoneId: card.milestoneId ?? null,
    phase: card.phase,
    priority: card.priority,
    plan: card.plan ?? null,
    blockedBy: card.blockedBy ?? [],
    decisions: (card.decisions ?? []).map(({ id, status, outcome }) => ({ id, status, outcome: outcome ?? null })),
  };
}

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function cardText(card) {
  return [card.title, card.body, card.plan, ...(card.log ?? []).map((entry) => entry.text)]
    .filter(Boolean)
    .join("\n");
}

function cleanPath(path) {
  const cleaned = path.replace(/[),:`'"\]}]+$/g, "").replace(/\.{2,}$/g, "");
  return cleaned.startsWith("tools/Tower/docs/") ? cleaned.slice("tools/Tower/".length) : cleaned;
}

function filesUnder(path, out = []) {
  if (!existsSync(path)) return out;
  const stat = statSync(path);
  if (!stat.isDirectory()) {
    out.push(path);
    return out;
  }
  for (const name of readdirSync(path).sort()) {
    if ([".git", "target", "node_modules", ".tower"].includes(name)) continue;
    filesUnder(join(path, name), out);
  }
  return out;
}

function explicitEvidence(card) {
  const paths = new Set([
    ...(cardText(card).match(PATH_PATTERN) ?? []).map(cleanPath),
    ...(EVIDENCE_HINTS.get(card.num) ?? []),
  ]);
  const commands = new Set(cardText(card).match(COMMAND_PATTERN) ?? []);
  for (const command of commands) {
    const match = command.match(/cargo test\s+--test\s+([A-Za-z0-9_-]+)/);
    if (match) paths.add(`tests/${match[1]}.rs`);
  }

  const evidence = [];
  for (const rel of [...paths].sort()) {
    let actualRel = rel;
    let absolute = join(ROOT, actualRel);
    if (existsSync(absolute) && statSync(absolute).isDirectory() && existsSync(`${absolute}.rs`)) {
      actualRel = `${actualRel}.rs`;
      absolute = `${absolute}.rs`;
    }
    if (!existsSync(absolute) || statSync(absolute).isDirectory()) continue;
    const kind = actualRel.endsWith(".md")
      ? "docs"
      : SOURCE_PREFIXES.some((prefix) => actualRel.startsWith(prefix))
      ? "source"
      : TEST_PREFIXES.some((prefix) => actualRel.startsWith(prefix))
        ? "test"
        : DOC_PREFIXES.some((prefix) => actualRel.startsWith(prefix))
          ? "docs"
          : null;
    if (!kind) continue;
    evidence.push({ kind, path: actualRel, sha256: sha256(readFileSync(absolute)) });
  }
  return { evidence, commands: [...commands].map((command) => command.trim()).sort() };
}

function indexedEvidence(card, index) {
  const text = cardText(card);
  const decisions = new Set([
    ...(card.decisions ?? []).map((decision) => decision.id),
    ...(cardText(card).match(DECISION_PATTERN) ?? []),
  ]);
  if (decisions.size === 0) return [];
  const evidence = [];
  for (const [rel, text] of index) {
    const testStem = rel.match(/^tests\/([A-Za-z0-9_-]+)\.rs$/)?.[1];
    const stemNamed = testStem && new RegExp(`\\b${testStem.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "i").test(cardText(card));
    const decisionNamed = [...decisions].some((id) => text.includes(id));
    if (!decisionNamed && !stemNamed) continue;
    const kind = rel.endsWith(".md")
      ? "docs"
      : rel.startsWith("tests/") || rel.startsWith("tools/ci/")
      ? "test"
      : SOURCE_PREFIXES.some((prefix) => rel.startsWith(prefix))
        ? "source"
        : rel.startsWith("docs/")
          ? "docs"
          : null;
    if (!kind) continue;
    evidence.push({ kind, path: rel, sha256: sha256(text) });
  }
  return evidence;
}

function makeIndex() {
  const roots = ["Source", "crates", "corelib", "examples", "tests", "tools/ci", "docs/spec", "docs/reference"];
  const index = [];
  for (const relRoot of roots) {
    for (const absolute of filesUnder(join(ROOT, relRoot))) {
      try {
        const text = readFileSync(absolute, "utf8");
        index.push([relative(ROOT, absolute), text]);
      } catch {
        // Binary proof artifacts are referenced explicitly and hashed there.
      }
    }
  }
  return index;
}

function uniqueEvidence(items) {
  const seen = new Set();
  return items.filter((item) => {
    const key = `${item.kind}:${item.path}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function compactEvidence(items) {
  const counts = new Map();
  return items.filter((item) => {
    const count = counts.get(item.kind) ?? 0;
    if (count >= 2) return false;
    counts.set(item.kind, count + 1);
    return true;
  });
}

function commandsForEvidence(evidence) {
  const commands = new Set();
  for (const item of evidence) {
    const topLevel = item.path.match(/^tests\/([A-Za-z0-9_-]+)\.rs$/);
    if (topLevel) commands.add(`nix develop -c cargo test --test ${topLevel[1]}`);
    if (item.path.startsWith("tests/ui/")) {
      commands.add("nix develop -c cargo test --test diagnostic_snapshots ui_snapshots");
    }
    if (item.path.startsWith("examples/features/")) {
      commands.add("nix develop -c cargo test --test golden examples_compile_and_run");
    }
  }
  return [...commands];
}

function matchingLog(card, pattern) {
  return (card.log ?? []).find((entry) => pattern.test(entry.text));
}

function towerEvidence(entry) {
  if (!entry) return null;
  return {
    at: entry.at,
    by: entry.by ?? null,
    textSha256: sha256(entry.text),
  };
}

function classifyOpen(text) {
  if (FACADE_PATTERN.test(text)) return "facade";
  if (PARTIAL_PATTERN.test(text)) return "partial";
  return "reserved";
}

function auditCard(card, index) {
  const text = cardText(card);
  const explicit = explicitEvidence(card);
  const evidence = compactEvidence(uniqueEvidence([...explicit.evidence, ...indexedEvidence(card, index)]));
  const commands = [...new Set([...explicit.commands, ...commandsForEvidence(evidence)])].sort();
  const source = evidence.some((item) => item.kind === "source");
  const test = evidence.some((item) => item.kind === "test");
  const docs = evidence.some((item) => item.kind === "docs");
  const executableSpec = source && commands.some((command) => command.includes("--test golden"));
  const assurance = card.kind === "bug" || ASSURANCE_PATTERN.test(`${card.title}\n${card.body ?? ""}`);
  const proofLog = matchingLog(card, STRONG_PROOF_PATTERN);
  const resolutionLog = matchingLog(card, RESOLUTION_PATTERN);
  const planOnlyLog = matchingLog(card, PLAN_ONLY_PATTERN);

  const auditOverride = AUDIT_REOPEN.get(card.num);
  let classification = auditOverride?.[0] ?? classifyOpen(text);
  let closure = "open";
  let claimKind = assurance ? "assurance" : "capability";
  let disposition = null;
  let reason = auditOverride?.[1] ?? "Capability remains open; current class follows source-backed audit language.";
  let log = proofLog ?? card.log?.[0] ?? null;

  const forcedReopen = card.phase === "done" ? auditOverride : null;
  if (forcedReopen) {
    closure = "reopen";
    [classification, reason] = forcedReopen;
    log = card.log?.[0] ?? null;
  } else if (card.phase === "done" && resolutionLog) {
    closure = "resolution";
    claimKind = "resolution";
    disposition = resolutionLog.text.match(RESOLUTION_PATTERN)?.[0].toLowerCase() ?? "resolved";
    classification = STRONG_PROOF_PATTERN.test(resolutionLog.text) ? "implemented" : classification;
    reason = "Card closes a recorded disposition, not a broad capability claim.";
    log = resolutionLog;
  } else if (card.phase === "done" && NON_CAPABILITY_PATTERN.test(`${card.title}\n${card.body ?? ""}`) && proofLog && docs) {
    closure = "resolution";
    claimKind = "resolution";
    disposition = "completed audit artifact";
    classification = "implemented";
    reason = "Card closes a checked audit artifact, not a runtime capability claim.";
    log = proofLog;
  } else if (card.phase === "done" && proofLog && assurance && test && commands.length > 0) {
    closure = "proof";
    classification = "proven";
    reason = "Assurance claim has executable test, command, and Tower proof evidence.";
    log = proofLog;
  } else if (card.phase === "done" && proofLog && source && (test || executableSpec) && docs && commands.length > 0) {
    closure = "proof";
    classification = "proven";
    reason = "Done claim has implementation, executable test, docs, command, and Tower proof evidence.";
    log = proofLog;
  } else if (card.phase === "done") {
    closure = "reopen";
    classification = FACADE_PATTERN.test(text) ? "facade" : PARTIAL_PATTERN.test(text) ? "partial" : "implemented";
    reason = planOnlyLog
      ? "Done state is backed by planning or decision clearance, not executable proof."
      : "Done state lacks a complete source, test, docs, command, and Tower proof bundle.";
    log = planOnlyLog ?? proofLog ?? card.log?.[0] ?? null;
  }

  return {
    cardId: card.id,
    num: card.num,
    title: card.title,
    boardPhase: card.phase,
    claimKind,
    classification,
    closure,
    ...(disposition ? { disposition } : {}),
    reason,
    boardDigest: sha256(stable(boardShape(card))),
    towerEvidence: towerEvidence(log),
    evidence,
    commands,
  };
}

function generate(towerPath) {
  const board = readJson(towerPath);
  const cards = board.cards
    .filter((card) => card.track === "epoch" && card.epoch === "e3")
    .sort((left, right) => left.num - right.num);
  const index = makeIndex();
  return {
    schemaVersion: 1,
    scope: { track: "epoch", epoch: "e3" },
    evidenceClasses: CLASSES,
    policy: {
      capabilityDoneRequires: "proven",
      resolutionMayCloseWithoutProof: true,
      proofBundle: {
        capability: ["source", "test-or-executable-example", "docs", "command", "tower"],
        assurance: ["test", "command", "tower"],
        resolution: ["disposition", "tower"],
      },
      fileEvidence: "sha256",
    },
    cardCount: cards.length,
    cards: cards.map((card) => auditCard(card, index)),
  };
}

function validateEvidence(item, root = ROOT) {
  if (!["source", "test", "docs", "target"].includes(item.kind)) {
    throw new Error(`unknown evidence kind ${item.kind}`);
  }
  const path = join(root, item.path);
  if (!existsSync(path) || statSync(path).isDirectory()) {
    throw new Error(`missing ${item.kind} proof ${item.path}`);
  }
  const actual = sha256(readFileSync(path));
  if (actual !== item.sha256) {
    throw new Error(`tampered ${item.kind} proof ${item.path}: expected ${item.sha256}, got ${actual}`);
  }
}

function validate(ledger, board, root = ROOT) {
  const errors = [];
  const fail = (message) => errors.push(message);
  if (ledger.schemaVersion !== 1) fail(`unsupported schemaVersion ${ledger.schemaVersion}`);
  if (stable(ledger.evidenceClasses) !== stable(CLASSES)) fail("five evidence-class definitions drifted");

  const boardCards = board.cards
    .filter((card) => card.track === "epoch" && card.epoch === "e3")
    .sort((left, right) => left.num - right.num);
  const rows = new Map(ledger.cards.map((row) => [row.cardId, row]));
  if (rows.size !== ledger.cards.length) fail("duplicate capability-ledger card ids");
  if (ledger.cardCount !== boardCards.length || ledger.cards.length !== boardCards.length) {
    fail(`E3 coverage drift: board=${boardCards.length}, ledger=${ledger.cards.length}, declared=${ledger.cardCount}`);
  }

  for (const card of boardCards) {
    const row = rows.get(card.id);
    if (!row) {
      fail(`missing E3 card #${card.num} ${card.title}`);
      continue;
    }
    if (row.num !== card.num || row.title !== card.title || row.boardPhase !== card.phase) {
      fail(`Tower identity/phase drift for #${card.num}`);
    }
    if (row.boardDigest !== sha256(stable(boardShape(card)))) fail(`Tower claim drift for #${card.num}`);
    if (!Object.hasOwn(CLASSES, row.classification)) fail(`invalid class for #${card.num}: ${row.classification}`);
    if (!["capability", "assurance", "resolution"].includes(row.claimKind)) {
      fail(`invalid claim kind for #${card.num}: ${row.claimKind}`);
    }

    for (const item of row.evidence ?? []) {
      try {
        validateEvidence(item, root);
      } catch (error) {
        fail(`#${card.num}: ${error.message}`);
      }
    }

    if (row.towerEvidence) {
      const matches = (card.log ?? []).some((entry) =>
        entry.at === row.towerEvidence.at
        && (entry.by ?? null) === row.towerEvidence.by
        && sha256(entry.text) === row.towerEvidence.textSha256);
      if (!matches) fail(`#${card.num}: Tower proof log was deleted or changed`);
    }

    if (card.phase === "done" && row.closure === "proof") {
      const kinds = new Set((row.evidence ?? []).map((item) => item.kind));
      if (row.classification !== "proven") fail(`#${card.num}: proof closure must be class proven`);
      const hasGolden = (row.commands ?? []).some((command) => command.includes("--test golden"));
      const required = row.claimKind === "assurance" ? ["test"] : ["source", "docs"];
      for (const kind of required) {
        if (!kinds.has(kind)) fail(`#${card.num}: proof closure lacks ${kind} evidence`);
      }
      if (row.claimKind === "capability" && !kinds.has("test") && !hasGolden) {
        fail(`#${card.num}: capability proof lacks test or executable-example evidence`);
      }
      if (!(row.commands ?? []).length) fail(`#${card.num}: proof closure lacks executable command`);
      if (!row.towerEvidence) fail(`#${card.num}: proof closure lacks Tower log evidence`);
    } else if (card.phase === "done" && row.closure === "resolution") {
      if (!row.disposition || !row.towerEvidence) fail(`#${card.num}: resolution lacks disposition evidence`);
    } else if (card.phase === "done") {
      fail(`#${card.num}: done claim is ${row.classification}/${row.closure}; reopen or attach complete proof`);
    }
  }

  for (const row of ledger.cards) {
    if (!boardCards.some((card) => card.id === row.cardId)) fail(`ledger contains stale card ${row.cardId}`);
  }

  if (errors.length) throw new Error(errors.join("\n"));
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), "jet-capability-ledger-"));
  try {
    writeFileSync(join(dir, "source.rs"), "fn real() {}\n");
    writeFileSync(join(dir, "test.rs"), "#[test] fn live() {}\n");
    writeFileSync(join(dir, "doc.md"), "# Real capability\n");
    const card = {
      id: "cproof",
      num: 1,
      title: "Proof",
      body: "Proof body",
      kind: "feature",
      track: "epoch",
      epoch: "e3",
      milestoneId: null,
      phase: "done",
      priority: "P0",
      plan: null,
      blockedBy: [],
      decisions: [],
      log: [{ at: "now", by: "test", text: "verified live" }],
    };
    const row = {
      cardId: card.id,
      num: card.num,
      title: card.title,
      boardPhase: card.phase,
      claimKind: "capability",
      classification: "proven",
      closure: "proof",
      reason: "self-test",
      boardDigest: sha256(stable(boardShape(card))),
      towerEvidence: towerEvidence(card.log[0]),
      evidence: [
        { kind: "source", path: "source.rs", sha256: sha256(readFileSync(join(dir, "source.rs"))) },
        { kind: "test", path: "test.rs", sha256: sha256(readFileSync(join(dir, "test.rs"))) },
        { kind: "docs", path: "doc.md", sha256: sha256(readFileSync(join(dir, "doc.md"))) },
      ],
      commands: ["cargo test --test live"],
    };
    const ledger = { schemaVersion: 1, evidenceClasses: CLASSES, cardCount: 1, cards: [row] };
    const board = { cards: [card] };
    validate(ledger, board, dir);

    rmSync(join(dir, "test.rs"));
    let deletedRejected = false;
    try { validate(ledger, board, dir); } catch { deletedRejected = true; }
    if (!deletedRejected) throw new Error("deleted proof was accepted");

    writeFileSync(join(dir, "test.rs"), "#[test] fn fake() {}\n");
    let tamperRejected = false;
    try { validate(ledger, board, dir); } catch { tamperRejected = true; }
    if (!tamperRejected) throw new Error("tampered proof was accepted");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  process.stdout.write("capability-ledger tamper gate: ok\n");
}

function towerPathFromArgs(args) {
  const index = args.indexOf("--tower");
  if (index >= 0) {
    if (!args[index + 1]) throw new Error("--tower requires a path");
    return resolve(args[index + 1]);
  }
  return resolve(process.env.TOWER_DATA ?? DEFAULT_TOWER_PATH);
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes("--self-test")) {
    selfTest();
    return;
  }
  const towerPath = towerPathFromArgs(args);
  if (args.includes("--generate")) {
    const ledger = generate(towerPath);
    writeFileSync(LEDGER_PATH, `${JSON.stringify(ledger, null, 2)}\n`);
    const reopen = ledger.cards.filter((row) => row.closure === "reopen");
    process.stdout.write(`wrote ${relative(ROOT, LEDGER_PATH)}: ${ledger.cards.length} cards, ${reopen.length} reopen\n`);
    for (const row of reopen) process.stdout.write(`#${row.num} ${row.classification}: ${row.reason}\n`);
    return;
  }
  if (args.includes("--check")) {
    validate(readJson(LEDGER_PATH), readJson(towerPath));
    process.stdout.write("capability ledger: current and proof-bound\n");
    return;
  }
  throw new Error("usage: check-capability-ledger.mjs --generate|--check|--self-test [--tower PATH]");
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
}
