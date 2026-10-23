#!/usr/bin/env node
// Render the generated lexical sections of the editor grammars.
//
// Highlights.jet (JET_HIGHLIGHT_TOKENS) and Prelude/Markers.jet (active
// `marker` rows) are the authority. This tool reads their Jet text and
// replaces the BEGIN/END GENERATED JET SYNTAX HIGHLIGHTS block in each grammar;
// it keeps no second token inventory.
//
// Usage: node Tools/agent/editor-grammars.mjs [--check|--write]

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SYNTAX_DIR = "Compiler/JetFoundation/Data/Syntax";
const HIGHLIGHTS = `${SYNTAX_DIR}/Highlights.jet`;
const MARKERS = "Compiler/Prelude/Markers.jet";
const TABLE_RE = /^JET_HIGHLIGHT_TOKENS :: \[HighlightToken\]\{[\s\S]*?\n\}/m;
const ROW_RE = /\{\s*text:\s*("(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z0-9_]*)\s*,\s*class:\s*HighlightClass\.([A-Za-z]+)\s*,?\s*\}/g;
const STRING_CONST_RE = /^([A-Za-z_][A-Za-z0-9_]*)\s*::\s*("(?:\\.|[^"\\])*")\s*(?:\/\/.*)?$/gm;

// Enum order of HighlightClass: tokens sort by class, then by text.
const CLASSES = [
  "KeywordControl",
  "KeywordDeclaration",
  "KeywordOwnership",
  "KeywordOther",
  "Literal",
  "TypeBuiltin",
  "Builtin",
  "MarkerRule",
  "Operator",
  "Sigil",
];
// Emission order: sigils before operators so `::` wins over `:`-led operators.
const EMIT_ORDER = [
  "KeywordControl",
  "KeywordDeclaration",
  "KeywordOwnership",
  "KeywordOther",
  "Literal",
  "TypeBuiltin",
  "Builtin",
  "MarkerRule",
  "Sigil",
  "Operator",
];
const KEYWORD_CLASSES = new Set(["KeywordControl", "KeywordDeclaration", "KeywordOwnership", "KeywordOther"]);

const TEXTMATE_SCOPE = {
  KeywordControl: "keyword.control.jet",
  KeywordDeclaration: "keyword.declaration.jet",
  KeywordOwnership: "keyword.other.ownership.jet",
  KeywordOther: "keyword.other.jet",
  Literal: "constant.language.jet",
  TypeBuiltin: "storage.type.builtin.jet",
  Builtin: "support.function.builtin.jet",
  MarkerRule: "entity.name.tag.rule.jet",
  Operator: "keyword.operator.jet",
  Sigil: "keyword.operator.sigil.jet",
};
const ZED_CAPTURE = {
  KeywordControl: "@keyword.control",
  KeywordDeclaration: "@keyword",
  KeywordOwnership: "@keyword",
  KeywordOther: "@keyword",
  Literal: "@constant.builtin",
  TypeBuiltin: "@type.builtin",
  Builtin: "@function.builtin",
  MarkerRule: "@attribute",
  Operator: "@operator",
  Sigil: "@operator",
};
const LABEL = {
  KeywordControl: "keyword.control",
  KeywordDeclaration: "keyword.declaration",
  KeywordOwnership: "keyword.ownership",
  KeywordOther: "keyword.other",
  Literal: "literal",
  TypeBuiltin: "type.builtin",
  Builtin: "builtin",
  MarkerRule: "marker.rule",
  Operator: "operator",
  Sigil: "sigil",
};
const TREE_SITTER_CONST = Object.fromEntries(
  CLASSES.map((cls) => [cls, `JET_HIGHLIGHT_${cls.replace(/([a-z])([A-Z])/g, "$1_$2").toUpperCase()}`]),
);

const BRACKETS = new Set(["{", "}", "[", "]", "(", ")"]);
// Lexer-reserved words stay global. Identifier-like declaration words
// (`add`, `client`, `as`, …) are colored only in declaration-shaped positions.
const GLOBAL_KEYWORDS = new Set([
  "fn", "pub", "priv", "if", "else", "in", "break", "true", "false", "mutate", "move", "copy",
  "struct", "enum", "impl", "trait", "tag", "effect", "derive", "self", "it", "null", "const",
  "comptime", "return", "loop", "yield", "use", "extern", "module",
]);
// Zed validates query string literals against anonymous tree-sitter tokens;
// words parsed as named nodes would make the whole language fail to load.
const ZED_ANONYMOUS_WORDS = new Set([
  "Bool", "Char", "Err", "F32", "F64", "Float", "I16", "I32", "I64", "I8", "Int", "List", "Map",
  "String", "Test", "U16", "U32", "U64", "U8", "add", "alias", "as", "break", "change", "comptime",
  "continue", "copy", "defer", "derive", "distinct", "else", "enum", "extern", "false", "fn", "if",
  "impl", "in", "loop", "marker", "migration", "module", "pub", "remove", "rename", "return", "rust",
  "self", "state", "step", "struct", "tag", "task", "trait", "true", "use", "via",
]);

export const START = "BEGIN GENERATED JET SYNTAX HIGHLIGHTS";
export const END = "END GENERATED JET SYNTAX HIGHLIGHTS";

function fail(message) {
  throw new Error(`editor grammars: ${message}`);
}

function readSource(path, root) {
  try {
    return readFileSync(resolve(root, path), "utf8");
  } catch (error) {
    fail(`could not read ${path}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

const JET_ESCAPES = { n: "\n", t: "\t", r: "\r", "0": "\0", "\\": "\\", '"': '"' };
function jetString(literal, field) {
  const body = literal.slice(1, -1);
  if (/[{}]/.test(body.replaceAll("{{", "").replaceAll("}}", ""))) {
    fail(`${field} string literal ${literal} interpolates; highlight tokens must be literal text`);
  }
  return body.replace(/\\(.)|\{\{|\}\}/g, (match, escaped) => {
    if (match === "{{") return "{";
    if (match === "}}") return "}";
    if (!(escaped in JET_ESCAPES)) fail(`unsupported escape \\${escaped} in ${field} string literal ${literal}`);
    return JET_ESCAPES[escaped];
  });
}

// Every `NAME :: "literal"` constant in the syntax registry directory.
function readSyntaxConstants(root) {
  const constants = new Map();
  for (const entry of readdirSync(resolve(root, SYNTAX_DIR)).sort()) {
    if (!entry.endsWith(".jet")) continue;
    const path = join(SYNTAX_DIR, entry);
    for (const match of readSource(path, root).matchAll(STRING_CONST_RE)) {
      const value = jetString(match[2], match[1]);
      const prior = constants.get(match[1]);
      if (prior !== undefined && prior.value !== value) {
        fail(`conflicting constant ${match[1]} in ${prior.path} and ${path}`);
      }
      constants.set(match[1], { path, value });
    }
  }
  return constants;
}

// Active rule names: every `marker Name(...)` row without a `$retired` fact.
export function readActiveMarkers(root = ROOT) {
  const names = [];
  for (const raw of readSource(MARKERS, root).split("\n")) {
    const line = raw.trim();
    if (!line.startsWith("marker ")) continue;
    const open = line.indexOf("(");
    if (open < 0) fail(`marker declaration without a parameter list: ${line}`);
    if (/\$retired\s*:/.test(line)) continue;
    names.push(line.slice("marker ".length, open).trim());
  }
  if (names.length === 0) fail(`no marker declarations in ${MARKERS}`);
  return names;
}

export function readHighlightTokens(root = ROOT) {
  const source = readSource(HIGHLIGHTS, root);
  const table = source.match(TABLE_RE)?.[0];
  if (!table) fail(`missing ${HIGHLIGHTS}::JET_HIGHLIGHT_TOKENS table`);
  const constants = readSyntaxConstants(root);
  const tokens = [];
  for (const match of table.matchAll(ROW_RE)) {
    let text;
    if (match[1].startsWith('"')) {
      text = jetString(match[1], "text");
    } else {
      const constant = constants.get(match[1]);
      if (!constant) fail(`unresolved highlight text constant ${match[1]} in ${SYNTAX_DIR}`);
      text = constant.value;
    }
    if (!CLASSES.includes(match[2])) fail(`unknown HighlightClass.${match[2]} in ${HIGHLIGHTS}`);
    tokens.push({ text, cls: match[2] });
  }
  const literalCount = (table.match(/\btext:/g) ?? []).length;
  if (tokens.length === 0 || tokens.length !== literalCount) {
    fail(`could not parse every HighlightToken row in ${HIGHLIGHTS} (parsed ${tokens.length}, found ${literalCount})`);
  }
  for (const name of readActiveMarkers(root)) tokens.push({ text: name, cls: "MarkerRule" });
  return tokens;
}

const byText = (a, b) => (a < b ? -1 : a > b ? 1 : 0);

function classTexts(tokens, cls) {
  return [...new Set(tokens.filter((t) => t.cls === cls && !BRACKETS.has(t.text)).map((t) => t.text))].sort(byText);
}

const isWordToken = (s) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(s);
const regexEscape = (s) => s.replace(/[\\.+*?^$()[\]{}|/-]/g, "\\$&");

function pushVscodePattern(patterns, scope, pattern) {
  // JSON string escaping; token text is ASCII.
  patterns.push(`        {\n          "name": "${scope}",\n          "match": ${JSON.stringify(pattern)}\n        }`);
}

export function renderVscode(tokens) {
  const patterns = [];
  for (const cls of EMIT_ORDER) {
    const texts = classTexts(tokens, cls);
    const words = texts.filter(isWordToken).map(regexEscape);
    const symbols = texts.filter((s) => !isWordToken(s)).map(regexEscape);
    const keyword = KEYWORD_CLASSES.has(cls);
    const global = keyword ? words.filter((w) => GLOBAL_KEYWORDS.has(w)) : words;
    const contextual = keyword ? words.filter((w) => !GLOBAL_KEYWORDS.has(w)) : [];
    const scope = TEXTMATE_SCOPE[cls];
    if (global.length > 0) {
      const alternation = global.join("|");
      let pattern;
      if (cls === "MarkerRule") {
        // Applied rules are `#Error` / `#[Codable]` / `@allow`, not bare names.
        pattern = `#\\[?(${alternation})\\b`;
      } else if (cls === "Builtin") {
        pattern = `(?<![.A-Za-z0-9_])(${alternation})(?=\\s*\\()`;
      } else if (keyword) {
        pattern = `(?<![.A-Za-z0-9_])(${alternation})(?![A-Za-z0-9_])`;
      } else {
        pattern = `\\b(${alternation})\\b`;
      }
      pushVscodePattern(patterns, scope, pattern);
    }
    if (contextual.length > 0) {
      // Identifier-like keywords must not steal ordinary names; color only
      // declaration shape.
      if (contextual.includes("as")) {
        pushVscodePattern(patterns, scope, "(?<![.A-Za-z0-9_])(as)(?=\\s+[A-Za-z_][A-Za-z0-9_]*)");
      }
      const rest = contextual.filter((w) => w !== "as");
      if (rest.length > 0) {
        pushVscodePattern(
          patterns,
          scope,
          `(?<![.A-Za-z0-9_])(${rest.join("|")})(?=\\s*(?:\\{|[A-Za-z_][A-Za-z0-9_]*\\s*:))`,
        );
      }
    }
    if (symbols.length > 0) pushVscodePattern(patterns, scope, `(${symbols.join("|")})`);
  }
  return `      "comment": "${START}",\n      "patterns": [\n${patterns.join(",\n")}\n      ],\n      "endComment": "${END}"\n`;
}

export function renderTreeSitter(tokens) {
  let out = `// ${START}\n`;
  for (const cls of EMIT_ORDER) {
    out += `const ${TREE_SITTER_CONST[cls]} = [${classTexts(tokens, cls).map((s) => JSON.stringify(s)).join(", ")}];\n`;
  }
  return `${out}// ${END}\n`;
}

export function renderZed(tokens) {
  let out = `; ${START}\n`;
  for (const cls of EMIT_ORDER) {
    const texts = classTexts(tokens, cls);
    out += `; ${LABEL[cls]}: ${texts.join(" ")}\n`;
    if (cls === "MarkerRule") continue;
    const words = texts
      .filter((s) => isWordToken(s) && ZED_ANONYMOUS_WORDS.has(s))
      .filter((s) => !KEYWORD_CLASSES.has(cls) || GLOBAL_KEYWORDS.has(s))
      .map((s) => `  ${JSON.stringify(s)}`);
    if (words.length > 0) out += `[\n${words.join("\n")}\n] ${ZED_CAPTURE[cls]}\n\n`;
  }
  return `${out}; ${END}\n`;
}

export const TARGETS = [
  { path: "Tools/editors/vscode/syntaxes/jet.tmLanguage.json", render: renderVscode },
  { path: "Tools/editors/jet.tmGrammar", render: renderVscode },
  { path: "Tools/editors/tree-sitter/grammar.js", render: renderTreeSitter },
  { path: "Tools/editors/zed/languages/jet/highlights.scm", render: renderZed },
];

// Replace from the start of the START marker's line through the end of the
// END marker's line.
export function replaceSection(text, fresh, path) {
  const start = text.indexOf(START);
  if (start < 0) fail(`${path} has no \`${START}\` marker`);
  const endRel = text.indexOf(END, start);
  if (endRel < 0) fail(`${path} has no \`${END}\` marker`);
  const prefixEnd = text.lastIndexOf("\n", start) + 1;
  const endMarker = endRel + END.length;
  const newline = text.indexOf("\n", endMarker);
  const suffixStart = newline < 0 ? text.length : newline + 1;
  return `${text.slice(0, prefixEnd)}${fresh.trimEnd()}\n${text.slice(suffixStart)}`;
}

function usage() {
  return [
    "usage: node Tools/agent/editor-grammars.mjs [--check|--write]",
    "",
    "  --check  fail when a generated grammar section is stale (default)",
    "  --write  regenerate the sections in place",
  ].join("\n");
}

export function main(argv = process.argv.slice(2), root = ROOT) {
  const write = argv.includes("--write");
  const unknown = argv.filter((arg) => arg !== "--write" && arg !== "--check");
  if (unknown.length > 0 || (write && argv.includes("--check"))) {
    console.error(usage());
    return 2;
  }
  let tokens;
  try {
    tokens = readHighlightTokens(root);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    return 1;
  }
  const stale = [];
  for (const target of TARGETS) {
    const file = resolve(root, target.path);
    let current;
    let next;
    try {
      current = readFileSync(file, "utf8");
      next = replaceSection(current, target.render(tokens), target.path);
    } catch (error) {
      console.error(error instanceof Error ? error.message : String(error));
      return 1;
    }
    if (next === current) continue;
    if (write) {
      writeFileSync(file, next);
      console.log(`wrote ${target.path}`);
    } else {
      stale.push(target.path);
    }
  }
  if (stale.length > 0) {
    for (const path of stale) console.error(`editor grammars: stale generated section in ${path}`);
    console.error("run: node Tools/agent/editor-grammars.mjs --write");
    return 1;
  }
  if (!write) console.log(`editor grammars: ${TARGETS.length} generated sections current`);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main();
}
