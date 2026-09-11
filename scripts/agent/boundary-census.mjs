#!/usr/bin/env node

import {
  existsSync,
  readFileSync,
  readdirSync,
} from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { coreSourceFacts } from "./check-core-surface-ledger.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const REQUESTED_CORE_SOURCE = "Source/Core.jet";
const CORE_SOURCE = "crates/jet-codegen/src/Prelude/Core.jet";
const SCHEMA = "jet-semantic-boundary-census-v1";
const SELECTED_CORE_MODULE = /^(?:app|core\.(?:compute|data|encoding|http|math|game|ui|web|units|mem|net|text))(?:\.|$)/;
const SOURCE_EXTENSIONS = new Set([".js", ".jet", ".md", ".rs"]);

const FACT_DIMENSIONS = Object.freeze({
  nominal_identity: {
    label: "nominal identity",
    pattern: /\b(?:key|id|identity|schema|type_name|type_id|row_type|column_id|column_key)\b|\b(?:Identity|Schema|Key|Id)\b/i,
  },
  unit: {
    label: "unit",
    pattern: /\b(?:unit|duration|timeout|deadline|bytes|width|height|stride|scale|precision|ms|ns|hz|rate)\b|\b(?:Duration|Instant|Decimal|Fraction)\b/i,
  },
  shape: {
    label: "shape",
    pattern: /\b(?:shape|rank|rows|columns|column|dims?|dimension|arity|length|len|stride|extent)\b|\b(?:Shape|Tensor|Matrix|Vec[234]|Mat[34])\b/i,
  },
  nullability: {
    label: "nullability",
    pattern: /\b(?:Option|None|null|nullable|missing|optional|absent)\b|\?\s*[A-Za-z_]/i,
  },
  ownership: {
    label: "ownership",
    pattern: /\b(?:owner|ownership|release|drop|Arc|buffer|borrow|lifetime|clone|copy|handle|Handle|Stream|Reader|Writer)\b/i,
  },
  model_identity: {
    label: "model identity",
    pattern: /\b(?:model|weights?|checkpoint|embedding|artifact|inference)\b/i,
  },
  index_identity: {
    label: "index identity",
    pattern: /\b(?:index|vector_store|ann|nearest|top_k|search_index)\b/i,
  },
});

const REPRESENTATIONS = Object.freeze({
  string: {
    label: "String",
    pattern: /\bString\b|&\s*str\b|\bstr\b/i,
  },
  float: {
    label: "Float",
    pattern: /\bf(?:32|64)\b|\bFloat\b/i,
  },
  array: {
    label: "array",
    pattern: /\bVec\s*</i,
  },
  handle: {
    label: "handle",
    pattern: /\b(?:OpaqueHandle|RawHandle|Handle)\b|(?:\b(?:i64|u64|usize)\b[^,\n;)]*\bhandle\b|\bhandle\b[^,\n;)]*:\s*(?:i64|u64|usize))/i,
  },
  dynamic_record: {
    label: "dynamic record",
    pattern: /\b(?:DataTree|DataEvent|Record|Object|BTreeMap|BTreeSet|Value)\b/,
  },
});

const DOMAIN_RULES = Object.freeze([
  {
    domain: "parquet",
    owner: { card: "#2969", decision: "D-DATA-READER1" },
    path: /(?:Parquet|FFI)/i,
  },
  {
    domain: "columnar",
    owner: { card: "#2968", decision: "D-COLUMNAR-BOUNDARY1" },
    path: /(?:Column|Arrow|Parquet)/i,
  },
  {
    domain: "web_request",
    owner: { card: "#2962", decision: "D-ENDPOINT-SHAPE1" },
    path: /(?:HTTP|Web|Router|Form|App)/i,
  },
  {
    domain: "model_provider",
    owner: { card: "#2971", decision: "D-MODEL-BACKEND1" },
    path: /(?:Provider|Foreign|Native)/i,
  },
  {
    domain: "model_index",
    owner: { card: "#2970", decision: "D-MODEL-PACKAGE1" },
    path: /(?:Model|Embedding|Index|Artifact|ProviderFacts|jet-pkg-model\/src\/.*Tensor)/i,
  },
  {
    domain: "tensor",
    owner: { card: "#2965", decision: "D-SPACE-GEOMETRY1" },
    path: /(?:Compute|Tensor|GPU|WebGpu)/i,
  },
  {
    domain: "geometry",
    owner: { card: "#2965", decision: "D-SPACE-GEOMETRY1" },
    path: /(?:Math|Linalg|Vector|Matrix|Geometry|Ui)/i,
  },
  {
    domain: "data_query",
    owner: { card: "#2963", decision: "D-QUERY-RETAIN1" },
    path: /(?:Data|Query|Flow|Fmt|Plot|Stats|Tree|JSON|CSV|CommonTypes)/i,
  },
]);

const SOURCE_ROOTS = [
  "crates/jet-foundation/src",
  "crates/jet-codegen/src/Prelude",
  "crates/jet-pkg-model/src",
];
const CORPUS_ROOTS = [
  "docs/spec/reference",
  "docs/audits/jet-refounding-2026-09-05",
  "examples/features",
];

function walkFiles(relativeRoot) {
  const absoluteRoot = join(ROOT, relativeRoot);
  if (!existsSync(absoluteRoot)) return [];
  const result = [];
  const visit = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (entry.name === ".git" || entry.name === "target" || entry.name === "node_modules") continue;
      const absolute = join(directory, entry.name);
      if (entry.isDirectory()) visit(absolute);
      else if (entry.isFile() && SOURCE_EXTENSIONS.has(extname(entry.name))) result.push(absolute);
    }
  };
  visit(absoluteRoot);
  return result;
}

function maskSource(source, language = "rust") {
  const chars = source.split("");
  let index = 0;
  while (index < source.length) {
    if (source.startsWith("//", index)) {
      const start = index;
      index += 2;
      while (index < source.length && source[index] !== "\n") index += 1;
      for (let cursor = start; cursor < index; cursor += 1) chars[cursor] = " ";
      continue;
    }
    if (source.startsWith("/*", index)) {
      const start = index;
      index += 2;
      let depth = 1;
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
      for (let cursor = start; cursor < index; cursor += 1) {
        if (source[cursor] !== "\n" && source[cursor] !== "\r") chars[cursor] = " ";
      }
      continue;
    }
    if (language === "rust") {
      const charLiteral = source.slice(index).match(/^'(?:\\.|[^'\\\n\r])'/);
      if (charLiteral) {
        for (let cursor = index; cursor < index + charLiteral[0].length; cursor += 1) chars[cursor] = " ";
        index += charLiteral[0].length;
        continue;
      }
    }
    const rawPrefix = source.slice(index).match(/^(?:b|c)?r(#+)?"/);
    if (rawPrefix) {
      const start = index;
      const hashes = rawPrefix[1] ?? "";
      const terminator = `"${hashes}`;
      const contentStart = index + rawPrefix[0].length;
      const contentEnd = source.indexOf(terminator, contentStart);
      index = contentEnd < 0 ? source.length : contentEnd + terminator.length;
      for (let cursor = start; cursor < index; cursor += 1) {
        if (source[cursor] !== "\n" && source[cursor] !== "\r") chars[cursor] = " ";
      }
      continue;
    }
    if (language === "javascript" && (source[index] === "'" || source[index] === "`")) {
      const quote = source[index];
      const start = index;
      index += 1;
      while (index < source.length) {
        if (source[index] === "\\") index += 2;
        else if (source[index] === quote) {
          index += 1;
          break;
        } else index += 1;
      }
      for (let cursor = start; cursor < index; cursor += 1) {
        if (source[cursor] !== "\n" && source[cursor] !== "\r") chars[cursor] = " ";
      }
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
      for (let cursor = start; cursor < index; cursor += 1) {
        if (source[cursor] !== "\n" && source[cursor] !== "\r") chars[cursor] = " ";
      }
      continue;
    }
    index += 1;
  }
  return chars.join("");
}

function balancedEnd(source, opening, open, close) {
  let depth = 0;
  for (let index = opening; index < source.length; index += 1) {
    if (source[index] === open) depth += 1;
    else if (source[index] === close) {
      depth -= 1;
      if (depth === 0) return index;
    }
  }
  return -1;
}

function lineAt(source, offset) {
  let line = 1;
  for (let index = 0; index < offset; index += 1) if (source[index] === "\n") line += 1;
  return line;
}

function lineText(source, line) {
  return source.split("\n")[line - 1]?.trim() ?? "";
}

function ownerTypeBefore(source, start) {
  const before = source.slice(Math.max(0, start - 2400), start);
  const matches = [...before.matchAll(/\bimpl(?:\s*<[^{}]*>)?\s+([^{}]+?)(?=\{|$)/g)];
  const clause = matches.at(-1)?.[1]?.replace(/\s+/g, " ").trim() ?? "";
  const type = clause.match(/\bfor\s+(?:&\s*)?(?:[A-Za-z_]\w*::)*([A-Za-z_]\w*)/)?.[1]
    ?? clause.match(/^(?:[A-Za-z_]\w*::)*([A-Za-z_]\w*)/)?.[1];
  return type ?? null;
}

function rustFunctionOpening(masked, start) {
  let angleDepth = 0;
  for (let index = start; index < masked.length; index += 1) {
    if (masked[index] === "<") angleDepth += 1;
    else if (masked[index] === ">" && angleDepth > 0) angleDepth -= 1;
    else if (masked[index] === "(" && angleDepth === 0) return index;
  }
  return -1;
}

function parseRustFunctions(source) {
  const masked = maskSource(source, "rust");
  const pattern = /\b(?:(pub(?:\s*\([^)]*\))?)\s+)?fn\s+([A-Za-z_]\w*)/g;
  const functions = [];
  let match;
  while ((match = pattern.exec(masked))) {
    const start = match.index;
    const visibility = match[1] ?? "private";
    const name = match[2];
    const opening = rustFunctionOpening(masked, start + match[0].length);
    if (opening < 0) continue;
    const close = balancedEnd(masked, opening, "(", ")");
    if (close < 0) continue;
    const brace = masked.indexOf("{", close);
    const semicolon = masked.indexOf(";", close);
    if (brace < 0 || (semicolon >= 0 && semicolon < brace)) continue;
    const end = balancedEnd(masked, brace, "{", "}");
    if (end < 0) continue;
    const signature = source.slice(start, brace).replace(/\s+/g, " ").trim();
    const args = source.slice(opening + 1, close).replace(/\s+/g, " ").trim();
    const arrow = source.slice(close, brace).match(/->\s*([\s\S]*?)(?:\bwhere\b|$)/);
    functions.push({
      language: "rust",
      name,
      visibility,
      start,
      end,
      line: lineAt(source, start),
      signature,
      args,
      returnType: arrow?.[1]?.trim() || "()",
      body: source.slice(brace + 1, end),
      ownerType: ownerTypeBefore(source, start),
    });
    pattern.lastIndex = end + 1;
  }
  return functions;
}

function parseJavaScriptFunctions(source) {
  const masked = maskSource(source, "javascript");
  const pattern = /\b(?:(export)\s+)?(?:async\s+)?function\s+([A-Za-z_]\w*)\s*\(/g;
  const functions = [];
  let match;
  while ((match = pattern.exec(masked))) {
    const start = match.index;
    const opening = masked.indexOf("(", start + match[0].length - 1);
    const close = balancedEnd(masked, opening, "(", ")");
    const brace = close < 0 ? -1 : masked.indexOf("{", close);
    const end = brace < 0 ? -1 : balancedEnd(masked, brace, "{", "}");
    if (close < 0 || brace < 0 || end < 0) continue;
    functions.push({
      language: "javascript",
      name: match[2],
      visibility: match[1] ? "export" : "private",
      start,
      end,
      line: lineAt(source, start),
      signature: source.slice(start, brace).replace(/\s+/g, " ").trim(),
      args: source.slice(opening + 1, close).replace(/\s+/g, " ").trim(),
      returnType: "(inferred)",
      body: source.slice(brace + 1, end),
      ownerType: null,
    });
    pattern.lastIndex = end + 1;
  }
  return functions;
}

function parseFunctions(source, extension) {
  return extension === ".js" ? parseJavaScriptFunctions(source) : parseRustFunctions(source);
}

function detectFacts(text) {
  return Object.entries(FACT_DIMENSIONS)
    .filter(([, definition]) => definition.pattern.test(text))
    .map(([name]) => name);
}

function unqualifiedArray(text) {
  const scalar = /^(?:f(?:32|64)|i(?:8|16|32|64|128)|u(?:8|16|32|64|128)|usize|isize|bool|String|str|char)(?:\s*;.*)?$/i;
  for (const match of text.matchAll(/\bVec\s*<([^>\n]+)>/g)) {
    if (scalar.test(match[1].trim())) return true;
  }
  return /\[\s*(?:f(?:32|64)|i(?:8|16|32|64|128)|u(?:8|16|32|64|128)|usize|isize|bool|String|str|char)\b/i.test(text);
}

function detectRepresentations(text) {
  return Object.entries(REPRESENTATIONS)
    .filter(([name, definition]) => name === "array" ? unqualifiedArray(text) : definition.pattern.test(text))
    .map(([name]) => name);
}

function representationLabels(names) {
  return names.map((name) => REPRESENTATIONS[name]?.label ?? name);
}

function sourceDomain(relativePath) {
  const rule = DOMAIN_RULES.find((candidate) => candidate.path.test(relativePath));
  return rule?.domain ?? "unclassified";
}

function sourcePolicy(relativePath) {
  if (/crates\/jet-codegen\/src\/Prelude\/CoreLib\/Top\/(?:Data|Compute|HTTP|Web|App|Linalg|Tensor|Parquet|Arrow|Column)/i.test(relativePath)) return true;
  if (/crates\/jet-codegen\/src\/Prelude\/CoreLib\/JetStd\/(?:DataTree|JSONDataTree|CommonTypes|MathTaskMem|Parquet|Arrow)/i.test(relativePath)) return true;
  if (/crates\/jet-codegen\/src\/Prelude\/Core\/(?:Column|Columns|Math|Tensor|Parquet|.*Arrow|Ui)/i.test(relativePath)) return true;
  if (/crates\/jet-foundation\/src\/(?:Arrow|Data|Compute|HTTP|Web|Column|Parquet|Model|Index|Tensor|Provider|Foreign|Native|CoreModule)/i.test(relativePath)) return true;
  if (/crates\/jet-pkg-model\/src\/(?:Model|Embedding|Index|ProviderFacts|Artifact|Tensor|Package\/|Prelude\/Parquet|FFI)/i.test(relativePath)) return true;
  return false;
}

function registryRows(facts) {
  return facts.modules
    .filter((module) => SELECTED_CORE_MODULE.test(module.module))
    .flatMap((module) => {
      const members = (module.members ?? []).map((member) => ({
        module: module.module,
        member,
        kind: "member",
        sourceLine: module.sourceLine ?? null,
      }));
      const types = (module.types ?? []).map((type) => ({
        module: module.module,
        member: typeof type === "string" ? type : type.name,
        kind: "type",
        sourceLine: module.sourceLine ?? null,
      }));
      return [...members, ...types];
    });
}

function registryMatches(fn, rows) {
  const haystack = fn.name.replace(/^jet_/, "").toLowerCase();
  const matches = rows.filter((row) => {
    const member = String(row.member).toLowerCase();
    if (member.length < 3) return false;
    if (fn.ownerType && fn.ownerType.toLowerCase() === member) return true;
    return haystack === member || haystack.endsWith(`_${member}`);
  });
  return [...new Map(matches.map((row) => [`${row.module}:${row.member}:${row.kind}`, row])).values()]
    .sort((left, right) => `${left.module}:${left.member}`.localeCompare(`${right.module}:${right.member}`));
}

function isTypedCarrier(text) {
  return /\b(?:Jet|Data|Tensor|Vec[234]|Mat[34]|Matrix|Schema|Identity|Column|Plan|Request|Response|S::|T::|__jet_(?:Encode|Decode))\b|(?:shape|schema|identity|metadata|authority|owner|lifetime)/i.test(text);
}

function conversionEvidence(text, fnName) {
  const evidence = [];
  const patterns = [
    ["codec", /\b(?:encode|decode|serialize|deserialize|marshal|unmarshal|wire|json|csv|parquet|arrow)\b/i],
    ["bridge", /\b(?:ffi|bridge|provider|adapter|foreign|native|gpu|webgpu)\b/i],
    ["projection", /(?:\b(?:to_string|as_str|to_list|to_vec|into_bytes|as_bytes|collect|project|unproject|render|display|show)\b|_(?:show|shape|rank|dimensions?)\b)/i],
    ["identity", /\b(?:canonical|identity|schema|type_name|column_id|key)\b/i],
    ["ownership", /\b(?:owner|release|drop|Arc|clone|copy|buffer|handle|stream|reader|writer)\b/i],
  ];
  for (const [kind, pattern] of patterns) if (pattern.test(text)) evidence.push(kind);
  if (/^(?:to_|from_|as_|into_)/.test(fnName)) evidence.push("named conversion");
  return [...new Set(evidence)];
}

function validationEvidence(text) {
  const evidence = [];
  const patterns = [
    ["validation", /\b(?:validate|validated|validation|check|checked|ensure|assert|reject|invalid|error|failure)\b/i],
    ["bounds", /\b(?:bounds?|range|limit|finite|overflow|out_of_bounds|size_mismatch)\b|(?:\b(?:len|length|rank|extent)\b[^;\n]*(?:>|<|!=|==))/i],
    ["fallible result", /\b(?:Result|Option|JetOutcome|FieldError|DataError)\b|(?:\b(?:Err|None)\b[^;\n]*(?:return|map_err|ok_or|is_none))/i],
    ["availability", /\b(?:unsupported|unavailable|missing|provider-owned|requires? bridge)\b/i],
  ];
  for (const [kind, pattern] of patterns) if (pattern.test(text)) evidence.push(kind);
  return [...new Set(evidence)];
}

function classifyBoundary({ facts, args, returnType, body, bodyRepresentations, conversion, validation, ownerType, name }) {
  const argumentRepresentations = detectRepresentations(args);
  const returnRepresentations = detectRepresentations(returnType);
  const observedExit = returnRepresentations.length > 0
    ? returnRepresentations
    : returnType === "(inferred)" ? bodyRepresentations : [];
  const broadExit = observedExit.length > 0;
  const carrier = isTypedCarrier(`${returnType} ${body} ${ownerType ?? ""}`);
  const explicitDisplay = /\b(?:show|display|format|to_string|as_str|name|path|url|json)\b/i.test(name + body);
  const explicitShape = facts.includes("shape")
    && /\b(?:shape|rank|dims?|dimension|extent)\b/i.test(name + body);
  const dynamicCodec = /\b(?:DataTree|encode|decode|json|wire|ffi|bridge|provider)\b/i.test(body + returnType);
  const listProjection = /\b(?:to_list|values|to_vec|collect)\b/i.test(name + body);

  if (!broadExit && argumentRepresentations.length > 0 && carrier) {
    if (validation.length > 0) {
      return {
        classification: "runtime_validation",
        rationale: "A broad input is admitted to a typed carrier only after an explicit checked path.",
      };
    }
    return {
      classification: "clean",
      rationale: "The boundary returns a typed carrier instead of exposing the incoming representation.",
    };
  }
  if (!broadExit && argumentRepresentations.length === 0) {
    return {
      classification: "clean",
      rationale: "No unqualified representation exits this boundary; the semantic carrier remains typed.",
    };
  }
  if (validation.length > 0 && broadExit) {
    return {
      classification: "runtime_validation",
      rationale: "The outgoing representation is guarded by a checked validation/error path.",
    };
  }
  if (broadExit && listProjection && facts.includes("shape") && !/\b(?:shape|rank|dims?|dimension|schema|metadata)\b/i.test(body + returnType)) {
    return {
      classification: "gap",
      rationale: "A shape-bearing value is projected to an array without a shape carrier in this boundary.",
    };
  }
  if (broadExit && (explicitShape || dynamicCodec || explicitDisplay || (carrier && conversion.length > 0))) {
    return {
      classification: "intentional_conversion",
      rationale: "The source names an explicit shape, wire, display, bridge, or representation conversion.",
    };
  }
  if (broadExit) {
    return {
      classification: "gap",
      rationale: "A semantic fact reaches an unqualified representation with no carrier or validation evidence.",
    };
  }
  return {
    classification: "clean",
    rationale: "The semantic carrier remains typed at the observed boundary.",
  };
}

function ownerFor(domain) {
  if (domain === "model_index") return { card: "#2970", decision: "D-MODEL-PACKAGE1" };
  const rule = DOMAIN_RULES.find((candidate) => candidate.domain === domain);
  return rule?.owner ?? { card: "#2974", decision: null };
}

function decisionIds(source) {
  return [...new Set(source.match(/D-[A-Z0-9-]+(?:=[A-Z])?/g) ?? [])].sort();
}

function corpusScan() {
  const files = CORPUS_ROOTS.flatMap((root) => walkFiles(root));
  const observations = [];
  for (const absolute of files) {
    const relativePath = relative(ROOT, absolute);
    const source = readFileSync(absolute, "utf8");
    const lines = source.split("\n");
    const facts = detectFacts(source);
    const evidence = [];
    for (let index = 0; index < lines.length; index += 1) {
      const lineFacts = detectFacts(lines[index]);
      if (lineFacts.length === 0) continue;
      if (!/(?:Data|Tensor|Arrow|Parquet|route|request|schema|ownership|unit|shape|nullable|model|embedding|index|column|query|group)/i.test(lines[index])) continue;
      evidence.push({ line: index + 1, text: lines[index].trim(), facts: lineFacts });
    }
    if (facts.length > 0 && evidence.length > 0) {
      observations.push({
        path: relativePath,
        facts: [...new Set(evidence.flatMap((item) => item.facts))].sort(),
        matching_lines: evidence.length,
        evidence: evidence.slice(0, 3),
      });
    }
  }
  observations.sort((left, right) => left.path.localeCompare(right.path));
  return {
    roots: CORPUS_ROOTS,
    files_scanned: files.length,
    matching_files: observations.length,
    evidence_lines_per_file: 3,
    observations,
  };
}

function semanticRows(facts, files) {
  const rows = registryRows(facts);
  const rowsByModule = new Map();
  for (const row of rows) rowsByModule.set(`${row.module}:${row.member}:${row.kind}`, row);
  const candidates = [];
  const parsedFiles = [];
  for (const absolute of files) {
    const relativePath = relative(ROOT, absolute);
    if (!sourcePolicy(relativePath)) continue;
    const extension = extname(relativePath);
    if (extension !== ".rs" && extension !== ".js") continue;
    const source = readFileSync(absolute, "utf8");
    const functions = parseFunctions(source, extension);
    parsedFiles.push({ path: relativePath, functions: functions.length });
    const domain = sourceDomain(relativePath);
    const fileDecisions = decisionIds(source);
    for (const fn of functions) {
      const matches = registryMatches(fn, rows);
      const generated = /^jet_(?:data|compute|http|web|app|tensor|column|arrow|model|index)/i.test(fn.name);
      const adapterPath = /(?:Column|Arrow|Parquet|Math|Linalg|Model|Embedding|Index|Artifact|Tensor)/i.test(relativePath);
      const dataTreePath = /(?:DataTree|JSONDataTree|CommonTypes)/i.test(relativePath);
      const publicDeclaration = fn.visibility !== "private";
      const ownerCarrier = publicDeclaration && fn.ownerType && /^(?:Jet|Data|Tensor|Vec|Mat|Matrix|Model|Index)/.test(fn.ownerType);
      const isBoundary = generated || matches.length > 0 || (publicDeclaration && (adapterPath || dataTreePath || ownerCarrier));
      if (!isBoundary) continue;

      const text = `${fn.signature}\n${fn.body}`;
      const factsFound = detectFacts(`${text}\n${fn.ownerType ?? ""}`);
      const argumentRepresentations = detectRepresentations(fn.args);
      const returnRepresentations = detectRepresentations(fn.returnType);
      const bodyRepresentations = detectRepresentations(fn.body);
      const conversion = conversionEvidence(text, fn.name);
      const validation = validationEvidence(text);

      const classification = classifyBoundary({
        facts: factsFound,
        args: fn.args,
        returnType: fn.returnType,
        bodyRepresentations,
        body: fn.body,
        conversion,
        validation,
        ownerType: fn.ownerType,
        name: fn.name,
      });
      const owner = ownerFor(domain, factsFound);
      const id = `${relativePath}:${fn.line}:${fn.name}`;
      const registryModules = [...new Set(matches.map((match) => match.module))];
      const registryMembers = matches.map((match) => ({ module: match.module, member: match.member, kind: match.kind, line: match.sourceLine }));
      const outgoing = returnRepresentations.length > 0
        ? representationLabels(returnRepresentations)
        : fn.language === "javascript" && bodyRepresentations.length > 0
          ? representationLabels(bodyRepresentations)
          : [fn.returnType];
      const incoming = argumentRepresentations.length > 0 ? representationLabels(argumentRepresentations) : [fn.args || "(none)"];
      const cleanCarrier = fn.returnType === "()"
        ? "the receiver or owned store (unit return)"
        : fn.returnType === "(inferred)"
          ? "the JavaScript value carrier"
          : fn.returnType;
      const losingMechanism = classification.classification === "clean"
        ? `none — semantic carrier remains in ${cleanCarrier}`
        : classification.classification === "runtime_validation"
          ? `${outgoing.join(", ")} crosses through a checked path (${validation.join(", ")})`
          : classification.classification === "intentional_conversion"
            ? `${fn.returnType} uses an explicit ${conversion.join(", ") || "representation"} conversion`
            : `${factsFound.join(", ")} becomes ${outgoing.join(", ")}; no typed carrier or validator is observed [INFERENCE: static source only]`;
      const row = {
        id,
        boundary: `${relativePath}::${fn.ownerType ? `${fn.ownerType}.` : ""}${fn.name}`,
        domain,
        registry: {
          source: CORE_SOURCE,
          modules: registryModules,
          members: registryMembers,
          selected_by: matches.length > 0 ? "Core declaration registry match" : "adapter path/public declaration policy",
        },
        source: {
          path: relativePath,
          language: fn.language,
          line: fn.line,
          excerpt: lineText(source, fn.line),
          decisions: fileDecisions,
        },
        entering_type: fn.args || "(none)",
        exiting_type: fn.returnType,
        semantic_facts: factsFound,
        incoming_representation: incoming,
        outgoing_representation: outgoing,
        conversion_mechanism: conversion,
        classification: classification.classification,
        classification_rationale: classification.rationale,
        losing_mechanism: losingMechanism,
        lost_fact: classification.classification === "gap" ? factsFound : [],
        consumer_consequence: classification.classification === "gap"
          ? `A consumer cannot recover ${factsFound.join(", ")} from this ${outgoing.join(", ")} result without consulting another boundary [INFERENCE: no runtime execution].`
          : null,
        authority: {
          structural_owner: owner,
          registry: CORE_SOURCE,
          source_decisions: fileDecisions,
        },
        evidence: {
          kind: "static-source",
          runtime_status: "not-run",
          runtime_reason: "This census does not infer a runtime defect from a type name or stale binary.",
        },
      };
      if (factsFound.length === 0) continue;
      if (!row.entering_type || !row.exiting_type || row.semantic_facts.length === 0 || !row.losing_mechanism) {
        throw new Error(`unclassifiable boundary ${id}`);
      }
      candidates.push(row);
    }
  }
  candidates.sort((left, right) => left.id.localeCompare(right.id));
  return { rows: candidates, parsedFiles, registryRows: [...rowsByModule.values()] };
}

function validateRows(rows) {
  const classifications = new Set(["clean", "intentional_conversion", "runtime_validation", "gap"]);
  for (const row of rows) {
    if (!classifications.has(row.classification)) throw new Error(`unknown classification for ${row.id}`);
    if (!row.boundary || !row.entering_type || !row.exiting_type || !row.losing_mechanism) throw new Error(`incomplete boundary ${row.id}`);
    if (!Array.isArray(row.semantic_facts) || row.semantic_facts.length === 0) throw new Error(`missing semantic facts for ${row.id}`);
    for (const fact of row.semantic_facts) if (!Object.hasOwn(FACT_DIMENSIONS, fact)) throw new Error(`unregistered semantic fact ${fact}`);
    if (row.classification === "gap") {
      if (!row.lost_fact?.length || !row.consumer_consequence?.includes("[INFERENCE:")) throw new Error(`gap lacks explicit inference for ${row.id}`);
    }
  }
}

function coverage(rows, corpus) {
  return Object.fromEntries(Object.entries(FACT_DIMENSIONS).map(([dimension, definition]) => {
    const related = rows.filter((row) => row.semantic_facts.includes(dimension));
    const byClassification = Object.fromEntries(["clean", "intentional_conversion", "runtime_validation", "gap"].map((classification) => [
      classification,
      related.filter((row) => row.classification === classification).length,
    ]));
    return [dimension, {
      label: definition.label,
      boundary_rows: related.length,
      by_classification: byClassification,
      corpus_files: corpus.observations.filter((observation) => observation.facts.includes(dimension)).length,
      representative_boundaries: related.slice(0, 5).map((row) => row.id),
    }];
  }));
}

export function census() {
  const sourceFacts = coreSourceFacts();
  const files = SOURCE_ROOTS.flatMap((root) => walkFiles(root));
  const inventory = semanticRows(sourceFacts, files);
  validateRows(inventory.rows);
  const corpus = corpusScan();
  const counts = Object.fromEntries(["clean", "intentional_conversion", "runtime_validation", "gap"].map((classification) => [
    classification,
    inventory.rows.filter((row) => row.classification === classification).length,
  ]));
  const unresolved = inventory.rows.filter((row) => row.classification === "gap");
  const selectedModules = sourceFacts.modules
    .filter((module) => SELECTED_CORE_MODULE.test(module.module))
    .map((module) => ({
      module: module.module,
      source_line: module.sourceLine ?? null,
      members: [...(module.members ?? [])].sort(),
      types: (module.types ?? []).map((type) => typeof type === "string" ? type : type.name).sort(),
    }));
  return {
    schema: SCHEMA,
    status: "ok",
    declaration_source: {
      requested: REQUESTED_CORE_SOURCE,
      resolved: CORE_SOURCE,
      registry: "coreSourceFacts() from check-core-surface-ledger.mjs (#2898 source registry)",
      selected_modules: selectedModules,
      selected_member_count: selectedModules.reduce((total, module) => total + module.members.length + module.types.length, 0),
    },
    selection_policy: {
      source_roots: SOURCE_ROOTS,
      selected_module_rule: String(SELECTED_CORE_MODULE),
      public_boundary_rule: "registered Core member/type, generated jet_* adapter, or public typed adapter declaration in a policy-selected source path",
      candidate_filter: "Retain every policy-selected boundary that carries at least one semantic fact; rows without an unqualified representation remain as clean denominator rows. The full Core registry inventory remains in denominator.registry_rows.",
      representation_rule: {
        ...Object.fromEntries(Object.entries(REPRESENTATIONS).map(([name, definition]) => [name, String(definition.pattern)])),
        array: "unqualified Vec<scalar> or scalar slice/array; typed Vec<T> remains a semantic carrier",
      },
      semantic_dimensions: Object.fromEntries(Object.entries(FACT_DIMENSIONS).map(([name, definition]) => [name, String(definition.pattern)])),
      excluded_non_boundary_files: "All other Prelude/foundation files; they remain outside the selected denominator rather than being guessed at.",
    },
    denominator: {
      files_scanned: files.length,
      parsed_files: inventory.parsedFiles,
      registry_rows: inventory.registryRows.length,
      selected_boundary_rows: inventory.rows.length,
      classifications: counts,
      unresolved_rows: unresolved.map((row) => row.id),
      fail_closed: unresolved.length > 0
        ? "Gap rows block a clean claim and carry an explicit static-source inference."
        : "No unresolved rows; an empty gap result is retained and auditable.",
    },
    dimensions: coverage(inventory.rows, corpus),
    rows: inventory.rows,
    unresolved_rows: unresolved,
    corpus,
    owners_reused: {
      "#2898": "Core declaration/registration census",
      "#2962": "checked endpoint and transport boundary",
      "#2963": "typed query and aggregate boundary",
      "#2965": "space-safe geometry and tensor shape boundary",
      "#2968": "Arrow ownership boundary",
      "#2969": "Parquet reader boundary",
      "#2970": "typed model package and model identity",
      "#2971": "model execution provider",
    },
    runtime_claims: {
      made: false,
      policy: "Only a fresh build/execution may promote a static gap to a runtime defect; this report makes no such claim.",
    },
  };
}

function usage() {
  return "usage: node scripts/agent/boundary-census.mjs [--json]";
}

export function main(argv = process.argv.slice(2)) {
  try {
    if (argv.some((argument) => argument !== "--json")) throw new Error(usage());
    process.stdout.write(`${JSON.stringify(census(), null, 2)}\n`);
    return 0;
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
