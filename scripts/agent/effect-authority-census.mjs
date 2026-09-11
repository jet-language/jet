#!/usr/bin/env node

import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const GENERATED_COMMAND = "node scripts/agent/effect-authority-census.mjs";
const DIAGNOSTICS_PATH = "crates/jet-codegen/src/Prelude/Diagnostics.jet";
const EFFECT_SOURCE_PATH = "crates/jet-codegen/src/Prelude/Effects.jet";
const CORE_CALLS_PATH = "crates/jet-foundation/src/Syntax/core_calls.rs";
const EFFECTS_PATH = "crates/jet-foundation/src/Effects.rs";
const FIXTURE_ROOT = "scripts/agent/effect-authority-census-fixtures";
const FIXTURE_MANIFEST_PATH = `${FIXTURE_ROOT}/manifest.json`;
const SOURCE_ROOTS = ["crates", "tests", "examples", "docs/spec", FIXTURE_ROOT];
const SOURCE_EXTENSIONS = new Set([".rs", ".jet", ".md"]);
const RUNTIME_TIMEOUT_MS = Number(process.env.EFFECT_CENSUS_TIMEOUT_MS ?? 30_000);
const SCRATCH_ROOT = process.env.JET_TEST_SCRATCH
  ?? join(homedir(), ".cache", "jet-test-scratch");
const LOCAL_JET_BINARY = process.env.JET_CENSUS_BIN
  ? resolve(ROOT, process.env.JET_CENSUS_BIN)
  : absPath("target/debug/jet");
const OUTPUTS = {
  json: "docs/audits/effect-authority-census-2026-09-03.json",
  tsv: "docs/audits/effect-authority-census-2026-09-03.tsv",
  md: "docs/audits/effect-authority-census-2026-09-03.md",
};
const TIERS = ["aot", "run", "interpret"];
const EVIDENCE_FIELDS = ["registration", "implementation", "fixtures", "projections"];


function fail(message) {
  throw new Error(`effect authority census: ${message}`);
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function absPath(path) {
  return resolve(ROOT, path);
}

function relPath(path) {
  return relative(ROOT, path).split("\\").join("/");
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function parseStringLiteral(value) {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

function maskRust(source) {
  const output = [...source];
  const blank = (index) => {
    if (source[index] !== "\n" && source[index] !== "\r") output[index] = " ";
  };
  const blankRange = (start, end) => {
    for (let index = start; index < end; index += 1) blank(index);
  };
  for (let index = 0; index < source.length; ) {
    if (source.startsWith("//", index)) {
      const start = index;
      index += 2;
      while (index < source.length && source[index] !== "\n") index += 1;
      blankRange(start, index);
      continue;
    }
    if (source.startsWith("/*", index)) {
      const start = index;
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source.startsWith("/*", index)) {
          depth += 1;
          index += 2;
        } else if (source.startsWith("*/", index)) {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }
      blankRange(start, index);
      continue;
    }
    const raw = source.slice(index).match(/^r(#+)?"/);
    if (raw) {
      const start = index;
      const hashes = raw[1] ?? "";
      index += raw[0].length;
      const close = `"${hashes}`;
      while (index < source.length && !source.startsWith(close, index)) index += 1;
      if (index < source.length) index += close.length;
      blankRange(start, index);
      continue;
    }
    if (source[index] === '"') {
      const start = index;
      index += 1;
      while (index < source.length) {
        if (source[index] === "\\") index += 2;
        else if (source[index] === '"') {
          index += 1;
          break;
        } else index += 1;
      }
      blankRange(start, index);
      continue;
    }
    index += 1;
  }
  return output.join("");
}

function matching(masked, open, opener, closer) {
  assert(masked[open] === opener, `expected ${opener} at offset ${open}`);
  let depth = 1;
  for (let index = open + 1; index < masked.length; index += 1) {
    if (masked[index] === opener) depth += 1;
    else if (masked[index] === closer) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  fail(`unclosed ${opener} at offset ${open}`);
}

function makeFile(path) {
  const source = readFileSync(absPath(path), "utf8");
  const masked = maskRust(source);
  const lines = source.split("\n");
  const starts = [0];
  for (let index = 0; index < source.length; index += 1) {
    if (source[index] === "\n") starts.push(index + 1);
  }
  const lineNumber = (offset) => {
    let low = 0;
    let high = starts.length;
    while (low + 1 < high) {
      const middle = Math.floor((low + high) / 2);
      if (starts[middle] <= offset) low = middle;
      else high = middle;
    }
    return low + 1;
  };
  const lineText = (line) => (lines[line - 1] ?? "").replace(/\r$/u, "");
  return { path, source, masked, lines, starts, lineNumber, lineText };
}

function walkFiles(root) {
  const absolute = absPath(root);
  assert(existsSync(absolute), `missing source root: ${root}`);
  const visit = (directory) => readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      if (entry.name === ".git" || entry.name === "target" || entry.name === "node_modules") return [];
      const child = resolve(directory, entry.name);
      if (entry.isDirectory()) return visit(child);
      if (!entry.isFile() || !SOURCE_EXTENSIONS.has(extname(entry.name))) return [];
      return [relPath(child)];
    });
  return visit(absolute);
}

function loadSources() {
  const paths = [...new Set(SOURCE_ROOTS.flatMap(walkFiles))].sort((a, b) => a.localeCompare(b));
  return paths.map((path) => makeFile(path));
}

function evidence(file, offset) {
  const line = file.lineNumber(offset);
  return { file: file.path, line, text: file.lineText(line) };
}

function uniqueEvidence(items) {
  const seen = new Set();
  return items
    .filter((item) => item && item.file && Number.isInteger(item.line))
    .filter((item) => {
      const key = `${item.file}:${item.line}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.text.localeCompare(b.text))
    .map(({ file, line, text }) => ({ file, line, text }));
}

function evidenceStatus(refs, reason = "no exact source evidence was found") {
  const normalized = uniqueEvidence(refs);
  return normalized.length > 0
    ? { status: "found", refs: normalized }
    : { status: "missing", reason };
}

function matcherWithoutGlobal(regex) {
  return new RegExp(regex.source, regex.flags.replace(/g/gu, ""));
}

function findRefs(files, regex, limit = 24) {
  const matcher = matcherWithoutGlobal(regex);
  const refs = [];
  for (const file of files) {
    for (let index = 0; index < file.lines.length; index += 1) {
      if (!matcher.test(file.lines[index])) continue;
      refs.push(evidence(file, file.starts[index]));
      if (refs.length >= limit) return uniqueEvidence(refs);
    }
  }
  return uniqueEvidence(refs);
}

function sourceFilesFor(files, includes = []) {
  if (includes.length === 0) return files;
  return files.filter((file) => includes.some((part) => file.path.includes(part)));
}

function parseRustStrings(text) {
  const strings = [];
  const pattern = /"(?:\\.|[^"\\])*"/gu;
  for (const match of text.matchAll(pattern)) strings.push({ value: parseStringLiteral(match[0].slice(1, -1)), offset: match.index });
  return strings;
}

function findTopLevelComma(masked, start, end) {
  let paren = 0;
  let bracket = 0;
  let brace = 0;
  for (let index = start; index < end; index += 1) {
    const ch = masked[index];
    if (ch === "(") paren += 1;
    else if (ch === ")") paren -= 1;
    else if (ch === "[") bracket += 1;
    else if (ch === "]") bracket -= 1;
    else if (ch === "{") brace += 1;
    else if (ch === "}") brace -= 1;
    else if (ch === "," && paren === 0 && bracket === 0 && brace === 0) return index;
  }
  return end;
}

function parseCoreRows(file) {
  const match = /\bpub\s+const\s+CORE_CALLS\b[\s\S]*?=\s*&\s*\[/u.exec(file.masked);
  assert(match, `cannot find CORE_CALLS in ${file.path}`);
  const open = file.masked.indexOf("[", match.index + match[0].lastIndexOf("["));
  const close = matching(file.masked, open, "[", "]");
  const body = file.masked.slice(open + 1, close);
  const constructorPattern = /CoreCallRecord::(new_with_coverage|new|receiver)\s*\(/gu;
  const rows = [];
  let constructor;
  while ((constructor = constructorPattern.exec(body))) {
    const callStart = open + 1 + constructor.index;
    const callOpen = file.masked.indexOf("(", callStart);
    const callClose = matching(file.masked, callOpen, "(", ")");
    const entryEnd = findTopLevelComma(file.masked, callClose + 1, close);
    const args = file.source.slice(callOpen + 1, callClose);
    const strings = parseRustStrings(args).map((item) => item.value);
    const kind = constructor[1] === "receiver" ? "receiver" : "plain";
    let module = "";
    let member = "";
    let receiverTypes = [];
    if (kind === "receiver") {
      const receiver = /^\s*&\s*\[([\s\S]*?)\]\s*,\s*"((?:\\.|[^"\\])*)"/u.exec(args);
      assert(receiver, `cannot parse receiver row at ${file.path}:${file.lineNumber(callStart)}`);
      receiverTypes = parseRustStrings(receiver[1]).map((item) => item.value);
      member = parseStringLiteral(receiver[2]);
    } else {
      assert(strings.length >= 3, `cannot parse CoreCallRecord at ${file.path}:${file.lineNumber(callStart)}`);
      [module, member] = strings;
    }
    assert(member.length > 0, `empty CoreCallRecord member at ${file.path}:${file.lineNumber(callStart)}`);
    if (kind === "plain") assert(module.length > 0, `empty CoreCallRecord module at ${file.path}:${file.lineNumber(callStart)}`);
    rows.push({
      kind,
      module,
      member,
      receiver_types: receiverTypes,
      declaration: evidence(file, callStart),
      entry: file.source.slice(callStart, entryEnd),
    });
    constructorPattern.lastIndex = entryEnd - (open + 1);
  }
  assert(rows.length > 0, "CORE_CALLS has no rows");
  return rows;
}

function extractFunction(file, name) {
  const match = new RegExp(`(?:pub\\s+)?(?:const\\s+)?fn\\s+${name}\\s*\\(`, "u").exec(file.masked);
  assert(match, `cannot find function ${name} in ${file.path}`);
  const open = file.masked.indexOf("{", match.index + match[0].length);
  const close = matching(file.masked, open, "{", "}");
  return { match, open, close, bodyStart: open + 1 };
}

function stripOuterParens(value) {
  let text = value.trim();
  while (text.startsWith("(") && text.endsWith(")")) {
    let depth = 0;
    let encloses = true;
    for (let index = 0; index < text.length; index += 1) {
      if (text[index] === "(") depth += 1;
      else if (text[index] === ")") depth -= 1;
      if (depth === 0 && index < text.length - 1) {
        encloses = false;
        break;
      }
    }
    if (!encloses) break;
    text = text.slice(1, -1).trim();
  }
  return text;
}

function splitConditionClauses(value) {
  const clauses = [];
  let start = 0;
  let depth = 0;
  for (let index = 0; index < value.length - 1; index += 1) {
    if (value[index] === "(") depth += 1;
    else if (value[index] === ")") depth -= 1;
    if (value[index] === "|" && value[index + 1] === "|" && depth <= 1) {
      clauses.push(value.slice(start, index));
      start = index + 2;
      index += 1;
    }
  }
  clauses.push(value.slice(start));
  return clauses.map(stripOuterParens).filter(Boolean);
}

function conditionCandidates(condition) {
  const modules = new Set();
  const types = new Set();
  const methods = new Set();
  const samePattern = /same_text\s*\(\s*(module|type_name|method)\s*,\s*"((?:\\.|[^"\\])*)"/gu;
  for (const match of condition.matchAll(samePattern)) {
    const value = parseStringLiteral(match[2]);
    if (match[1] === "module") modules.add(value);
    else if (match[1] === "type_name") types.add(value);
    else methods.add(value);
  }
  const oneOfPattern = /one_of\s*\(\s*method\s*,\s*&\s*\[([\s\S]*?)\]\s*,?\s*\)/gu;
  for (const match of condition.matchAll(oneOfPattern)) {
    for (const item of parseRustStrings(match[1])) methods.add(item.value);
  }
  return { modules: [...modules].sort(), types: [...types].sort(), methods: [...methods].sort() };
}

function parseLeafRules(file) {
  const rules = [];
  const unclassified = [];
  for (const descriptor of [
    { name: "effect_leaf_for", kind: "plain" },
    { name: "receiver_effect_leaf", kind: "receiver" },
  ]) {
    const range = extractFunction(file, descriptor.name);
    const body = file.masked.slice(range.bodyStart, range.close);
    const ifPattern = /\bif\b/gu;
    let ifMatch;
    while ((ifMatch = ifPattern.exec(body))) {
      const ifOffset = range.bodyStart + ifMatch.index;
      const open = file.masked.indexOf("{", ifOffset + ifMatch[0].length);
      if (open < 0 || open >= range.close) continue;
      const blockClose = matching(file.masked, open, "{", "}");
      if (blockClose > range.close) continue;
      const block = file.source.slice(open + 1, blockClose);
      const leafMatch = /Some\s*\(\s*"Time\.Wait"\s*\)/u.exec(block);
      if (!leafMatch) continue;
      const condition = file.source.slice(ifOffset + ifMatch[0].length, open);
      const refs = [
        evidence(file, ifOffset),
        evidence(file, open + 1 + leafMatch.index),
      ];
      let produced = 0;
      for (const clause of splitConditionClauses(condition)) {
        const candidates = conditionCandidates(clause);
        if (candidates.modules.length === 0 && candidates.types.length === 0) continue;
        const methods = candidates.methods.length > 0 ? candidates.methods : [null];
        const names = descriptor.kind === "plain" ? candidates.modules : candidates.types;
        for (const name of names) {
          for (const method of methods) {
            rules.push({
              kind: descriptor.kind,
              name,
              method,
              refs: uniqueEvidence(refs),
              function: descriptor.name,
            });
            produced += 1;
          }
        }
      }
      if (produced === 0) {
        unclassified.push({
          function: descriptor.name,
          source: evidence(file, ifOffset),
          reason: "Time.Wait branch has no module/type and method identity",
        });
      }
    }
  }
  return { rules, unclassified };
}

function diagnosticFamily(text) {
  if (/sandbox|wasm|native sandbox|no enforceable|sandboxed action/iu.test(text)) return "sandbox";
  if (/unsafe|sentry|low-level|raw pointer/iu.test(text)) return "unsafe";
  if (/authority|ungranted|grant|policy|harden|contain|memory denial/iu.test(text)) return "authority";
  return "effect";
}

function diagnosticLinks(family, code) {
  const authority = new Set(["authority:application-boundary", "authority:canonical-holds"]);
  const sandbox = new Set();
  if (family === "authority") {
    authority.add("authority:manifest-holds-allow");
    authority.add("authority:manifest-holds-deny");
    authority.add("authority:build-capabilities");
  }
  if (family === "unsafe") {
    authority.add("authority:unsafe-policy");
    authority.add("authority:package-guarantees");
    sandbox.add("sandbox:sentry-fenced-scope");
  }
  if (family === "sandbox" || /^E(?:1258|1259|1260|1275|3505)$/u.test(code)) {
    authority.add("authority:package-guarantees");
    authority.add("authority:sandbox-lending");
    sandbox.add("sandbox:substitute-or-refuse");
    sandbox.add("sandbox:build-native-boundary");
  }
  return { authority: [...authority].sort(), sandbox: [...sandbox].sort() };
}

function parseDiagnostics(file) {
  const rows = [];
  for (let index = 0; index < file.lines.length; index += 1) {
    const line = file.lines[index].replace(/\r$/u, "");
    if (!line.startsWith("diagnostic\t")) continue;
    const fields = line.split("\t");
    if (fields.length < 7 || !/^E\d{4}$/u.test(fields[1] ?? "")) continue;
    const code = fields[1];
    const text = fields.slice(1).join(" ");
    const relevant = /effect|authority|unsafe|sentry|sandbox|wasm|harden|ungranted|grant|no enforceable|low-level|memory denial|contain/iu.test(text);
    if (!relevant) continue;
    const registration = evidence(file, file.starts[index]);
    rows.push({
      id: `diagnostic:${code}`,
      kind: "diagnostic",
      code,
      family: diagnosticFamily(text),
      stage: fields[2] ?? "",
      severity: fields[3] ?? "",
      moment: fields[4] ?? "",
      status: fields[5] ?? "unknown",
      meaning: fields[6] ?? "",
      label: fields[6] ?? "",
      what: fields[7] ?? "",
      why: fields[8] ?? "",
      fix: fields[9] ?? "",
      detail: fields[10] ?? "",
      structured_fix: fields[11] ?? "",
      source: evidenceStatus([registration]),
      registration: registration,
    });
  }
  rows.sort((a, b) => a.code.localeCompare(b.code, undefined, { numeric: true }));
  assert(rows.length > 0, "no effect/authority/sandbox diagnostics found");
  return rows;
}

function makeEvidenceBundle(source, implementation, fixtures, projections) {
  return {
    registration: source,
    implementation,
    fixtures,
    projections,
  };
}

function withDiagnosticEvidence(row, files, implementationFiles, fixtureFiles, projectionFiles) {
  const codeRefs = (pool) => findRefs(pool, new RegExp(`\\b${escapeRegExp(row.code)}\\b`, "u"), 24);
  const implementation = evidenceStatus(
    codeRefs(implementationFiles.filter((file) => file.path !== DIAGNOSTICS_PATH)),
    `no implementation reference for ${row.code}`,
  );
  const fixtures = evidenceStatus(codeRefs(fixtureFiles), `no fixture reference for ${row.code}`);
  const projections = evidenceStatus(codeRefs(projectionFiles), `no projection reference for ${row.code}`);
  const links = diagnosticLinks(row.family, row.code);
  return {
    ...row,
    links,
    evidence: makeEvidenceBundle(row.source, implementation, fixtures, projections),
  };
}

const AUTHORITY_DEFINITIONS = [
  {
    id: "authority:effect-roots",
    label: "closed effect roots",
    includes: ["crates/jet-foundation/src/Authority.rs"],
    pattern: /EFFECT_SOURCE|EFFECT_ROOTS/iu,
    decision: "D-META-ONE1 / D-AUTHORITY-MODEL1",
  },
  {
    id: "authority:canonical-holds",
    label: "canonical Holds relation",
    includes: ["crates/jet-foundation/src/Authority.rs"],
    pattern: /type Holds|struct Authority|fn allows|fn tighten/iu,
    decision: "D-AUTHORITY-MODEL1",
  },
  {
    id: "authority:host-import-right",
    label: "host import required grant",
    includes: ["crates/jet-foundation/src/Authority.rs"],
    pattern: /HostImportFact|required_grant/iu,
    decision: "D-AUTHORITY-MODEL1",
  },
  {
    id: "authority:application-boundary",
    label: "application authority boundary",
    includes: ["crates/jet-foundation/src/Authority.rs"],
    pattern: /ApplicationAuthority|policy_diagnostic|denied_required_effects/iu,
    decision: "D-EFFECT-AUTHORITY1",
  },
  {
    id: "authority:manifest-holds-allow",
    label: "package authority allow holds",
    includes: ["crates/jet-pkg-model/src/Package/Blocks.rs"],
    pattern: /AUTHORITY_HOLDS_FIELD_ALLOW|authority\.holds\.allow|holds\.allow/iu,
    decision: "D-AUTHORITY-MANIFEST1",
  },
  {
    id: "authority:manifest-holds-deny",
    label: "package authority deny holds",
    includes: ["crates/jet-pkg-model/src/Package/Blocks.rs"],
    pattern: /AUTHORITY_HOLDS_FIELD_DENY|authority\.holds\.deny|holds\.deny/iu,
    decision: "D-AUTHORITY-MANIFEST1 / D-AUTHORITY-MEM1",
  },
  {
    id: "authority:package-guarantees",
    label: "package contain and harden guarantees",
    includes: ["crates/jet-foundation/src/AST/program_imports.rs", "crates/jet-pkg-model/src/Package/mod.rs"],
    pattern: /PackageGuarantees|memory_denials|pub harden|pub contain/iu,
    decision: "D-MEM-GUARANTEE1",
  },
  {
    id: "authority:memory-denials",
    label: "package memory denial projection",
    includes: ["crates/jet-foundation/src/AST/program_imports.rs", "crates/jet-pkg-model/src/Package/mod.rs"],
    pattern: /memory_denials|D-AUTHORITY-MEM1|Mem\.Alloc/iu,
    decision: "D-AUTHORITY-MEM1 / D-AUTHORITY-MEM2",
  },
  {
    id: "authority:build-capabilities",
    label: "typed build capability grants",
    includes: ["crates/jet-comptime/src/Comptime/Build/actions_policy.rs", "crates/jet-driver/src/Driver/mod.rs"],
    pattern: /BuildCapability|validate_build_authority|MissingGrant/iu,
    decision: "D-BUILDPOLICY1 / D-CTEFFECT1",
  },
  {
    id: "authority:unsafe-policy",
    label: "unsafe gate and harden policy",
    includes: ["crates/jet-driver/src/Driver/mod.rs", "crates/jet-codegen/src/Codegen/TIR/eval/stmts.rs"],
    pattern: /sentries_active|SentryPolicy|unsafe_paths|#Unsafe/iu,
    decision: "D-MEM-SENTRY1 / D-TEAMPOLICY1",
  },
  {
    id: "authority:sandbox-lending",
    label: "tighten-only sandbox lending",
    includes: ["crates/jet-pkg-model/src/Authority.rs"],
    pattern: /SandboxAuthority|fn lend|host\.tighten|UndeclaredRight/iu,
    decision: "D-AUTHORITY-MODEL1 / D-PLUGIN-AUTHORITY1",
  },
  {
    id: "authority:workspace-default",
    label: "workspace authority default",
    includes: ["crates/jet-codegen/src/Prelude/Core/Authority.rs"],
    pattern: /workspace|FS\.Read:repo|FS\.Write/iu,
    decision: "D-AUTHORITY-MODEL1",
  },
];

const SANDBOX_DEFINITIONS = [
  {
    id: "sandbox:target-zero-host-authority",
    label: "sandbox target has zero host authority",
    includes: ["crates/jet-driver/src/PluginExport.rs", "crates/jet-codegen/src/Codegen/Plugin.rs"],
    pattern: /zero host authority|target sandbox|E1260|guest/iu,
    decision: "D-PLUGIN1 / D-DEP-WASM1",
  },
  {
    id: "sandbox:build-native-boundary",
    label: "build actions cross a native child boundary",
    includes: ["crates/jet-comptime/src/Comptime/Build/execution_runtime.rs"],
    pattern: /run_native_sandboxed|native child|Bubblewrap|Seatbelt|AppContainer/iu,
    decision: "D-JPK-SANDBOX2",
  },
  {
    id: "sandbox:substitute-or-refuse",
    label: "substitute first, then refuse unsandboxed execution",
    includes: ["crates/jetpack/src/RuntimePolicy.rs", "crates/jet-driver/src/Driver/mod.rs"],
    pattern: /substitute-first|no unsandboxed fallback|refused after substitution|E1275/iu,
    decision: "D-JPK-SANDBOX2",
  },
  {
    id: "sandbox:network-deny",
    label: "network sharing is an explicit sandbox fact",
    includes: ["crates/jet-comptime/src/Comptime/Build/execution_runtime.rs", "crates/jet-process-sandbox"],
    pattern: /share_network|unshare.*net|network namespace|network.*share/iu,
    decision: "D-JPK-SANDBOX2",
  },
  {
    id: "sandbox:project-contained-outputs",
    label: "inputs and outputs stay inside declared project paths",
    includes: ["crates/jet-comptime/src/Comptime/Build/execution_runtime.rs", "crates/jet-driver/src/Driver/mod.rs"],
    pattern: /output_dir|source_dir|read-only|inside the project|contained/iu,
    decision: "D-JPK-SANDBOX2 / D-MEM-GUARANTEE1",
  },
  {
    id: "sandbox:authority-lend-tighten",
    label: "guest authority only tightens host rights",
    includes: ["crates/jet-pkg-model/src/Authority.rs"],
    pattern: /SandboxAuthority|tighten|MissingNeed|MissingRoot|NoFollow/iu,
    decision: "D-AUTHORITY-MODEL1",
  },
  {
    id: "sandbox:plugin-export-contract",
    label: "plugin export shape is checked before sandbox entry",
    includes: ["crates/jet-driver/src/PluginExport.rs", "crates/jet-driver/src/Driver/mod.rs"],
    pattern: /E1260|PluginExport|export shape|plugin target/iu,
    decision: "D-PLUGIN1 / D-DEP-WASM1",
  },
  {
    id: "sandbox:sentry-fenced-scope",
    label: "foreign and raw memory edges use shared sentry fences",
    includes: ["crates/jet-foundation/src/MemSentry.rs", "crates/jet-codegen/src/Codegen/TIR"],
    pattern: /jet_sentry_fenced_scope|jet_sentry_quarantine|R0801|foreign.*boundary/iu,
    decision: "D-MEM-SENTRY1",
  },
  {
    id: "sandbox:wasm-component-toolchain",
    label: "WASM plugin toolchain is sandboxed",
    includes: ["crates/jet-codegen/src/Codegen/Plugin.rs", "crates/jet-driver/src/PluginExport.rs"],
    pattern: /WASM|wasm|E1259|component/iu,
    decision: "D-DEP-WASM1",
  },
  {
    id: "sandbox:workspace-default",
    label: "workspace sandbox starts with narrow rights",
    includes: ["crates/jet-codegen/src/Prelude/Core/Authority.rs"],
    pattern: /FS\.Read:repo|FS\.Write|without|workspace/iu,
    decision: "D-AUTHORITY-MODEL1",
  },
];

function makeDefinitionRow(definition, files, implementationFiles, fixtureFiles, projectionFiles, kind) {
  const selected = sourceFilesFor(files, definition.includes);
  const registrationRefs = findRefs(selected, definition.pattern, 24);
  const implementationRefs = findRefs(implementationFiles, definition.pattern, 24);
  const fixtureRefs = findRefs(fixtureFiles, definition.pattern, 24);
  const projectionRefs = findRefs(projectionFiles, definition.pattern, 24);
  const source = evidenceStatus(registrationRefs, `no source anchor for ${definition.id}`);
  const implementation = evidenceStatus(implementationRefs, `no implementation anchor for ${definition.id}`);
  const fixtures = evidenceStatus(fixtureRefs, `no fixture anchor for ${definition.id}`);
  const projections = evidenceStatus(projectionRefs, `no projection anchor for ${definition.id}`);
  const links = kind === "authority"
    ? { authority: [definition.id], sandbox: definition.id === "authority:sandbox-lending" ? ["sandbox:authority-lend-tighten"] : [] }
    : {
        authority: definition.id === "sandbox:workspace-default"
          ? ["authority:workspace-default"]
          : definition.id === "sandbox:authority-lend-tighten"
            ? ["authority:sandbox-lending"]
            : ["authority:package-guarantees", "authority:application-boundary"],
        sandbox: [definition.id],
      };
  return {
    id: definition.id,
    kind: kind === "authority" ? "authority_hold" : "sandbox_fact",
    label: definition.label,
    decision: definition.decision,
    status: source.status,
    source,
    source_of_truth: definition.includes,
    links,
    evidence: makeEvidenceBundle(source, implementation, fixtures, projections),
  };
}

function addCoreRow(map, descriptor, rule, registryRows, implementationFiles, fixtureFiles, projectionFiles) {
  const id = descriptor.kind === "plain"
    ? `core-call:${descriptor.name}.${descriptor.method}`
    : `core-receiver:${descriptor.name}.${descriptor.method}`;
  const registryMatches = registryRows.filter((row) => {
    if (descriptor.kind === "plain") return row.kind === "plain" && row.module === descriptor.name && row.member === descriptor.method;
    return row.kind === "receiver" && row.member === descriptor.method && row.receiver_types.includes(descriptor.name);
  });
  const ruleRefs = rule?.refs ?? [];
  const registryRefs = registryMatches.map((row) => row.declaration);
  const registration = evidenceStatus(
    [...ruleRefs, ...registryRefs],
    registryMatches.length === 0
      ? `effect leaf rule has no matching CoreCallRecord for ${id}`
      : `effect leaf rule has no source evidence for ${id}`,
  );
  assert(typeof descriptor.method === "string", `missing effect leaf method for ${descriptor.kind}:${descriptor.name}; rule=${JSON.stringify(rule)}`);
  const implementationRegex = new RegExp(`\\b${escapeRegExp(descriptor.method)}\\b`, "u");
  const implementation = evidenceStatus(findRefs(implementationFiles, implementationRegex, 18), `no implementation reference for ${id}`);
  const fixtures = evidenceStatus(findRefs(fixtureFiles, implementationRegex, 18), `no fixture reference for ${id}`);
  const projections = evidenceStatus(findRefs(projectionFiles, implementationRegex, 18), `no projection reference for ${id}`);
  const links = {
    authority: ["authority:application-boundary", "authority:canonical-holds"],
    sandbox: /process|plugin/iu.test(descriptor.name) ? ["sandbox:substitute-or-refuse"] : [],
  };
  const previous = map.get(id);
  if (previous) {
    previous.source = evidenceStatus([...previous.source.refs ?? [], ...registration.refs ?? []], "duplicate effect leaf registration");
    previous.evidence.registration = previous.source;
    return;
  }
  map.set(id, {
    id,
    kind: descriptor.kind,
    label: `${descriptor.name}.${descriptor.method}`,
    leaf: "Time.Wait",
    status: registryMatches.length > 0 ? "found" : "missing",
    registry_status: registryMatches.length > 0 ? "found" : "missing",
    source: registration,
    links,
    evidence: makeEvidenceBundle(registration, implementation, fixtures, projections),
    registry_rows: registryMatches.map((row) => row.declaration),
  });
}

function buildCoreLeaves(coreFile, implementationFiles, fixtureFiles, projectionFiles) {
  const registryRows = parseCoreRows(coreFile);
  const ruleResult = parseLeafRules(coreFile);
  const rows = new Map();
  for (const rule of ruleResult.rules) {
    if (rule.kind === "plain") {
      const matches = rule.method === null
        ? registryRows.filter((row) => row.kind === "plain" && row.module === rule.name)
        : [{ kind: "plain", module: rule.name, member: rule.method }];
      if (matches.length === 0) {
        ruleResult.unclassified.push({
          function: rule.function,
          source: rule.refs[0],
          reason: `unrestricted effect leaf module ${rule.name} has no CoreCallRecord rows`,
        });
        continue;
      }
      for (const match of matches) {
        addCoreRow(rows, { kind: "plain", name: rule.name, method: match.member }, rule, registryRows, implementationFiles, fixtureFiles, projectionFiles);
      }
    } else {
      addCoreRow(rows, { kind: "receiver", name: rule.name, method: rule.method }, rule, registryRows, implementationFiles, fixtureFiles, projectionFiles);
    }
  }
  const output = [...rows.values()].sort((a, b) => a.id.localeCompare(b.id));
  assert(output.length > 0, "no Core effect leaves found");
  return { rows: output, unclassified: ruleResult.unclassified };
}

function findCodeSource(files, code) {
  return findRefs(files, new RegExp(`\\b${escapeRegExp(code)}\\b`, "u"), 12);
}

const ENFORCEMENT_DEFINITIONS = [
  {
    id: "enforcement:effect-fs-write-denial",
    label: "!FS.Write denial before host entry",
    category: "effect",
    fixture_id: "effect-fs-write-denial",
    pattern: /E0749|!FS\.Write|effect-fs-write|D-EFFECT-AUTHORITY1|denied_required_effects/iu,
    decision: "D-EFFECT-AUTHORITY1; an !FS.Write prohibition is carried into every tier",
    codes: ["E0749", "E0740", "E1803"],
    links: { authority: ["authority:application-boundary", "authority:canonical-holds"], sandbox: [] },
    cells: {
      aot: "refuse-before-host",
      run: "refuse-before-host",
      interpret: "refuse-before-host",
    },
    differences: { aot: "none", run: "none", interpret: "none" },
  },
  {
    id: "enforcement:effect-mem-alloc-denial",
    label: "!Mem.Alloc denial",
    category: "effect",
    fixture_id: "effect-mem-alloc-denial",
    pattern: /E0921|!Mem\.Alloc|effect-mem-alloc|D-AUTHORITY-MEM1/iu,
    decision: "D-AUTHORITY-MEM1; !Mem.Alloc is rejected before allocation",
    codes: ["E0921"],
    links: { authority: ["authority:memory-denials", "authority:canonical-holds"], sandbox: [] },
    cells: {
      aot: "refuse-before-allocation",
      run: "refuse-before-allocation",
      interpret: "refuse-before-allocation",
    },
    differences: { aot: "none", run: "none", interpret: "none" },
  },
  {
    id: "enforcement:memory-fact-denial",
    label: "transitive/package memory-fact denial",
    category: "effect",
    fixture_id: "memory-fact-denial",
    pattern: /E0921|memory-fact-denial|memory_denial_matches|D-MEM-FACTS1/iu,
    decision: "D-MEM-FACTS1; package memory facts reach every call-graph operation",
    codes: ["E0921"],
    links: { authority: ["authority:memory-denials", "authority:package-guarantees"], sandbox: [] },
    cells: {
      aot: "refuse-before-allocation",
      run: "refuse-before-allocation",
      interpret: "refuse-before-allocation",
    },
    differences: { aot: "none", run: "none", interpret: "none" },
  },
  {
    id: "enforcement:authority-holds",
    label: "authority holds and child-scope attenuation",
    category: "authority",
    fixture_id: "authority-holds",
    pattern: /fn tighten|SandboxAuthority|E0712|D-AUTHORITY-MODEL1|authority-hold/iu,
    decision: "D-AUTHORITY-MODEL1; one Holds relation is tightened at each boundary",
    codes: ["E0712", "E1803"],
    links: { authority: ["authority:canonical-holds", "authority:sandbox-lending"], sandbox: ["sandbox:authority-lend-tighten"] },
    cells: {
      aot: "refuse-widening-or-missing-hold",
      run: "refuse-widening-or-missing-hold",
      interpret: "refuse-widening-or-missing-hold",
    },
    differences: { aot: "none", run: "none", interpret: "none" },
  },
  {
    id: "enforcement:sentry-poisoning",
    label: "released storage quarantine and sentry fault",
    category: "sentry",
    fixture_id: "sentry-release",
    pattern: /jet_sentry_quarantine|jet_sentry_check|R080[123]|sentry-release|D-MEM-SENTRY1/iu,
    decision: "D-MEM-SENTRY1; dev/hardened sentries are intentional, not an authority bypass",
    codes: ["R0801", "R0802", "R0803"],
    links: { authority: ["authority:unsafe-policy", "authority:package-guarantees"], sandbox: ["sandbox:sentry-fenced-scope"] },
    cells: {
      aot: "release-silent-or-hardened-check",
      run: "dev-or-hardened-runtime-check",
      interpret: "dev-or-hardened-runtime-check",
    },
    differences: {
      aot: "release AOT may be unwatched unless package/profile harden enables sentries",
      run: "shared sentry kernel is active in the dev tier or when harden is enabled",
      interpret: "shared sentry kernel is active in the dev tier or when harden is enabled",
    },
  },
  {
    id: "enforcement:harden-fence",
    label: "package/profile harden keeps foreign fences active",
    category: "harden",
    fixture_id: "harden-fence",
    pattern: /package_hardened|sentries_active|harden-fence|D-HARDENED1|D-MEM-GUARANTEE1/iu,
    decision: "D-MEM-GUARANTEE1; harden is a monotone package/profile fact",
    codes: ["R0801", "R0802", "R0803"],
    links: { authority: ["authority:package-guarantees", "authority:unsafe-policy"], sandbox: ["sandbox:sentry-fenced-scope"] },
    cells: {
      aot: "hardened-runtime-check",
      run: "hardened-runtime-check",
      interpret: "hardened-runtime-check",
    },
    differences: { aot: "none", run: "none", interpret: "none" },
  },
  {
    id: "enforcement:process-sandbox",
    label: "executable actions require a native boundary",
    category: "sandbox",
    fixture_id: "process-sandbox",
    pattern: /run_native_sandboxed|no unsandboxed fallback|E1275|E1803|SandboxUnavailable|process-sandbox|D-JPK-SANDBOX2/iu,
    decision: "D-JPK-SANDBOX2; substitute/remote resolution precedes refusal, never an unsandboxed launch",
    codes: ["E1275", "E1803", "E3504", "E3505"],
    links: { authority: ["authority:build-capabilities", "authority:package-guarantees"], sandbox: ["sandbox:build-native-boundary", "sandbox:substitute-or-refuse"] },
    cells: {
      aot: "native-boundary-or-refuse",
      run: "native-boundary-or-refuse",
      interpret: "refuse-process-action-without-host-launch",
    },
    differences: {
      aot: "native build/action boundary or structured refusal",
      run: "native build/action boundary or structured refusal",
      interpret: "interpreter refuses process/OS actions instead of launching; refusal is not a bypass",
    },
  },
];
const ENFORCEMENT_SOURCES = {
  "enforcement:effect-fs-write-denial": [
    DIAGNOSTICS_PATH,
    "crates/jet-sema/src/Sema/Effects.rs",
    "crates/jet-codegen/src/Codegen/TIR/mod.rs",
  ],
  "enforcement:effect-mem-alloc-denial": [
    DIAGNOSTICS_PATH,
    "crates/jet-sema/src/Sema/MemoryFacts.rs",
    "tests/memory_semantics.rs",
  ],
  "enforcement:memory-fact-denial": [
    DIAGNOSTICS_PATH,
    "crates/jet-sema/src/Sema/MemoryFacts.rs",
    "tests/memory_semantics.rs",
  ],
  "enforcement:authority-holds": [
    "crates/jet-foundation/src/Authority.rs",
    "crates/jet-sema/src/Sema/Effects.rs",
    "crates/jet-sema/src/Sema/CheckerCore/statements.rs",
  ],
  "enforcement:sentry-poisoning": [
    "crates/jet-foundation/src/MemSentry.rs",
    "crates/jet-driver/src/Driver/mod.rs",
    "crates/jet-jit/src/ambient_interp.rs",
  ],
  "enforcement:harden-fence": [
    "crates/jet-driver/src/Driver/mod.rs",
    "crates/jet-foundation/src/MemSentry.rs",
  ],
  "enforcement:process-sandbox": [
    "crates/jet-comptime/src/Comptime/Build/execution_runtime.rs",
    "crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs",
  ],
};

function loadFixtureManifest() {
  const manifest = JSON.parse(readFileSync(absPath(FIXTURE_MANIFEST_PATH), "utf8"));
  assert(manifest.schema_version === 1, "effect census fixture manifest schema must be 1");
  assert(manifest.fixture_root === FIXTURE_ROOT, "fixture manifest root disagrees with script");
  assert(manifest.denominator && typeof manifest.denominator.statement === "string", "fixture manifest has no denominator statement");
  assert(Array.isArray(manifest.denominator.rules) && manifest.denominator.rules.length === ENFORCEMENT_DEFINITIONS.length, "fixture denominator must name every enforcement point exactly once");
  const denominatorIds = new Set();
  for (const rule of manifest.denominator.rules) {
    assert(typeof rule.id === "string" && typeof rule.enforcement_point === "string" && typeof rule.fixture === "string" && typeof rule.decision === "string", "fixture denominator rule is incomplete");
    assert(!denominatorIds.has(rule.id), `duplicate denominator rule ${rule.id}`);
    denominatorIds.add(rule.id);
    const definition = ENFORCEMENT_DEFINITIONS.find((candidate) => candidate.id === rule.enforcement_point);
    assert(definition && definition.fixture_id === rule.fixture, `denominator rule ${rule.id} does not match its enforcement definition`);
  }
  assert(Array.isArray(manifest.fixtures) && manifest.fixtures.length > 0, "effect census fixture manifest has no fixtures");
  const ids = new Set();
  const fixtures = manifest.fixtures.map((fixture) => {
    assert(typeof fixture.id === "string" && fixture.id.length > 0, "fixture has no id");
    assert(!ids.has(fixture.id), `duplicate fixture id ${fixture.id}`);
    ids.add(fixture.id);
    assert(ENFORCEMENT_DEFINITIONS.some((definition) => definition.id === fixture.enforcement_point), `fixture ${fixture.id} names an unknown enforcement point`);
    for (const name of ["source", "package"]) {
      if (fixture[name] === null) continue;
      assert(typeof fixture[name] === "string" && !fixture[name].includes(".."), `fixture ${fixture.id} has an unsafe ${name} path`);
      assert(existsSync(absPath(`${FIXTURE_ROOT}/${fixture[name]}`)), `fixture ${fixture.id} is missing ${name} file`);
    }
    assert(existsSync(absPath(`${FIXTURE_ROOT}/${fixture.source}`)), `fixture ${fixture.id} source file is missing`);
    assert(fixture.pair && typeof fixture.pair === "object", `fixture ${fixture.id} has no allow/refuse pair`);
    for (const pairSide of ["allow_source", "refuse_source"]) {
      const pairPath = fixture.pair[pairSide];
      assert(typeof pairPath === "string" && !pairPath.includes(".."), `fixture ${fixture.id} has an unsafe ${pairSide}`);
      assert(existsSync(absPath(`${FIXTURE_ROOT}/${pairPath}`)), `fixture ${fixture.id} is missing ${pairSide}`);
    }
    assert(fixture.pair.allow_expected === "allow" && fixture.pair.refuse_expected === "refuse", `fixture ${fixture.id} pair must be allow/refuse`);
    return fixture;
  }).sort((a, b) => a.id.localeCompare(b.id));
  for (const definition of ENFORCEMENT_DEFINITIONS) {
    const matches = fixtures.filter((fixture) => fixture.enforcement_point === definition.id);
    assert(matches.length === 1, `${definition.id} must have exactly one fixture, found ${matches.length}`);
    assert(matches[0].id === definition.fixture_id, `${definition.id} fixture id disagrees with its definition`);
  }
  return { denominator: manifest.denominator, fixtures };
}

function parseEffectDeclarations(effectFile) {
  const declarations = [];
  for (const [index, line] of effectFile.source.split(/\r?\n/u).entries()) {
    const match = /^\s*effect\s+([A-Z][A-Za-z0-9_]*(?:\.[A-Z][A-Za-z0-9_]*)?)(?:\s+(@[A-Za-z0-9_]+))?/u.exec(line);
    if (!match) continue;
    const name = match[1];
    declarations.push({
      name,
      root: name.split(".")[0],
      modifier: match[2] ?? null,
      source: { file: EFFECT_SOURCE_PATH, line: index + 1, text: line },
    });
  }
  assert(declarations.length > 0, `no effect declarations found in ${EFFECT_SOURCE_PATH}`);
  return declarations;
}

function fixtureExpected(fixture, tier) {
  return fixture.expected.tiers?.[tier] ?? fixture.expected.default ?? null;
}

function fixturePath(fixture, name) {
  return absPath(`${FIXTURE_ROOT}/${fixture[name]}`);
}

function copyFixture(fixture, tier, sourceName = fixture.source) {
  mkdirSync(SCRATCH_ROOT, { recursive: true });
  const runRoot = mkdtempSync(join(SCRATCH_ROOT, `effect-authority-${fixture.id}-${tier}-`));
  copyFileSync(absPath(`${FIXTURE_ROOT}/${sourceName}`), join(runRoot, "main.jet"));
  if (fixture.package) copyFileSync(fixturePath(fixture, "package"), join(runRoot, "package.jet"));
  return runRoot;
}

function stripAnsi(value) {
  return String(value ?? "").replace(/\u001b\[[0-?]*[ -/]*[@-~]/gu, "");
}

function stableOutput(value, runRoot) {
  return stripAnsi(value)
    .replaceAll(runRoot, "<fixture>")
    .replaceAll(ROOT, "<repo>")
    .replaceAll(SCRATCH_ROOT, "<scratch>")
    .replace(/\b(?:\/[^ \n:]+)+\/main\.jet\b/gu, "<fixture>/main.jet")
    .replace(/^\s+|\s+$/gmu, "")
    .replace(/[ \t]+/gu, " ")
    .replace(/\r/gu, "");
}

function diagnosticCodes(output) {
  return [...new Set((output.match(/\b[ER]\d{4}\b/gu) ?? []))].sort();
}

function diagnosticMeaning(output, codes, runRoot) {
  const lines = stableOutput(output, runRoot)
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const selected = lines.filter((line) => (
    codes.some((code) => line.includes(code))
      || /^(?:error|runtime fault|what:|why:|fix:|diagnostic:|refused|unsandboxed|sandbox)/iu.test(line)
  ));
  const meaningful = selected.length > 0 ? selected : lines.slice(0, 3);
  return meaningful.join(" | ").replace(/\s+/gu, " ").slice(0, 1200);
}

function commandDisplay(args) {
  return args.map((arg) => {
    if (arg === ROOT) return "<repo>";
    if (arg === LOCAL_JET_BINARY) return "<jet>";
    if (arg === SCRATCH_ROOT) return "<scratch>";
    return arg;
  }).join(" ");
}

function invokeJet(args, cwd) {
  const command = process.env.JET_CENSUS_BIN
    ? [LOCAL_JET_BINARY, ...args]
    : ["scripts/agent/jet-env", "jet", ...args];
  const executable = command[0].startsWith("/") ? command[0] : absPath(command[0]);
  const result = spawnSync(executable, command.slice(1), {
    cwd,
    encoding: "utf8",
    env: {
      ...process.env,
      NO_COLOR: "1",
      CLICOLOR: "0",
    },
    timeout: RUNTIME_TIMEOUT_MS,
    maxBuffer: 2 * 1024 * 1024,
  });
  if (result.error) {
    return {
      status: result.error.code === "ETIMEDOUT" ? "timeout" : "unavailable",
      reason: result.error.code === "ETIMEDOUT"
        ? "tier command timed out before producing a verdict"
        : `tier command could not start (${result.error.code ?? "unknown spawn error"})`,
      command: commandDisplay(command),
    };
  }
  if (result.signal) {
    return {
      status: "unavailable",
      reason: `tier command terminated by ${result.signal} before producing a verdict`,
      command: commandDisplay(command),
    };
  }
  return {
    status: "exited",
    exit_code: result.status,
    stdout: String(result.stdout ?? ""),
    stderr: String(result.stderr ?? ""),
    command: commandDisplay(command),
  };
}

function unavailableObservation(command, reason) {
  return {
    status: "unavailable",
    decision: null,
    diagnostic_codes: [],
    meaning: null,
    meaning_key: null,
    command,
    reason,
  };
}

function observeInvocation(result, phase, runRoot) {
  if (result.status !== "exited") return unavailableObservation(result.command, result.reason);
  const output = `${result.stdout}\n${result.stderr}`;
  const codes = diagnosticCodes(stableOutput(output, runRoot));
  if (result.exit_code === 0) {
    return {
      status: "observed",
      phase,
      decision: "allow",
      diagnostic_codes: codes,
      meaning: "allowed",
      meaning_key: "allow",
      command: result.command,
      exit_code: result.exit_code,
    };
  }
  if (codes.length === 0) {
    return unavailableObservation(result.command, "tier exited without a registered diagnostic code");
  }
  const meaning = diagnosticMeaning(output, codes, runRoot);
  return {
    status: "observed",
    phase,
    decision: "refuse",
    diagnostic_codes: codes,
    meaning,
    meaning_key: `${codes.join(",")}|${meaning}`,
    command: result.command,
    exit_code: result.exit_code,
  };
}

function runFixtureTier(fixture, tier, sourceName = fixture.source) {
  if (!existsSync(LOCAL_JET_BINARY)) {
    return unavailableObservation(
      `scripts/agent/jet-env jet ${tier} <fixture>`,
      `local compiler ${relPath(LOCAL_JET_BINARY)} is unavailable; runtime differential is fail-closed`,
    );
  }
  const runRoot = copyFixture(fixture, tier, sourceName);
  const entry = fixture.package ? "." : "main.jet";
  try {
    if (tier === "aot") {
      const profile = fixture.aot_profile ?? "dev";
      const build = invokeJet(["build", `--profile=${profile}`, entry], runRoot);
      if (build.status !== "exited") return unavailableObservation(build.command, build.reason);
      if (build.exit_code !== 0) return observeInvocation(build, "build", runRoot);
      const artifact = ["build/run", "build/main"]
        .map((path) => join(runRoot, path))
        .find((path) => existsSync(path));
      if (!artifact) {
        return unavailableObservation(
          build.command,
          "AOT build exited successfully but produced no build/run or build/main artifact",
        );
      }
      const execute = spawnSync(artifact, [], {
        cwd: runRoot,
        encoding: "utf8",
        env: { ...process.env, NO_COLOR: "1", CLICOLOR: "0" },
        timeout: RUNTIME_TIMEOUT_MS,
        maxBuffer: 2 * 1024 * 1024,
      });
      const artifactName = relative(runRoot, artifact);
      const artifactCommand = `${build.command} && <fixture>/${artifactName}`;
      if (execute.error) {
        return unavailableObservation(
          artifactCommand,
          execute.error.code === "ETIMEDOUT"
            ? "AOT artifact timed out before producing a verdict"
            : `AOT artifact could not start (${execute.error.code ?? "unknown spawn error"})`,
        );
      }
      return observeInvocation({
        status: "exited",
        exit_code: execute.status,
        stdout: execute.stdout,
        stderr: execute.stderr,
        command: artifactCommand,
      }, "execute", runRoot);
    }
    const args = tier === "interpret"
      ? ["run", "--interpret", entry]
      : ["run", entry];
    return observeInvocation(invokeJet(args, runRoot), "execute", runRoot);
  } finally {
    rmSync(runRoot, { recursive: true, force: true });
  }
}

function expectedMismatch(fixture, tier, observation) {
  const expected = fixtureExpected(fixture, tier);
  if (!expected || observation.status !== "observed") return null;
  if (observation.decision !== expected.decision) {
    return {
      fixture: fixture.id,
      tier,
      kind: "decision",
      expected: expected.decision,
      observed: observation.decision,
    };
  }
  if (expected.diagnostic_codes.length > 0 && !observation.diagnostic_codes.some((code) => expected.diagnostic_codes.includes(code))) {
    return {
      fixture: fixture.id,
      tier,
      kind: "diagnostic_code",
      expected: expected.diagnostic_codes,
      observed: observation.diagnostic_codes,
    };
  }
  return null;
}

function intendedDifferenceApplies(fixture, observations) {
  const intent = fixture.intended_difference;
  if (!intent) return false;
  const observedTiers = intent.tiers.every((tier) => observations[tier]?.status === "observed");
  if (!observedTiers) return false;
  return true;
}

function compareFixture(fixture, observations) {
  const unavailable = TIERS.filter((tier) => observations[tier].status !== "observed");
  const contract_mismatches = TIERS
    .map((tier) => expectedMismatch(fixture, tier, observations[tier]))
    .filter(Boolean);
  if (unavailable.length > 0) {
    return {
      status: "unavailable",
      fail_closed: true,
      unavailable_tiers: unavailable,
      contract_mismatches,
      divergences: [],
      card_required: false,
    };
  }
  const decisions = TIERS.map((tier) => observations[tier].decision);
  const meanings = TIERS.map((tier) => observations[tier].meaning_key);
  const sameDecision = decisions.every((decision) => decision === decisions[0]);
  const sameMeaning = meanings.every((meaning) => meaning === meanings[0]);
  if (sameDecision && sameMeaning) {
    return {
      status: "same_decision_same_meaning",
      fail_closed: contract_mismatches.length > 0,
      unavailable_tiers: [],
      contract_mismatches,
      divergences: [],
      card_required: false,
    };
  }
  const intended = intendedDifferenceApplies(fixture, observations);
  const status = intended
    ? "intended_difference"
    : sameDecision
      ? "same_decision_different_meaning"
      : "different_decision";
  const divergences = [];
  for (let left = 0; left < TIERS.length; left += 1) {
    for (let right = left + 1; right < TIERS.length; right += 1) {
      const a = observations[TIERS[left]];
      const b = observations[TIERS[right]];
      if (a.decision === b.decision && a.meaning_key === b.meaning_key) continue;
      divergences.push({
        fixture: fixture.id,
        enforcement_point: fixture.enforcement_point,
        programs: {
          allow: `${FIXTURE_ROOT}/${fixture.pair.allow_source}`,
          refuse: `${FIXTURE_ROOT}/${fixture.pair.refuse_source}`,
          package: fixture.package ? `${FIXTURE_ROOT}/${fixture.package}` : null,
        },
        tiers: [TIERS[left], TIERS[right]],
        kind: a.decision === b.decision ? "same_decision_different_meaning" : "different_decision",
        wrong_tier: a.decision === b.decision
          ? `diagnostic/meaning mismatch: ${TIERS[left]} vs ${TIERS[right]}`
          : `${TIERS[left]}=${a.decision}; ${TIERS[right]}=${b.decision}`,
        left: {
          tier: TIERS[left],
          decision: a.decision,
          diagnostic_codes: a.diagnostic_codes,
          meaning: a.meaning,
        },
        right: {
          tier: TIERS[right],
          decision: b.decision,
          diagnostic_codes: b.diagnostic_codes,
          meaning: b.meaning,
        },
        intended: intended,
        decision_id: intended ? fixture.intended_difference.decision_id : null,
        card: intended ? null : fixture.card ?? null,
      });
    }
  }
  return {
    status,
    fail_closed: !intended || contract_mismatches.length > 0,
    unavailable_tiers: [],
    contract_mismatches,
    divergences,
    card_required: !intended && divergences.length > 0,
  };
}

function comparePair(fixture, allowObservations, refuseObservations) {
  const allowFixture = {
    ...fixture,
    expected: {
      default: {
        decision: fixture.pair.allow_expected,
        diagnostic_codes: [],
        meaning: "allow pair",
      },
    },
    intended_difference: null,
  };
  const allowComparison = compareFixture(allowFixture, allowObservations);
  const refuseComparison = compareFixture(fixture, refuseObservations);
  const pairFailures = [];
  if (allowComparison.status !== "unavailable" && allowComparison.status !== "same_decision_same_meaning") {
    pairFailures.push({ case: "allow", status: allowComparison.status });
  }
  if (refuseComparison.status !== "unavailable" && refuseComparison.status !== "same_decision_same_meaning" && refuseComparison.status !== "intended_difference") {
    pairFailures.push({ case: "refuse", status: refuseComparison.status });
  }
  const unavailable = [
    ...allowComparison.unavailable_tiers.map((tier) => ({ case: "allow", tier, reason: allowObservations[tier].reason })),
    ...refuseComparison.unavailable_tiers.map((tier) => ({ case: "refuse", tier, reason: refuseObservations[tier].reason })),
  ];
  return {
    allow_observations: allowObservations,
    refuse_observations: refuseObservations,
    allow_comparison: allowComparison,
    refuse_comparison: refuseComparison,
    status: unavailable.length > 0 ? "unavailable" : pairFailures.length > 0 ? "fail" : "allow_refuse",
    unavailable,
    failures: pairFailures,
  };
}

function buildRuntimeDifferential(fixtures) {
  const fixtureRows = fixtures.map((fixture) => {
    const refuseObservations = Object.fromEntries(TIERS.map((tier) => [tier, runFixtureTier(fixture, tier, fixture.pair.refuse_source)]));
    const allowObservations = Object.fromEntries(TIERS.map((tier) => [tier, runFixtureTier(fixture, tier, fixture.pair.allow_source)]));
    const comparison = compareFixture(fixture, refuseObservations);
    const pair = comparePair(fixture, allowObservations, refuseObservations);
    return {
      id: fixture.id,
      enforcement_point: fixture.enforcement_point,
      source: `${FIXTURE_ROOT}/${fixture.source}`,
      package: fixture.package ? `${FIXTURE_ROOT}/${fixture.package}` : null,
      programs: {
        allow: `${FIXTURE_ROOT}/${fixture.pair.allow_source}`,
        refuse: `${FIXTURE_ROOT}/${fixture.pair.refuse_source}`,
      },
      expected: Object.fromEntries(TIERS.map((tier) => [tier, fixtureExpected(fixture, tier)])),
      observations: refuseObservations,
      comparison,
      pair,
      intended_difference: fixture.intended_difference ?? null,
    };
  });
  const divergences = fixtureRows.flatMap((row) => [
    ...row.comparison.divergences,
    ...row.pair.allow_comparison.divergences,
  ]);
  const untracked = divergences.filter((divergence) => !divergence.intended && !divergence.card);
  const unavailable = fixtureRows.flatMap((row) => [
    ...row.comparison.unavailable_tiers.map((tier) => ({
      fixture: row.id,
      enforcement_point: row.enforcement_point,
      case: "refuse",
      program: row.programs.refuse,
      tier,
      reason: row.observations[tier].reason,
    })),
    ...row.pair.allow_comparison.unavailable_tiers.map((tier) => ({
      fixture: row.id,
      enforcement_point: row.enforcement_point,
      case: "allow",
      program: row.programs.allow,
      tier,
      reason: row.pair.allow_observations[tier].reason,
    })),
  ]);
  const contractMismatches = fixtureRows.flatMap((row) => row.comparison.contract_mismatches);
  const pairFailures = fixtureRows.flatMap((row) => row.pair.failures.map((failure) => ({
    fixture: row.id,
    kind: `pair_${failure.case}_${failure.status}`,
  })));
  const failures = [
    ...fixtureRows
      .filter((row) => row.comparison.status === "same_decision_different_meaning" || row.comparison.status === "different_decision")
      .map((row) => ({ fixture: row.id, kind: row.comparison.status })),
    ...contractMismatches.map((mismatch) => ({ ...mismatch, kind: `contract_${mismatch.kind}` })),
    ...pairFailures,
    ...untracked.map((divergence) => ({ fixture: divergence.fixture, kind: "untracked_difference", tiers: divergence.tiers })),
  ];
  return {
    schema_version: 1,
    required_tiers: TIERS,
    status: unavailable.length > 0 ? "fail_closed_unavailable" : failures.length > 0 ? "fail" : "pass",
    fail_closed: unavailable.length > 0,
    local_compiler: existsSync(LOCAL_JET_BINARY) ? relPath(LOCAL_JET_BINARY) : null,
    fixtures: fixtureRows,
    divergences,
    unavailable,
    failures,
    untracked_differences: untracked,
    gate: {
      name: "effect-authority-three-tier-differential",
      status: unavailable.length > 0 || failures.length > 0 ? "fail" : "pass",
      future_tier_changes_must_update: ["fixture manifest", "allow/refuse pair", "expected tier contract", "decision-cited intended differences"],
    },
  };
}

function buildEnforcementPoints(files, implementationFiles, fixtureFiles, projectionFiles, diagnostics, differential) {
  const diagnosticSet = new Set(diagnostics.map((row) => row.code));
  return ENFORCEMENT_DEFINITIONS.map((definition) => {
    const refs = findRefs(files, definition.pattern, 30);
    const source = evidenceStatus(refs, `no enforcement source anchor for ${definition.id}`);
    const implementation = evidenceStatus(findRefs(implementationFiles, definition.pattern, 30), `no implementation anchor for ${definition.id}`);
    const fixtures = evidenceStatus(findRefs(fixtureFiles, definition.pattern, 30), `no fixture anchor for ${definition.id}`);
    const projections = evidenceStatus(findRefs(projectionFiles, definition.pattern, 30), `no projection anchor for ${definition.id}`);
    const codes = definition.codes.filter((code) => diagnosticSet.has(code) || findCodeSource(files, code).length > 0);
    const runtime = differential.fixtures.find((fixture) => fixture.enforcement_point === definition.id) ?? null;
    const tiers = {};
    for (const tier of TIERS) {
      tiers[tier] = {
        outcome: definition.cells[tier],
        diagnostic_codes: codes,
        intended_difference: definition.differences[tier],
        runtime: runtime?.observations[tier] ?? null,
        allow_runtime: runtime?.pair.allow_observations[tier] ?? null,
        refuse_runtime: runtime?.pair.refuse_observations[tier] ?? null,
        evidence: makeEvidenceBundle(source, implementation, fixtures, projections),
      };
    }
    return {
      id: definition.id,
      kind: "enforcement_point",
      category: definition.category,
      fixture_id: definition.fixture_id,
      label: definition.label,
      status: source.status,
      decision: definition.decision,
      diagnostic_codes: codes,
      links: definition.links,
      source,
      evidence: makeEvidenceBundle(source, implementation, fixtures, projections),
      tiers,
      runtime_comparison: runtime?.comparison ?? null,
      pair_status: runtime?.pair.status ?? "missing",
      source_of_truth: ENFORCEMENT_SOURCES[definition.id] ?? [],
    };
  });
}

function buildCrossLinks(diagnostics, authorityRows, sandboxRows) {
  const authorityById = new Map(authorityRows.map((row) => [row.id, row]));
  const sandboxById = new Map(sandboxRows.map((row) => [row.id, row]));
  const links = [];
  for (const diagnostic of diagnostics) {
    for (const authority of diagnostic.links.authority) {
      const sandboxes = diagnostic.links.sandbox.length > 0 ? diagnostic.links.sandbox : [null];
      for (const sandbox of sandboxes) {
        const refs = [
          ...(diagnostic.source.refs ?? []),
          ...(authorityById.get(authority)?.source.refs ?? []),
          ...(sandbox ? sandboxById.get(sandbox)?.source.refs ?? [] : []),
        ];
        links.push({
          id: `cross-link:${diagnostic.code}:${authority}:${sandbox ?? "none"}`,
          kind: "cross_link",
          diagnostic_code: diagnostic.code,
          authority,
          sandbox,
          label: `${diagnostic.code} → ${authority}${sandbox ? ` → ${sandbox}` : ""}`,
          why: sandbox
            ? "the denial names one authority verdict and the sandbox boundary that prevents an ambient escape"
            : "the denial names the canonical authority verdict; no sandbox boundary is applicable to this diagnostic",
          status: refs.length > 0 ? "found" : "missing",
          source: evidenceStatus(refs, "no exact cross-link evidence"),
          evidence: makeEvidenceBundle(
            evidenceStatus(refs, "no exact cross-link evidence"),
            evidenceStatus(diagnostic.evidence.implementation.refs ?? [], "no diagnostic implementation evidence"),
            evidenceStatus(diagnostic.evidence.fixtures.refs ?? [], "no diagnostic fixture evidence"),
            evidenceStatus(diagnostic.evidence.projections.refs ?? [], "no diagnostic projection evidence"),
          ),
          links: { authority: [authority], sandbox: sandbox ? [sandbox] : [] },
        });
      }
    }
  }
  return links.sort((a, b) => a.id.localeCompare(b.id));
}

function flattenRows(report) {
  return [
    ...report.diagnostics,
    ...report.authority_holds,
    ...report.sandbox_facts,
    ...report.core_effect_leaves,
    ...report.enforcement_points,
  ];
}

function collectEvidenceStatuses(report) {
  const statuses = [];
  const visit = (row, field, value) => {
    if (!value || typeof value !== "object" || typeof value.status !== "string") return;
    if (value.status === "missing") statuses.push({ row: row.id, field, reason: value.reason ?? "missing evidence" });
  };
  for (const row of [...flattenRows(report), ...report.cross_links]) {
    for (const field of ["source", ...EVIDENCE_FIELDS]) {
      visit(row, field, field === "source" ? row.source : row.evidence?.[field]);
    }
  }
  return statuses.sort((a, b) => a.row.localeCompare(b.row) || a.field.localeCompare(b.field));
}

function collectStaleEvidence(report, sources) {
  const sourceMap = new Map(sources.map((file) => [file.path, file]));
  const stale = [];
  const visit = (row, status, field) => {
    if (!status || status.status !== "found") return;
    for (const ref of status.refs ?? []) {
      const file = sourceMap.get(ref.file);
      const actual = file?.lineText(ref.line);
      if (!file || actual !== ref.text) stale.push({ row: row.id, field, ref });
    }
  };
  for (const row of [...flattenRows(report), ...report.cross_links]) {
    visit(row, row.source, "source");
    for (const field of EVIDENCE_FIELDS) visit(row, row.evidence?.[field], field);
    for (const tier of Object.values(row.tiers ?? {})) {
      for (const field of EVIDENCE_FIELDS) visit(row, tier.evidence?.[field], `tiers.${field}`);
    }
  }
  return stale.sort((a, b) => a.row.localeCompare(b.row) || a.field.localeCompare(b.field) || a.ref.file.localeCompare(b.ref.file) || a.ref.line - b.ref.line);
}

function duplicateIds(rows) {
  const counts = new Map();
  for (const row of rows) counts.set(row.id, (counts.get(row.id) ?? 0) + 1);
  return [...counts.entries()].filter(([, count]) => count > 1).map(([id, count]) => ({ id, count })).sort((a, b) => a.id.localeCompare(b.id));
}

function validateReport(report, sources, unclassified) {
  const rows = flattenRows(report);
  const duplicates = duplicateIds(rows);
  const stale = collectStaleEvidence(report, sources);
  const missing = collectEvidenceStatuses(report);
  const differentialFailures = report.differential?.failures ?? [];
  const unavailable = report.differential?.unavailable ?? [];
  const checks = {
    status: duplicates.length === 0
      && stale.length === 0
      && unclassified.length === 0
      && differentialFailures.length === 0
      && unavailable.length === 0
      ? "pass"
      : "fail",
    duplicate_ids: duplicates,
    stale_evidence: stale,
    unclassified,
    missing_evidence: missing,
    differential_failures: differentialFailures,
    runtime_unavailable: unavailable,
    untracked_differences: report.differential?.untracked_differences ?? [],
  };
  if (report.checks.status !== "pending") {
    assert(report.checks.status === checks.status, "checks.status disagrees with recomputed checks");
    assert(JSON.stringify(report.checks.duplicate_ids) === JSON.stringify(checks.duplicate_ids), "duplicate check is stale");
    assert(JSON.stringify(report.checks.stale_evidence) === JSON.stringify(checks.stale_evidence), "stale evidence check is stale");
    assert(JSON.stringify(report.checks.unclassified) === JSON.stringify(checks.unclassified), "unclassified check is stale");
    assert(JSON.stringify(report.checks.missing_evidence) === JSON.stringify(checks.missing_evidence), "missing evidence check is stale");
    assert(JSON.stringify(report.checks.differential_failures) === JSON.stringify(checks.differential_failures), "differential failure check is stale");
    assert(JSON.stringify(report.checks.runtime_unavailable) === JSON.stringify(checks.runtime_unavailable), "runtime unavailable check is stale");
    assert(JSON.stringify(report.checks.untracked_differences) === JSON.stringify(checks.untracked_differences), "untracked difference check is stale");
  }
  assert(report.denominator && Array.isArray(report.denominator.rules), "runtime denominator missing");
  assert(report.denominator.rules.length === ENFORCEMENT_DEFINITIONS.length, "runtime denominator count disagrees");
  for (const rule of report.denominator.rules) {
    assert(rule.tiers && TIERS.every((tier) => rule.tiers[tier] && Object.hasOwn(rule.tiers[tier], "allow") && Object.hasOwn(rule.tiers[tier], "refuse")), `runtime denominator missing pair observations for ${rule.id}`);
  }
  assert(report.totals.diagnostics === report.diagnostics.length, "diagnostic total disagrees");
  assert(report.totals.authority_holds === report.authority_holds.length, "authority total disagrees");
  assert(report.totals.sandbox_facts === report.sandbox_facts.length, "sandbox total disagrees");
  assert(report.totals.core_effect_leaves === report.core_effect_leaves.length, "core total disagrees");
  assert(report.totals.enforcement_points === report.enforcement_points.length, "enforcement total disagrees");
  assert(report.totals.enforcement_cells === report.enforcement_points.length * TIERS.length, "enforcement cell total disagrees");
  assert(report.totals.cross_links === report.cross_links.length, "cross-link total disagrees");
  assert(report.totals.rows === rows.length, "row total disagrees");
  return checks;
}

function escTsv(value) {
  return String(value ?? "").replace(/\\/gu, "\\\\").replace(/\t/gu, "\\t").replace(/\r/gu, "\\r").replace(/\n/gu, "\\n");
}

function evidenceCell(value) {
  if (!value || value.status !== "found") return value?.reason ? `MISSING: ${value.reason}` : "";
  return value.refs.map((ref) => `${ref.file}:${ref.line}`).join(";");
}

const TSV_HEADER = [
  "family",
  "id",
  "kind",
  "label",
  "status",
  "aot",
  "run",
  "interpret",
  "diagnostic_codes",
  "authority_links",
  "sandbox_links",
  "source_evidence",
  "implementation_evidence",
  "fixture_evidence",
  "projection_evidence",
  "intended_difference",
  "runtime_comparison",
  "runtime_aot",
  "runtime_run",
  "runtime_interpret",
];

function tsvRow(row, family) {
  const tierOutcome = (tier) => row.tiers?.[tier]?.outcome ?? "";
  const difference = row.tiers
    ? TIERS.map((tier) => `${tier}: ${row.tiers[tier].intended_difference}`).join(" | ")
    : "";
  const runtimeCell = (tier) => {
    const runtime = row.tiers?.[tier]?.runtime;
    if (!runtime) return "";
    if (runtime.status !== "observed") return `${runtime.status}: ${runtime.reason}`;
    return `${runtime.decision} [${runtime.diagnostic_codes.join(",")}] ${runtime.meaning}`;
  };
  return [
    family,
    row.id,
    row.kind,
    row.label,
    row.status,
    tierOutcome("aot"),
    tierOutcome("run"),
    tierOutcome("interpret"),
    (row.diagnostic_codes ?? []).join(","),
    (row.links?.authority ?? []).join(","),
    (row.links?.sandbox ?? []).join(","),
    evidenceCell(row.source),
    evidenceCell(row.evidence?.implementation),
    evidenceCell(row.evidence?.fixtures),
    evidenceCell(row.evidence?.projections),
    difference,
    row.runtime_comparison?.status ?? "",
    runtimeCell("aot"),
    runtimeCell("run"),
    runtimeCell("interpret"),
  ].map(escTsv).join("\t");
}

function renderTsv(report) {
  const lines = [TSV_HEADER.join("\t")];
  for (const row of report.diagnostics) lines.push(tsvRow(row, "diagnostic"));
  for (const row of report.authority_holds) lines.push(tsvRow(row, "authority"));
  for (const row of report.sandbox_facts) lines.push(tsvRow(row, "sandbox"));
  for (const row of report.core_effect_leaves) lines.push(tsvRow(row, "core_effect_leaf"));
  for (const row of report.enforcement_points) lines.push(tsvRow(row, "enforcement"));
  return `${lines.join("\n")}\n`;
}

function markdownEvidence(value) {
  if (!value || value.status !== "found") return `MISSING — ${value?.reason ?? "no evidence"}`;
  return value.refs.slice(0, 3).map((ref) => `${ref.file}:${ref.line}`).join(", ");
}

function md(value) {
  return String(value ?? "").replace(/\\/gu, "\\\\").replace(/\|/gu, "\\|").replace(/\n/gu, " ");
}
function markdownObservation(observation) {
  if (!observation) return "MISSING";
  if (observation.status !== "observed") return `UNAVAILABLE: ${observation.reason ?? "no observation"}`;
  return `${observation.decision} [${observation.diagnostic_codes.join(",") || "none"}] ${observation.meaning ?? observation.meaning_key ?? ""}`;
}

function renderMarkdown(report) {
  const lines = [
    "# Effect authority and sandbox census",
    "",
    "Source census plus a fail-closed three-tier runtime differential for effect denials, authority holds, sentry poisoning, harden fences, and process sandbox entry.",
    "A runtime cell is valid only when the same fixture is exercised through AOT, `jet run`, and `jet run --interpret`; unavailable execution is retained as a gate failure.",
    "",
    "## Totals",
    "",
    "| category | rows |",
    "| --- | ---: |",
    `| diagnostics | ${report.totals.diagnostics} |`,
    `| authority holds | ${report.totals.authority_holds} |`,
    `| sandbox facts | ${report.totals.sandbox_facts} |`,
    `| Core effect leaves | ${report.totals.core_effect_leaves} |`,
    `| enforcement points | ${report.totals.enforcement_points} |`,
    `| enforcement cells | ${report.totals.enforcement_cells} |`,
    `| cross-links | ${report.totals.cross_links} |`,
    `| TSV data rows | ${report.totals.tsv_rows - 1} |`,
    "",
    `Checks: **${report.checks.status}**; differential=${md(report.differential.status)}, duplicate IDs=${report.checks.duplicate_ids.length}, stale evidence=${report.checks.stale_evidence.length}, unclassified=${report.checks.unclassified.length}, runtime unavailable=${report.checks.runtime_unavailable.length}, differential failures=${report.checks.differential_failures.length}, reported missing evidence=${report.checks.missing_evidence.length}.`,
    "## Runtime safety denominator",
    "",
    md(report.denominator.statement),
    "",
    "| rule | fixture | decision | pair gate | AOT allow/refuse | jet run allow/refuse | --interpret allow/refuse | comparison |",
    "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ...report.denominator.rules.map((rule) => `| ${md(rule.id)} | ${md(rule.fixture)} | ${md(rule.decision)} | ${md(rule.pair_status)} | ${md(`${markdownObservation(rule.tiers.aot.allow)} / ${markdownObservation(rule.tiers.aot.refuse)}`)} | ${md(`${markdownObservation(rule.tiers.run.allow)} / ${markdownObservation(rule.tiers.run.refuse)}`)} | ${md(`${markdownObservation(rule.tiers.interpret.allow)} / ${markdownObservation(rule.tiers.interpret.refuse)}`)} | ${md(rule.comparison)} |`),
    "",
    "## Enforcement matrix",
    "",
    "| point | AOT | jet run | --interpret | diagnostic codes | intended difference | decision |",
    "| --- | --- | --- | --- | --- | --- | --- |",
  ];
  for (const row of report.enforcement_points) {
    const difference = TIERS.map((tier) => `${tier}: ${row.tiers[tier].intended_difference}`).join("; ");
    lines.push(`| ${md(row.label)} | ${md(row.tiers.aot.outcome)} | ${md(row.tiers.run.outcome)} | ${md(row.tiers.interpret.outcome)} | ${md(row.diagnostic_codes.join(", "))} | ${md(difference)} | ${md(row.decision)} |`);
  }
  lines.push(
    "",
    "## Runtime differential",
    "",
    `Gate: **${report.differential.gate.status}**; status=${report.differential.status}; unavailable=${report.differential.unavailable.length}; failures=${report.differential.failures.length}.`,
    "",
    "| fixture | enforcement point | allow/refuse pair | pair gate | comparison | AOT | jet run | --interpret | intended decision |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  );
  const runtimeCell = (fixture, tier) => markdownObservation(fixture.observations[tier]);
  const fixturePairs = new Map(report.fixtures.map((fixture) => [fixture.id, fixture.pair]));
  for (const fixture of report.differential.fixtures) {
    const pair = fixturePairs.get(fixture.id);
    lines.push(`| ${fixture.id} | ${fixture.enforcement_point} | ${md(`${pair.allow} / ${pair.refuse}`)} | ${fixture.pair.status} | ${fixture.comparison.status} | ${md(runtimeCell(fixture, "aot"))} | ${md(runtimeCell(fixture, "run"))} | ${md(runtimeCell(fixture, "interpret"))} | ${md(fixture.intended_difference?.decision_id ?? "none")} |`);
  }
  lines.push("", "### Unintended differences", "");
  if (report.differential.untracked_differences.length === 0) {
    lines.push("None observed. Any future divergence is a gate failure until it has an owner card.");
  } else {
    lines.push("| fixture | exact allow/refuse program | wrong tier / difference | tiers | card |");
    lines.push("| --- | --- | --- | --- | --- |");
    for (const divergence of report.differential.untracked_differences) {
      const programs = divergence.programs
        ? `allow=${divergence.programs.allow}; refuse=${divergence.programs.refuse}${divergence.programs.package ? `; package=${divergence.programs.package}` : ""}`
        : "manifest pair unavailable";
      lines.push(`| ${divergence.fixture} | ${md(programs)} | ${md(divergence.wrong_tier ?? divergence.kind)} | ${md(divergence.tiers.join(" vs "))} | ${md(divergence.card ?? "CARD REQUIRED")} |`);
    }
  }
  lines.push(
    "",
    "## Effect denial diagnostics",
    "",
    "| code | family | registry status | meaning | authority links | sandbox links | source | implementation | fixture | projection |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
  );
  for (const row of report.diagnostics) {
    lines.push(`| ${row.code} | ${row.family} | ${row.status} | ${md(row.meaning)} | ${md(row.links.authority.join(", "))} | ${md(row.links.sandbox.join(", "))} | ${md(markdownEvidence(row.source))} | ${md(markdownEvidence(row.evidence.implementation))} | ${md(markdownEvidence(row.evidence.fixtures))} | ${md(markdownEvidence(row.evidence.projections))} |`);
  }
  for (const [heading, rows] of [["Authority holds", report.authority_holds], ["Sandbox facts", report.sandbox_facts]]) {
    lines.push("", `## ${heading}`, "", "| id | label | status | decision | source | implementation | fixture | projection |", "| --- | --- | --- | --- | --- | --- | --- | --- |");
    for (const row of rows) lines.push(`| ${row.id} | ${md(row.label)} | ${row.status} | ${md(row.decision)} | ${md(markdownEvidence(row.source))} | ${md(markdownEvidence(row.evidence.implementation))} | ${md(markdownEvidence(row.evidence.fixtures))} | ${md(markdownEvidence(row.evidence.projections))} |`);
  }
  lines.push("", "## Core effect leaves", "", "| id | kind | leaf | registry status | authority links | sandbox links | source | implementation | fixture | projection |", "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
  for (const row of report.core_effect_leaves) lines.push(`| ${row.id} | ${row.kind} | ${row.leaf} | ${row.registry_status} | ${md(row.links.authority.join(", "))} | ${md(row.links.sandbox.join(", "))} | ${md(markdownEvidence(row.source))} | ${md(markdownEvidence(row.evidence.implementation))} | ${md(markdownEvidence(row.evidence.fixtures))} | ${md(markdownEvidence(row.evidence.projections))} |`);
  lines.push("", "## Cross-links", "", "| diagnostic | authority | sandbox | why | source |", "| --- | --- | --- | --- | --- |");
  for (const row of report.cross_links) lines.push(`| ${row.diagnostic_code} | ${row.authority} | ${row.sandbox ?? "not applicable"} | ${md(row.why)} | ${md(markdownEvidence(row.source))} |`);
  lines.push("", "## Check failures and reported gaps", "");
  const staticFailures = [...report.checks.duplicate_ids, ...report.checks.stale_evidence, ...report.checks.unclassified];
  const runtimeFailures = [...report.checks.differential_failures, ...report.checks.runtime_unavailable, ...report.checks.untracked_differences];
  if (staticFailures.length === 0 && runtimeFailures.length === 0) lines.push("No duplicate, stale, unclassified, unavailable, or divergent source rows.");
  else {
    for (const failure of [...staticFailures, ...runtimeFailures]) lines.push(`- ${md(JSON.stringify(failure))}`);
  }
  lines.push("", "Missing evidence is retained on each row with a `MISSING:` reason. It is not silently converted into coverage.", "");
  return `${lines.join("\n")}\n`;
}

function buildDenominator(manifestDenominator, differential) {
  return {
    statement: manifestDenominator.statement,
    rules: manifestDenominator.rules.map((rule) => {
      const fixture = differential.fixtures.find((candidate) => candidate.id === rule.fixture);
      return {
        ...rule,
        comparison: fixture?.comparison.status ?? "missing",
        pair_status: fixture?.pair.status ?? "missing",
        intended_difference: fixture?.intended_difference ?? null,
        tiers: Object.fromEntries(TIERS.map((tier) => [
          tier,
          {
            allow: fixture?.pair.allow_observations[tier] ?? null,
            refuse: fixture?.pair.refuse_observations[tier] ?? null,
          },
        ])),
      };
    }),
  };
}

function buildReport() {
  const sources = loadSources();
  const diagnosticsFile = sources.find((file) => file.path === DIAGNOSTICS_PATH);
  const effectsFile = sources.find((file) => file.path === EFFECT_SOURCE_PATH);
  const coreFile = sources.find((file) => file.path === CORE_CALLS_PATH);
  assert(diagnosticsFile, `missing ${DIAGNOSTICS_PATH}`);
  assert(effectsFile, `missing ${EFFECT_SOURCE_PATH}`);
  assert(coreFile, `missing ${CORE_CALLS_PATH}`);
  const implementationFiles = sources.filter((file) => file.path.startsWith("crates/") && [".rs", ".jet"].includes(extname(file.path)));
  const projectionFiles = sources.filter((file) => file.path.startsWith("crates/") && extname(file.path) === ".rs");
  const fixtureFiles = sources.filter((file) => (
    /(?:^|\/)(?:tests?|examples?)(?:\/|$)/u.test(file.path)
      || file.path.startsWith(`${FIXTURE_ROOT}/`)
      || /_tests?\.rs$/u.test(file.path)
  ));
  const fixtureManifest = loadFixtureManifest();
  const fixtures = fixtureManifest.fixtures;
  const effect_declarations = parseEffectDeclarations(effectsFile);
  const rawDiagnostics = parseDiagnostics(diagnosticsFile);
  const diagnostics = rawDiagnostics.map((row) => withDiagnosticEvidence(row, sources, implementationFiles, fixtureFiles, projectionFiles));
  const authority_holds = AUTHORITY_DEFINITIONS
    .map((definition) => makeDefinitionRow(definition, sources, implementationFiles, fixtureFiles, projectionFiles, "authority"))
    .sort((a, b) => a.id.localeCompare(b.id));
  const sandbox_facts = SANDBOX_DEFINITIONS
    .map((definition) => makeDefinitionRow(definition, sources, implementationFiles, fixtureFiles, projectionFiles, "sandbox"))
    .sort((a, b) => a.id.localeCompare(b.id));
  const coreResult = buildCoreLeaves(coreFile, implementationFiles, fixtureFiles, projectionFiles);
  const core_effect_leaves = coreResult.rows;
  const differential = buildRuntimeDifferential(fixtures);
  const enforcement_points = buildEnforcementPoints(sources, implementationFiles, fixtureFiles, projectionFiles, diagnostics, differential);
  const denominator = buildDenominator(fixtureManifest.denominator, differential);
  const cross_links = buildCrossLinks(diagnostics, authority_holds, sandbox_facts);
  const report = {
    schema_version: 3,
    generated_by: "scripts/agent/effect-authority-census.mjs",
    generated_command: GENERATED_COMMAND,
    generated_static_only: false,
    runtime_differential: true,
    tiers: TIERS,
    denominator,
    source_roots: SOURCE_ROOTS,
    sources_scanned: sources.length,
    authoritative: {
      effects: {
        source: EFFECT_SOURCE_PATH,
        runtime_registry: EFFECTS_PATH,
        declarations: effect_declarations,
      },
      authority: {
        sources: ["crates/jet-foundation/src/Authority.rs", "crates/jet-codegen/src/Prelude/Core/Authority.rs"],
        definitions: AUTHORITY_DEFINITIONS.map((definition) => definition.id),
      },
      sandbox: {
        sources: ["crates/jet-comptime/src/Comptime/Build/execution_runtime.rs", "crates/jet-codegen/src/Prelude/CoreLib/Top/ProcessSandbox.rs", "crates/jet-foundation/src/MemSentry.rs"],
        definitions: SANDBOX_DEFINITIONS.map((definition) => definition.id),
      },
    },
    fixtures: fixtures.map((fixture) => ({
      id: fixture.id,
      enforcement_point: fixture.enforcement_point,
      source: `${FIXTURE_ROOT}/${fixture.source}`,
      package: fixture.package ? `${FIXTURE_ROOT}/${fixture.package}` : null,
      pair: {
        allow: `${FIXTURE_ROOT}/${fixture.pair.allow_source}`,
        refuse: `${FIXTURE_ROOT}/${fixture.pair.refuse_source}`,
        allow_expected: fixture.pair.allow_expected,
        refuse_expected: fixture.pair.refuse_expected,
      },
      intended_difference: fixture.intended_difference ?? null,
    })),
    diagnostics,
    authority_holds,
    sandbox_facts,
    core_effect_leaves,
    enforcement_points,
    cross_links,
    differential,
    totals: {
      diagnostics: diagnostics.length,
      authority_holds: authority_holds.length,
      sandbox_facts: sandbox_facts.length,
      core_effect_leaves: core_effect_leaves.length,
      enforcement_points: enforcement_points.length,
      enforcement_cells: enforcement_points.length * TIERS.length,
      cross_links: cross_links.length,
      runtime_fixtures: differential.fixtures.length,
      rows: flattenRows({ diagnostics, authority_holds, sandbox_facts, core_effect_leaves, enforcement_points }).length,
      tsv_rows: 1 + diagnostics.length + authority_holds.length + sandbox_facts.length + core_effect_leaves.length + enforcement_points.length,
    },
    checks: {
      status: "pending",
      duplicate_ids: [],
      stale_evidence: [],
      unclassified: [],
      missing_evidence: [],
      differential_failures: [],
      runtime_unavailable: [],
      untracked_differences: [],
    },
    notes: [
      "The source table is anchored to Prelude effects, Foundation Authority, and the native build sandbox boundary.",
      "Each fixture is executed through AOT, jet run, and --interpret; unavailable execution is fail-closed and retained in the report.",
      "A divergent refusal or diagnostic meaning is a gate failure unless the fixture cites its ratified intended-difference decision.",
      "Missing fixture or projection evidence remains explicit on the row and in checks.missing_evidence.",
    ],
  };
  report.checks = validateReport(report, sources, coreResult.unclassified);
  report.totals.missing_evidence = report.checks.missing_evidence.length;
  const tsv = renderTsv(report);
  assert(report.totals.tsv_rows === tsv.trimEnd().split("\n").length, "TSV total disagrees");
  return { report, tsv, markdown: renderMarkdown(report), sources };
}

function parseArgs(argv) {
  if (argv.includes("--help") || argv.includes("-h")) return { help: true };
  let mode = null;
  for (const arg of argv) {
    if (arg === "--write" || arg === "--check" || arg === "--probe") {
      assert(mode === null, "choose exactly one of --write, --check, or --probe");
      mode = arg.slice(2);
    } else {
      fail(`unknown argument ${arg}; use --help`);
    }
  }
  assert(mode !== null, "choose --write, --check, or --probe; use --help for usage");
  return { mode };
}

function printHelp() {
  process.stdout.write([
    "Effect authority census",
    "",
    `Usage: ${GENERATED_COMMAND} --write`,
    `       ${GENERATED_COMMAND} --check`,
    `       ${GENERATED_COMMAND} --probe`,
    `       ${GENERATED_COMMAND} --help`,
    "",
    "--write  deterministically regenerate the JSON, TSV, and Markdown reports.",
    "--check  regenerate in memory, validate totals/evidence, and fail if reports are stale.",
    "--probe  run the same differential without writing generated reports.",
    "",
  ].join("\n"));
}
function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    printHelp();
    return;
  }
  const result = buildReport();
  if (args.mode === "probe") {
    process.stdout.write(`PROBE OK differential=${result.report.differential.status} gate=${result.report.differential.gate.status} unavailable=${result.report.differential.unavailable.length} failures=${result.report.differential.failures.length} compiler=${result.report.differential.local_compiler ?? "unavailable"}\n`);
    if (result.report.checks.status !== "pass") process.exitCode = 1;
    return;
  }
  const expected = {
    [OUTPUTS.json]: `${JSON.stringify(result.report, null, 2)}\n`,
    [OUTPUTS.tsv]: result.tsv,
    [OUTPUTS.md]: result.markdown,
  };
  if (args.mode === "write") {
    for (const path of Object.values(OUTPUTS)) mkdirSync(dirname(absPath(path)), { recursive: true });
    for (const [path, content] of Object.entries(expected)) writeFileSync(absPath(path), content);
    process.stdout.write(`WRITE OK rows=${result.report.totals.rows} tsv_rows=${result.report.totals.tsv_rows} checks=${result.report.checks.status} differential=${result.report.differential.status} gate=${result.report.differential.gate.status} missing_evidence=${result.report.checks.missing_evidence.length}\n`);
    if (result.report.checks.status !== "pass") process.exitCode = 1;
    return;
  }
  for (const [path, content] of Object.entries(expected)) {
    assert(existsSync(absPath(path)), `${path} is missing; run ${GENERATED_COMMAND} --write`);
    const actual = readFileSync(absPath(path), "utf8");
    assert(actual === content, `${path} is stale; run ${GENERATED_COMMAND} --write`);
  }
  process.stdout.write(`CHECK OK rows=${result.report.totals.rows} tsv_rows=${result.report.totals.tsv_rows} checks=${result.report.checks.status} differential=${result.report.differential.status} gate=${result.report.differential.gate.status} missing_evidence=${result.report.checks.missing_evidence.length}\n`);
  if (result.report.checks.status !== "pass") process.exitCode = 1;
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}
