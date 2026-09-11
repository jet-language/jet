#!/usr/bin/env node

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const REGISTRY = "crates/jet-codegen/src/Prelude/Diagnostics.jet";
const REPORTS = {
  json: "docs/audits/diagnostic-actionability-census.json",
  tsv: "docs/audits/diagnostic-actionability-census.tsv",
  markdown: "docs/audits/diagnostic-actionability-census.md",
};
const FIXTURES = "tests/fixtures/diagnostic-actionability/actionability-cases.json";
const BASELINE = "tests/fixtures/diagnostic-actionability/known-failures.json";
const GENERATED_COMMAND = "node scripts/agent/diagnostic-actionability-census.mjs --write";
const CHECK_COMMAND = "node scripts/agent/diagnostic-actionability-census.mjs --check";
const SCHEMA = "jet-diagnostic-actionability-v1";
const CODE_RE = /^(?:[A-Z]+\d{4}|[A-Z]+(?:-[A-Z0-9]+){2,})$/u;
const STATUS = new Set(["active", "retired", "reserved"]);
const SEVERITY = new Set(["error", "lint"]);
const DETAIL = new Set(["true", "false"]);

// This is deliberately data, not a comment: the report and the check both
// publish and consume this rule. A Fix is actionable only when a contributor
// can perform one named edit, run one named command, or open one named path.
// A source_edit/replacement marker is an executable edit supplied by the
// typed row. A generic imperative without a concrete object is not enough.
const ACTIONABILITY_RULE = {
  id: "edit-command-path-v1",
  text: "A Fix is actionable when it names a concrete source edit, a runnable command, or a repository/system path. Concrete edits use an imperative operation with a named code fragment, declaration, value, or other object; structured source_edit, suggested_source_edit, replace, remove, and generated markers count as executable edits. A command must name a recognized executable and subcommand (or a REPL command). A path must name a literal path or filename. Placeholders, generic advice, inspection-only advice, and prose such as `follow the guidance`, `check the definition`, or `nothing to fix` are not actionable.",
  tier_command_text: "A tier/toolchain diagnostic is actionable only when its What or Why also names the command the user ran; a command that appears only in Fix is a remedy, not the invoking command.",
};

const COMMAND_WORDS = {
  jet: "build run dev test debug fetch fix fmt format explain help check new init update self registry os push pull image inspect prove gc config clean flash publish serve watch login logout exec eval repl".split(" "),
  jetpack: "env services secrets update add tool use image hangar".split(" "),
  cargo: "build run test check fmt clippy metadata tree".split(" "),
  rustc: ["--version", "--target", "--print", "--crate-name"],
  rustup: "target toolchain component show which run".split(" "),
  nix: "build develop run shell store profile eval flake fmt".split(" "),
  git: "status commit fetch pull push clone checkout switch".split(" "),
  npm: "install run test exec".split(" "),
  pnpm: "install add run test exec".split(" "),
  yarn: "install add run test".split(" "),
  python: ["-m"],
  "wasm-pack": "build test pack".split(" "),
  wasmtime: ["run"],
  "wasm-bindgen": ["--out-dir"],
  qemu: ["system"],
  ssh: ["-V"],
  node: ["--version", "--test"],
  deno: "run test task".split(" "),
  make: ["-C"],
  cmake: ["--build", "-S"],
  clang: ["--version"],
  gcc: ["--version"],
  cc: ["--version"],
};
const QUOTED_TOOL_COMMAND = /`((?:jet|jetpack|cargo|rustc|rustup|nix|git|npm|pnpm|yarn|python(?:3)?|wasm-pack|wasmtime|wasm-bindgen|qemu(?:-[a-z0-9-]+)?|ssh|node|deno|make|cmake|clang|gcc|cc)\s+[^`\n]+)`/giu;
const COMMAND = /(?:^|[\s(`'"/])((?:jet|jetpack|cargo|rustc|rustup|nix|git|npm|pnpm|yarn|python(?:3)?|wasm-pack|wasmtime|wasm-bindgen|qemu(?:-[a-z0-9-]+)?|ssh|node|deno|make|cmake|clang|gcc|cc)\s+(?:[a-z][a-z0-9_.:/-]*|-[a-z][a-z0-9-]*(?:=[^\s,;.)]+)?))/giu;
const QUOTED_COMMAND = /`((?:pacman|apt(?:-get)?|dnf|brew|systemctl|docker|podman|make|cmake|clang|gcc|cc|wasmtime|wasm-bindgen|node|deno|python(?:3)?|bash|sh|qemu(?:-[a-z0-9-]+)?|curl|openssl|tar|chmod|mkdir|rm|cp|mv|install)\s+(?:[a-z][a-z0-9_.:/-]*|-[a-z][a-z0-9-]*(?:=[^\s,;.)]+)?))/giu;
const REPL_COMMAND = /(?:^|[\s(`'"/])(:(?:run|build|check|test|help|quit|reset))\b/giu;
const COMMAND_INTRO = /\b(?:run|using|via|with|invoke|execute|start|call|rerun)\s*$/iu;
const PATH = /(?:`[^`\n]*(?:\b[a-z0-9_.-]+\/[a-z0-9_./-]+|\.{0,2}\/[a-z0-9_./-]+|[a-z0-9_.-]+\.(?:jet|toml|jsonl|json|nix|rs|wasm|c|h|md)(?![a-z0-9_]))[^`\n]*`|<(?:(?:file|path|dir|directory|module|target)[^>]*>)|(?:^|[\s(])(?:\.{1,2}\/|\/[a-z0-9_.-]+|(?:boot|bin|lib|sbin|source|src|docs|examples|config|modules|packages|target|tests|scripts|path)\/[a-z0-9_.-]+(?:\/[a-z0-9_.-]+)*)(?=$|[\s`),.;:])|(?:^|[\s`(])[a-z0-9_.-]+\.(?:jet|toml|jsonl|json|nix|rs|wasm|c|h|md)(?=$|[\s`),.;:]))/giu;
const ACTION_VERB = /(?:^|[.;,]\s*|\b(?:and|or|then)\s+)(?:add|align|append|apply|assign|base|bind|bound|break|bring|build|bump|call|change|choose|close|combine|compare|commit|compute|configure|consume|convert|correct|create|declare|define|delete|do not use|don't use|downgrade|drop|edit|embed|enable|end|finish|fix|give|grant|handle|implement|improve|inject|inspect|install|introduce|iterate|join|keep|log|make|mark|match|merge|move|name|narrow|only use|open|pass|perform|pick|place|point|publish|project|provide|pull|put|raise|read|recapture|reconcile|reduce|refresh|remove|rename|repair|replace|rerun|reset|restore|return|retry|rewrite|route|select|send|set|split|spread|supply|target|tighten|transition|upgrade|use|verify|widen|write|wrap)\b/iu;
const VAGUE_FIX = /^(?:follow the guidance|nothing to fix|address the finding|apply the fix|fix the (?:named|reported|listed|indicated) (?:issue|problem|diagnostic|finding)|see (?:the )?(?:guidance|documentation|docs)|refer to (?:the )?(?:guidance|documentation|docs)|consult (?:the )?(?:guidance|documentation|docs)|\{fix\}|`\{fix\}`)\s*[.!]?$/iu;
const TIER_TOOLCHAIN = /\b(?:interpreter|interpreted|jit|aot|cranelift|tier(?:[- ]?[0-9]+)?|toolchain|rustc|rustup|cargo|nixpkgs|wasm|wasi|web(?: target| backend)?|native (?:build|target)|jetpack)\b/iu;

function fail(message) {
  throw new Error(`diagnostic actionability census: ${message}`);
}

function absPath(path) {
  return resolve(ROOT, path);
}

function relPath(path) {
  const relativePath = relative(ROOT, path);
  return relativePath && !relativePath.startsWith("..") ? relativePath : path;
}

function parseArgs(argv) {
  const options = {
    mode: "summary",
    json: false,
    failOnViolations: false,
    refreshBaseline: false,
    source: REGISTRY,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--write" || argument === "--check") {
      if (options.mode !== "summary") fail("choose only one of --write or --check");
      options.mode = argument.slice(2);
    } else if (argument === "--json") {
      options.json = true;
    } else if (argument === "--fail-on-violations" || argument === "--fail-on-non-actionable") {
      options.failOnViolations = true;
    } else if (argument === "--refresh-baseline") {
      options.refreshBaseline = true;
    } else if (argument === "--source") {
      if (!argv[index + 1]) fail("--source needs a path");
      options.source = argv[++index];
    } else if (argument === "--help" || argument === "-h") {
      return { help: true };
    } else {
      fail(`unknown option ${argument}`);
    }
  }
  if (options.refreshBaseline && options.mode !== "write") fail("--refresh-baseline requires --write");
  return options;
}

function usage() {
  return [
    `Usage: ${GENERATED_COMMAND}`,
    `       ${CHECK_COMMAND}`,
    "       node scripts/agent/diagnostic-actionability-census.mjs [--json] [--fail-on-violations]",
    "",
    "--write              regenerate the stable JSON, TSV, and Markdown reports.",
    "--check              verify reports, fixtures, and the known-failure regression baseline.",
    "--json               emit the complete machine-readable report.",
    "--fail-on-violations fail if the current active registry has any actionability failure.",
    "--refresh-baseline   explicitly accept the current failures as known debt (write mode only).",
    "--source PATH        read another registry-shaped file (fixture/debug use).",
    "",
    `Registry source of truth: ${REGISTRY}`,
    `Rule: ${ACTIONABILITY_RULE.text}`,
    ACTIONABILITY_RULE.tier_command_text,
  ].join("\n");
}

function sourceRow(raw, lineNumber) {
  const fields = raw.split("\t");
  const errors = [];
  if (raw.startsWith(" ") || raw.startsWith("\t")) errors.push("row must start in column 1");
  if (fields.length !== 12 && fields.length !== 13) {
    errors.push(`expected 12 fields for an error or 13 for a lint, found ${fields.length}`);
  }
  if (fields[0] !== "diagnostic") errors.push("first field must be diagnostic");
  if (!fields[1] || !CODE_RE.test(fields[1])) errors.push("code does not match the diagnostic code grammar");
  if (!fields[2]) errors.push("stage is empty");
  if (!SEVERITY.has(fields[3])) errors.push("severity must be error or lint");
  if (!fields[4]) errors.push("moment is empty");
  if (!STATUS.has(fields[5])) errors.push("status must be active, retired, or reserved");
  for (const [index, name] of [[6, "meaning"], [7, "what"], [8, "why"], [9, "fix"]]) {
    if (!fields[index]) errors.push(`${name} is empty`);
  }
  if (!DETAIL.has(fields[10])) errors.push("detail must be true or false");
  if (!fields[11]) errors.push("structured-fix is empty");
  if (fields[3] === "error" && fields.length !== 12) errors.push("error rows must have no lint-name field");
  if (fields[3] === "lint" && fields.length !== 13) errors.push("lint rows require a lint-name field");
  if (fields[3] === "lint" && fields.length === 13 && !fields[12]) errors.push("lint-name is empty");
  return {
    row: fields.length === 12 || fields.length === 13 ? { fields, line: lineNumber } : null,
    errors: errors.length > 0 ? [{ line: lineNumber, text: raw, reasons: errors }] : [],
  };
}

function parseSource(source, sourcePath = REGISTRY) {
  const rows = [];
  const errors = [];
  const seen = new Map();
  for (const [index, rawLine] of source.split(/\r?\n/u).entries()) {
    const raw = rawLine.replace(/\r$/u, "");
    const trimmed = raw.trim();
    if (!trimmed || trimmed.startsWith("//")) continue;
    if (!/^diagnostic(?:\t|$)/u.test(raw)) {
      errors.push({ line: index + 1, text: raw, reasons: ["expected a diagnostic row or comment"] });
      continue;
    }
    const parsed = sourceRow(raw, index + 1);
    errors.push(...parsed.errors);
    if (!parsed.row) continue;
    const code = parsed.row.fields[1];
    if (seen.has(code)) {
      errors.push({
        line: index + 1,
        text: raw,
        reasons: [`duplicate code; first declared at line ${seen.get(code)}`],
      });
      continue;
    }
    seen.set(code, index + 1);
    rows.push({ fields: parsed.row.fields, source_line: parsed.row.line, source: sourcePath });
  }
  return { rows, errors };
}

function matches(pattern, text) {
  pattern.lastIndex = 0;
  return pattern.test(text);
}

function captures(pattern, text) {
  pattern.lastIndex = 0;
  return [...text.matchAll(pattern)].map((match) => match[1] ?? match[0]);
}

function isKnownCommand(command) {
  const normalized = command.toLowerCase().replace(/[<>}`]/gu, "").replace(/\s+/gu, " ").trim();
  const [tool, verb] = normalized.split(" ");
  if (!tool || !verb) return false;
  if (verb.startsWith("--")) return true;
  return COMMAND_WORDS[tool]?.includes(verb) ?? false;
}

function commandCaptures(text) {
  return [...new Set([
    ...captures(QUOTED_TOOL_COMMAND, text).filter(isKnownCommand),
    ...captures(COMMAND, text).filter(isKnownCommand),
    ...captures(QUOTED_COMMAND, text),
    ...captures(REPL_COMMAND, text),
  ])];
}

function mentionedCommands(text) {
  const mentions = [];
  for (const command of captures(QUOTED_TOOL_COMMAND, text)) {
    if (isKnownCommand(command)) mentions.push(command);
  }
  for (const match of text.matchAll(COMMAND)) {
    const command = match[1];
    const start = (match.index ?? 0) + match[0].indexOf(command);
    const prefix = text.slice(Math.max(0, start - 40), start);
    const quoted = text[start - 1] === "`" || text[start - 1] === '"';
    if (isKnownCommand(command) && (quoted || matches(COMMAND_INTRO, prefix))) mentions.push(command);
  }
  mentions.push(...captures(QUOTED_COMMAND, text));
  mentions.push(...captures(REPL_COMMAND, text));
  return [...new Set(mentions)];
}


function pathCaptures(text) {
  return [...new Set(captures(PATH, text).map((value) => value.trim()).filter(Boolean))];
}

function hasStructuredEdit(structuredFix) {
  return /(?:^|[| ])(?:suggested_)?source_edit(?:$|[| ])/iu.test(structuredFix)
    || /(?:^|[| ])(?:replace|remove|generated):/iu.test(structuredFix);
}

function classifyFix(fix, structuredFix) {
  const commands = commandCaptures(fix);
  const paths = pathCaptures(fix);
  const structuredEdit = hasStructuredEdit(structuredFix);
  const vague = matches(VAGUE_FIX, fix);
  const concreteEdit = !vague && matches(ACTION_VERB, fix);
  if (vague) return { kind: "non_actionable", actionable: false, commands, paths, structuredEdit };
  if (commands.length > 0) return { kind: "exact_command", actionable: true, commands, paths, structuredEdit };
  if (paths.length > 0) return { kind: "exact_path", actionable: true, commands, paths, structuredEdit };
  if (structuredEdit || concreteEdit) return { kind: "source_edit", actionable: true, commands, paths, structuredEdit };
  return { kind: "non_actionable", actionable: false, commands, paths, structuredEdit };
}

function tierContext(row) {
  const [, , stage, , , , , what, why, fix] = row.fields;
  const text = `${what}\n${why}\n${fix}`;
  return matches(TIER_TOOLCHAIN, text) || /^(?:interp|jit|tooling|compiler|driver)$/iu.test(stage);
}

function codeKey(code) {
  const numeric = code.match(/^([A-Z]+)(\d{4})$/u);
  if (numeric) return [numeric[1], 0, Number(numeric[2]), code];
  return [code, 1, 0, code];
}

function compareRows(left, right) {
  const a = codeKey(left.code);
  const b = codeKey(right.code);
  for (let index = 0; index < a.length; index += 1) {
    if (a[index] < b[index]) return -1;
    if (a[index] > b[index]) return 1;
  }
  return 0;
}
function classify(row) {
  const [, code, stage, severity, moment, status, meaning, what, why, fix, detail, structuredFix, lintName] = row.fields;
  const result = classifyFix(fix, structuredFix);
  const tierOrToolchain = tierContext(row);
  const invokingCommands = [...new Set([
    ...mentionedCommands(what),
    ...mentionedCommands(why),
  ])];
  const violations = [];
  if (status === "active" && !result.actionable) {
    violations.push("Fix must name a concrete edit, runnable command, or path");
  }
  if (status === "active" && tierOrToolchain && invokingCommands.length === 0) {
    violations.push("tier/toolchain What or Why must name the command the user ran");
  }
  return {
    code,
    stage,
    severity,
    moment,
    status,
    meaning,
    what,
    why,
    fix,
    detail,
    structured_fix: structuredFix,
    lint_name: lintName ?? null,
    source: row.source,
    source_line: row.source_line,
    fix_kind: result.kind,
    actionable: result.actionable,
    fix_commands: result.commands,
    fix_paths: result.paths,
    structured_edit: result.structuredEdit,
    tier_or_toolchain: tierOrToolchain,
    invoking_commands: invokingCommands,
    violations,
  };
}

function stableSortStrings(values) {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

function buildSummary(rows, errors) {
  const active = rows.filter((row) => row.status === "active");
  const byStatus = Object.fromEntries([...STATUS].map((status) => [status, rows.filter((row) => row.status === status).length]));
  const byFixKind = Object.fromEntries(["source_edit", "exact_command", "exact_path", "non_actionable"].map((kind) => [kind, rows.filter((row) => row.status === "active" && row.fix_kind === kind).length]));
  return {
    registered: rows.length,
    active: active.length,
    dormant: rows.length - active.length,
    status_counts: byStatus,
    actionable: active.filter((row) => row.actionable).length,
    non_actionable: active.filter((row) => !row.actionable).length,
    tier_command_gaps: active.filter((row) => row.violations.includes("tier/toolchain What or Why must name the command the user ran")).length,
    violations: active.filter((row) => row.violations.length > 0).length,
    by_fix_kind: byFixKind,
    malformed_rows: errors.length,
  };
}

function sourceDigest(source) {
  return `sha256:${createHash("sha256").update(source).digest("hex")}`;
}

function census(source, sourcePath = REGISTRY) {
  const parsed = parseSource(source, sourcePath);
  const rows = parsed.rows.map(classify).sort(compareRows);
  const failures = rows
    .filter((row) => row.status === "active" && row.violations.length > 0)
    .map((row) => ({
      code: row.code,
      source_line: row.source_line,
      fix: row.fix,
      requirements: [...row.violations],
    }));
  const report = {
    schema: SCHEMA,
    generated_by: "scripts/agent/diagnostic-actionability-census.mjs",
    generated_command: GENERATED_COMMAND,
    authoritative_registry: sourcePath,
    actionability_rule: ACTIONABILITY_RULE,
    registry_sha256: sourceDigest(source),
    summary: buildSummary(rows, parsed.errors),
    failures,
    malformed_rows: parsed.errors,
    rows,
  };
  return { report, errors: parsed.errors };
}

function tsvCell(value) {
  return String(value ?? "")
    .replace(/\\/gu, "\\\\")
    .replace(/\t/gu, "\\t")
    .replace(/\r/gu, "\\r")
    .replace(/\n/gu, "\\n");
}

const TSV_HEADER = [
  "code", "status", "stage", "severity", "moment", "source_line", "fix_kind", "actionable",
  "tier_or_toolchain", "invoking_commands", "fix_commands", "fix_paths", "violations", "fix",
];

function renderTsv(report) {
  const lines = [TSV_HEADER.join("\t")];
  for (const row of report.rows) {
    lines.push([
      row.code,
      row.status,
      row.stage,
      row.severity,
      row.moment,
      row.source_line,
      row.fix_kind,
      row.actionable,
      row.tier_or_toolchain,
      row.invoking_commands.join(" | "),
      row.fix_commands.join(" | "),
      row.fix_paths.join(" | "),
      row.violations.join(" | "),
      row.fix,
    ].map(tsvCell).join("\t"));
  }
  return `${lines.join("\n")}\n`;
}

function md(value) {
  return String(value ?? "").replace(/\\/gu, "\\\\").replace(/\|/gu, "\\|").replace(/\n/gu, " ");
}

function renderMarkdown(report) {
  const { summary } = report;
  const lines = [
    "# Diagnostic actionability census",
    "",
    "This is a static, registry-driven census. It does not run the compiler or rewrite diagnostic text.",
    "",
    "## Command",
    "",
    "```text",
    report.generated_command,
    "```",
    "",
    `Source of truth: \`${report.authoritative_registry}\` (${summary.registered} registered rows; ${summary.active} active).`,
    `Registry digest: \`${report.registry_sha256}\``,
    "",
    "## Rule",
    "",
    report.actionability_rule.text,
    "",
    report.actionability_rule.tier_command_text,
    "",
    "The check treats retired and reserved rows as dormant: they remain in the exhaustive table, but only active rows can fail the actionability gate.",
    "",
    "## Counts",
    "",
    "| measure | count |",
    "| --- | ---: |",
    `| registered rows | ${summary.registered} |`,
    `| active rows | ${summary.active} |`,
    `| dormant rows | ${summary.dormant} |`,
    `| actionable active Fixes | ${summary.actionable} |`,
    `| non-actionable active Fixes | ${summary.non_actionable} |`,
    `| tier/toolchain command gaps | ${summary.tier_command_gaps} |`,
    `| active rows with violations | ${summary.violations} |`,
    `| malformed source rows | ${summary.malformed_rows} |`,
    "",
    `The known-failure baseline records ${summary.violations} current violations as a regression floor, not acceptance. \`--check\` fails when the violation count grows or a new code/requirement appears; repair rows may lower the count without adding baseline entries.`,
    "",
    "The JSON and TSV files are the exhaustive machine/diff views. Every row below is emitted from the registry; no diagnostic code list is copied into this report by hand.",
    "",
    "## Failures",
    "",
    "Each failure includes the registered code, the exact Fix text, and the requirement it misses. Fix text is shown verbatim.",
    "",
    "| code | line | Fix | missed requirement |",
    "| --- | ---: | --- | --- |",
  ];
  if (report.failures.length === 0) {
    lines.push("| (none) |  |  |  |");
  } else {
    for (const failure of report.failures) {
      for (const requirement of failure.requirements) {
        lines.push(`| \`${md(failure.code)}\` | ${failure.source_line} | ${md(failure.fix)} | ${md(requirement)} |`);
      }
    }
  }
  lines.push(
    "",
    "## Exhaustive row table",
    "",
    "| code | status | stage | fix kind | actionable | tier/toolchain | invoking command | violations | Fix |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  );
  for (const row of report.rows) {
    lines.push(`| \`${md(row.code)}\` | ${row.status} | ${md(row.stage)} | ${row.fix_kind} | ${row.actionable} | ${row.tier_or_toolchain} | ${md(row.invoking_commands.join(", "))} | ${md(row.violations.join("; "))} | ${md(row.fix)} |`);
  }
  lines.push("", "## Evidence boundary", "", "This census is the actionability check. UI snapshot coverage and diagnostic text repairs remain the diagnostics snapshot/repair work; the check reports those rows rather than inventing text or evidence.", "");
  return lines.join("\n");
}

function fixtureData() {
  const path = absPath(FIXTURES);
  if (!existsSync(path)) fail(`${FIXTURES} is missing`);
  let value;
  try {
    value = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${FIXTURES} is not valid JSON: ${error.message}`);
  }
  if (!value || value.schema !== "jet-diagnostic-actionability-fixtures-v1" || !Array.isArray(value.cases)) {
    fail(`${FIXTURES} has the wrong schema`);
  }
  return value;
}

function checkFixtures() {
  const fixture = fixtureData();
  const failures = [];
  for (const test of fixture.cases) {
    if (!test.id || typeof test.fix !== "string" || typeof test.actionable !== "boolean") {
      failures.push(`${test.id ?? "<unnamed>"}: fixture requires id, fix, and actionable`);
      continue;
    }
    const actual = classifyFix(test.fix, test.structured_fix ?? "");
    if (actual.actionable !== test.actionable) {
      failures.push(`${test.id}: expected actionable=${test.actionable}, got ${actual.actionable}`);
    }
    if (test.fix_kind && actual.kind !== test.fix_kind) {
      failures.push(`${test.id}: expected fix_kind=${test.fix_kind}, got ${actual.kind}`);
    }
    if (test.tier_or_toolchain !== undefined) {
      const fields = ["diagnostic", "FIXTURE", test.stage ?? "fixture", "error", "compile", "active", "meaning", test.what ?? "", test.why ?? "", test.fix, "false", test.structured_fix ?? "-"];
      const row = { fields };
      const context = tierContext(row);
      const commands = [...new Set([...mentionedCommands(test.what ?? ""), ...mentionedCommands(test.why ?? "")])];
      if (context !== test.tier_or_toolchain) failures.push(`${test.id}: expected tier_or_toolchain=${test.tier_or_toolchain}, got ${context}`);
      if (test.invoking_command !== undefined && commands.join(" | ") !== test.invoking_command) failures.push(`${test.id}: invoking command extraction drifted`);
    }
  }
  if (failures.length > 0) fail(`fixture failures:\n${failures.map((failure) => `  ${failure}`).join("\n")}`);
  return fixture.cases.length;
}

function readJson(path, label) {
  if (!existsSync(path)) fail(`${label} is missing: ${relPath(path)}`);
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    fail(`${label} is not valid JSON: ${relPath(path)}: ${error.message}`);
  }
}

function baselineFromReport(report) {
  return {
    schema: "jet-diagnostic-actionability-known-failures-v1",
    generated_by: "scripts/agent/diagnostic-actionability-census.mjs",
    source_of_truth: REGISTRY,
    rule_id: ACTIONABILITY_RULE.id,
    note: "Known debt is a regression floor, not an exemption from the actionability rule. A new code or new requirement fails --check; repair rows may leave this set.",
    allowed_failure_count: report.summary.violations,
    allowed_failures: report.failures.map((failure) => ({
      code: failure.code,
      requirements: failure.requirements,
    })),
  };
}

function checkBaseline(report) {
  const baseline = readJson(absPath(BASELINE), "known-failure baseline");
  if (baseline.schema !== "jet-diagnostic-actionability-known-failures-v1") fail(`${BASELINE} has the wrong schema`);
  if (baseline.rule_id !== ACTIONABILITY_RULE.id) fail(`${BASELINE} rule id does not match the actionability rule`);
  const allowed = new Map((baseline.allowed_failures ?? []).map((failure) => [failure.code, new Set(failure.requirements ?? [])]));
  const regressions = [];
  const allowedFailureCount = Number.isInteger(baseline.allowed_failure_count)
    ? baseline.allowed_failure_count
    : (baseline.allowed_failures ?? []).length;
  if (report.summary.violations > allowedFailureCount) {
    regressions.push(`violation count grew from ${allowedFailureCount} to ${report.summary.violations}`);
  }
  for (const failure of report.failures) {
    const known = allowed.get(failure.code);
    if (!known) {
      regressions.push(`${failure.code}: new actionability failure; Fix: ${failure.fix}; requirement: ${failure.requirements.join("; ")}`);
      continue;
    }
    for (const requirement of failure.requirements) {
      if (!known.has(requirement)) regressions.push(`${failure.code}: new requirement failure; Fix: ${failure.fix}; requirement: ${requirement}`);
    }
  }
  if (regressions.length > 0) {
    fail([
      "actionability regression(s) detected:",
      ...regressions.map((regression) => `  ${regression}`),
      `Write a concrete edit, runnable command, or path in each Fix; for tier/toolchain rows, also name the invoking command in What or Why.`,
      `If the current failures are intentional existing debt, update the report with ${GENERATED_COMMAND} only after review; do not add a new baseline entry without owner approval.`,
    ].join("\n"));
  }
  return baseline.allowed_failures.length;
}

function reportOutputs(report) {
  return {
    [REPORTS.json]: `${JSON.stringify(report, null, 2)}\n`,
    [REPORTS.tsv]: renderTsv(report),
    [REPORTS.markdown]: `${renderMarkdown(report)}\n`,
  };
}

function checkReports(report) {
  for (const [path, expected] of Object.entries(reportOutputs(report))) {
    const absolute = absPath(path);
    if (!existsSync(absolute) || readFileSync(absolute, "utf8") !== expected) {
      fail(`${path} is stale or missing; run ${GENERATED_COMMAND}`);
    }
  }
}

function human(report) {
  const { summary } = report;
  const lines = [
    "Diagnostic actionability census",
    `registry: ${report.authoritative_registry}`,
    `registered: ${summary.registered}`,
    `active: ${summary.active}`,
    `actionable: ${summary.actionable}`,
    `non-actionable Fixes: ${summary.non_actionable}`,
    `tier/toolchain command gaps: ${summary.tier_command_gaps}`,
    `active violations: ${summary.violations}`,
  ];
  if (report.failures.length > 0) {
    lines.push("failures:");
    for (const failure of report.failures) {
      for (const requirement of failure.requirements) lines.push(`  ${failure.code}: Fix: ${failure.fix}; requirement: ${requirement}`);
    }
  }
  if (report.malformed_rows.length > 0) {
    lines.push("malformed rows:");
    for (const error of report.malformed_rows) lines.push(`  line ${error.line}: ${error.reasons.join("; ")}`);
  }
  return `${lines.join("\n")}\n`;
}

function main(argv = process.argv.slice(2)) {
  try {
    const options = parseArgs(argv);
    if (options.help) {
      process.stdout.write(`${usage()}\n`);
      return 0;
    }
    const sourcePath = absPath(options.source);
    if (!existsSync(sourcePath)) fail(`missing registry source: ${options.source}`);
    const source = readFileSync(sourcePath, "utf8");
    const { report } = census(source, relPath(sourcePath));
    if (options.json) process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    else if (options.mode === "summary") process.stdout.write(human(report));

    checkFixtures();
    if (report.malformed_rows.length > 0) fail(`${report.malformed_rows.length} malformed registry row(s); fix the source row before regenerating the report`);
    if (options.mode === "write") {
      const outputs = reportOutputs(report);
      for (const path of Object.keys(outputs)) mkdirSync(dirname(absPath(path)), { recursive: true });
      if (options.refreshBaseline) mkdirSync(dirname(absPath(BASELINE)), { recursive: true });
      for (const [path, content] of Object.entries(outputs)) writeFileSync(absPath(path), content);
      if (options.refreshBaseline) writeFileSync(absPath(BASELINE), `${JSON.stringify(baselineFromReport(report), null, 2)}\n`);
      process.stdout.write(`WRITE OK registered=${report.summary.registered} active=${report.summary.active} actionable=${report.summary.actionable} non_actionable=${report.summary.non_actionable} tier_command_gaps=${report.summary.tier_command_gaps} violations=${report.summary.violations}\n`);
      return options.failOnViolations && report.summary.violations > 0 ? 1 : 0;
    }
    if (options.mode === "check") {
      checkReports(report);
      checkBaseline(report);
      process.stdout.write(`CHECK OK registered=${report.summary.registered} active=${report.summary.active} actionable=${report.summary.actionable} non_actionable=${report.summary.non_actionable} tier_command_gaps=${report.summary.tier_command_gaps} violations=${report.summary.violations}\n`);
      return options.failOnViolations && report.summary.violations > 0 ? 1 : 0;
    }
    return options.failOnViolations && report.summary.violations > 0 ? 1 : 0;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

export {
  ACTIONABILITY_RULE,
  BASELINE,
  REPORTS,
  REGISTRY,
  census,
  classifyFix,
  parseSource,
};

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
