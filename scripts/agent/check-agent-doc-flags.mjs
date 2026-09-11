#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SCRIPT_ROOT = join(ROOT, "scripts/agent");
const SKIP_DIRECTORIES = new Set([
  ".agent-worktrees",
  ".cache",
  ".claude",
  ".git",
  ".tmp",
  ".tower",
  "build",
  "node_modules",
  "result",
  "target",
]);
const PASSTHROUGH_SCRIPTS = new Set(["jet-env"]);
const DOC_SUFFIXES = new Set([".md", ".mdx", ".txt"]);

function isDoc(path) {
  return DOC_SUFFIXES.has(path.slice(path.lastIndexOf(".")))
    || ["AGENTS.md", "CLAUDE.md"].includes(path.split("/").pop());
}

function docFiles(base = ROOT) {
  const found = [];
  const seen = new Set();

  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
      if (entry.isSymbolicLink()) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIP_DIRECTORIES.has(entry.name)) visit(path);
      } else if (entry.isFile() && isDoc(path)) {
        const canonical = realpathSync(path);
        if (!seen.has(canonical)) {
          seen.add(canonical);
          found.push(canonical);
        }
      }
    }
  }

  visit(base);
  return found;
}

function logicalLines(text) {
  const physical = text.split(/\r?\n/);
  const lines = [];
  for (let index = 0; index < physical.length; index += 1) {
    const startLine = index + 1;
    let value = physical[index];
    while (value.trimEnd().endsWith("\\") && index + 1 < physical.length) {
      value = `${value.trimEnd().slice(0, -1)}\n${physical[++index]}`;
    }
    lines.push({ startLine, value });
  }
  return lines;
}

function lineAt(value, offset, startLine) {
  return startLine + (value.slice(0, offset).match(/\n/g) ?? []).length;
}

function documentedFlags(path, text) {
  const findings = [];
  const seen = new Set();

  function collect(line, script, scriptIndex) {
    if (PASSTHROUGH_SCRIPTS.has(script)) return;
    const scriptPath = join(SCRIPT_ROOT, script);
    if (!existsSync(scriptPath) || !statSync(scriptPath).isFile()) return;

    const command = line.value.slice(scriptIndex + script.length);
    for (const flag of command.matchAll(/--[A-Za-z][A-Za-z0-9-]*/g)) {
      const finding = {
        doc: path,
        line: lineAt(line.value, scriptIndex + script.length + flag.index, line.startLine),
        script,
        flag: flag[0],
        scriptPath,
      };
      const key = `${finding.doc}:${finding.line}:${finding.script}:${finding.flag}`;
      if (!seen.has(key)) {
        seen.add(key);
        findings.push(finding);
      }
    }
  }

  for (const line of logicalLines(text)) {
    for (const match of line.value.matchAll(/scripts\/agent\/([A-Za-z0-9][A-Za-z0-9_.-]*)/g)) {
      collect(line, match[1], match.index + match[0].length - match[1].length);
    }
    for (const match of line.value.matchAll(/(?:^|[\s`"'(])([A-Za-z0-9][A-Za-z0-9_.-]*\.(?:mjs|js|sh|py|rb|exs))(?=\s+--)/g)) {
      collect(line, match[1], match.index + match[0].lastIndexOf(match[1]));
    }
  }
  return findings;
}

function sourceFlags(scriptPath) {
  const source = readFileSync(scriptPath, "utf8");
  const flags = new Set();
  const isNode = /\.(?:m?js)$/.test(scriptPath);
  const isShell = scriptPath.endsWith(".sh") || source.startsWith("#!/usr/bin/env bash");

  if (isNode) {
    for (const rawLine of source.split(/\r?\n/)) {
      const line = rawLine.replace(/\/\/.*$/, "");
      if (!/\b(?:arg|args|argv|process\.argv)\b/.test(line)) continue;
      if (!/(?:===|!==|==|!=|includes\s*\(|indexOf\s*\(|has\s*\()/.test(line)) continue;
      for (const match of line.matchAll(/--[A-Za-z][A-Za-z0-9-]*/g)) flags.add(match[0]);
    }
  } else if (isShell) {
    for (const rawLine of source.split(/\r?\n/)) {
      const line = rawLine.replace(/(^|\s)#.*/, "$1");
      const parserLine = /\b(?:case|if|elif|while|until|getopts)\b|\[\[?|^\s*[-A-Za-z0-9|]+\)/.test(line);
      if (!parserLine) continue;
      for (const match of line.matchAll(/--[A-Za-z][A-Za-z0-9-]*/g)) flags.add(match[0]);
    }
  }
  return flags;
}

export function documentationPolicyErrors(path, text) {
  const findings = [];
  let fence = null;
  for (const [index, line] of text.split(/\r?\n/).entries()) {
    const marker = line.match(/^ {0,3}(`{3,}|~{3,})/);
    if (marker) {
      if (!fence) fence = marker[1];
      else if (marker[1][0] === fence[0] && marker[1].length >= fence.length && line.slice(marker[0].length).trim() === "") fence = null;
      continue;
    }
    if (fence) continue;
    const heading = line.match(/^ {0,3}#{1,6}\s+(.+?)(?:\s+#+)?\s*$/)?.[1];
    if (heading && /^(?:(?:\d+[.)]?\s+)?(?:implementation plans?|plan of record|derived plan|roadmap|milestones?|status(?: map| report)?|(?:current|shipped|implementation|delivery) status|progress(?: report)?|staged implementation|known (?:drift|gaps)|next steps|remaining work))\b/i.test(heading)) {
      findings.push(`${path}:${index + 1}: plans and development status belong in Tower`);
    }
    if (/^\s*[-*]\s+\[[ xX]\]/.test(line)) {
      findings.push(`${path}:${index + 1}: work checklists belong on Tower cards`);
    }
  }
  return findings;
}

function checkDocumentationPolicy() {
  const paths = [...docFiles(join(ROOT, "docs/spec")), ...docFiles(join(ROOT, "docs/proposals")), join(ROOT, "README.md")];
  const failures = paths.flatMap((path) => documentationPolicyErrors(relative(ROOT, path), readFileSync(path, "utf8")));
  for (const retired of [
    "docs/spec/roadmap.md", "docs/plans", "docs/spec/reference/errors",
    "docs/spec/diagnostic-rows.md", "docs/spec/reference/core-surface-ledger.md",
    "llms.text",
    "docs/spec/reference/core-backend-facts.md", "docs/spec/reference/feature-claims.md",
  ]) {
    if (existsSync(join(ROOT, retired))) failures.push(`${retired}: use Tower or generate the reference from code on demand`);
  }
  if (failures.length) {
    console.error(failures.join("\n"));
    process.exitCode = 1;
  } else {
    console.log(`documentation-policy: ok (${paths.length} guidance files; plans/status and retired mirrors checked)`);
  }
}

function main() {
  if (process.argv.includes("--policy-only")) {
    checkDocumentationPolicy();
    return;
  }
  const findings = docFiles().flatMap((path) => documentedFlags(path, readFileSync(path, "utf8")));
  const accepted = new Map();
  const errors = [];

  for (const finding of findings) {
    if (!accepted.has(finding.scriptPath)) accepted.set(finding.scriptPath, sourceFlags(finding.scriptPath));
    const flags = accepted.get(finding.scriptPath);
    if (!flags.has(finding.flag)) {
      const known = [...flags].sort().join(", ") || "(none detected)";
      errors.push(`${relative(ROOT, finding.doc)}:${finding.line}: ${finding.script} ${finding.flag} is not accepted; accepted flags: ${known}`);
    }
  }

  if (errors.length > 0) {
    console.error(`agent-doc-flags: ${errors.length} drift(s)`);
    for (const error of errors) console.error(`- ${error}`);
    process.exitCode = 1;
    return;
  }
  console.log(`agent-doc-flags: ok (${findings.length} documented flag reference(s))`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
