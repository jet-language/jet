#!/usr/bin/env node
// The `jet` command table has one source: Compiler/JetCli/Source/Cli/Commands.jet
// (D-JETCLI-HOST1=A). The Rust host keeps a frozen copy in
// crates/jet-cli/src/CLI.rs (`HandlerKey`, `InspectPlane`, the `*_ACTIONS`
// arrays, `COMMANDS`, and `BASE_FLAGS`) until the Jet CLI replaces it. This
// tool reads both tables as data and fails when they differ in any command,
// nested action, dispatcher seam, flag, or order.
//
// usage: node Compiler/Bootstrap/check-command-table.mjs
//   exit 0 when the tables agree; exit 1 with the first differences otherwise.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const JET_TABLE = "Compiler/JetCli/Source/Cli/Commands.jet";
export const RUST_TABLE = "crates/jet-cli/src/CLI.rs";

function fail(message) {
  throw new Error(`command-table: ${message}`);
}

// One tokenizer for both spellings. Strings decode Rust escapes or Jet
// escapes (`{{`/`}}` are literal braces; a bare brace would interpolate).
function tokenize(text, language) {
  const tokens = [];
  let index = 0;
  while (index < text.length) {
    const character = text[index];
    if (/\s/.test(character)) { index += 1; continue; }
    if (text.startsWith("//", index)) {
      const end = text.indexOf("\n", index);
      const stop = end < 0 ? text.length : end;
      tokens.push({ kind: "comment", value: text.slice(index, stop) });
      index = stop;
      continue;
    }
    if (character === '"') {
      let value = "";
      index += 1;
      for (;;) {
        if (index >= text.length) fail("unterminated string literal");
        const next = text[index];
        if (next === '"') { index += 1; break; }
        if (next === "\\") {
          const escape = text[index + 1];
          index += 2;
          if (escape === "n") value += "\n";
          else if (escape === "t") value += "\t";
          else if (escape === "r") value += "\r";
          else if (escape === "0") value += "\0";
          else if (escape === "\\" || escape === '"' || escape === "'") value += escape;
          else if (escape === "u" && text[index] === "{") {
            const close = text.indexOf("}", index);
            value += String.fromCodePoint(parseInt(text.slice(index + 1, close), 16));
            index = close + 1;
          } else if (escape === "\n" && language === "rust") {
            while (/\s/.test(text[index])) index += 1;
          } else fail(`unsupported escape \\${escape}`);
          continue;
        }
        if (language === "jet" && (next === "{" || next === "}")) {
          if (text[index + 1] !== next) fail(`Jet string has an interpolation brace: ${text.slice(index, index + 20)}`);
          value += next;
          index += 2;
          continue;
        }
        value += next;
        index += 1;
      }
      tokens.push({ kind: "string", value });
      continue;
    }
    const word = /^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*/.exec(text.slice(index));
    if (word) { tokens.push({ kind: "ident", value: word[0] }); index += word[0].length; continue; }
    const number = /^[0-9][0-9_]*/.exec(text.slice(index));
    if (number) { tokens.push({ kind: "number", value: Number(number[0].replace(/_/g, "")) }); index += number[0].length; continue; }
    tokens.push({ kind: "punct", value: character });
    index += 1;
  }
  return tokens;
}

// Literal grammar shared by both tables: strings, numbers, booleans,
// `None`/`Some(x)`/`Val(x)`, struct literals `Name { field: value }`, Rust
// slices `&[ ... ]`, Jet lists `[Type]{ ... }`, calls `f(x)`, and paths.
// Comments directly before a list item travel with that item.
class Parser {
  constructor(tokens, constants) {
    this.tokens = tokens;
    this.index = 0;
    this.constants = constants;
  }
  peek(offset = 0) { return this.tokens[this.index + offset]; }
  comments() {
    const lines = [];
    while (this.peek()?.kind === "comment") lines.push(this.tokens[this.index++].value);
    return lines;
  }
  expect(value) {
    this.comments();
    const token = this.tokens[this.index++];
    if (!token || token.value !== value) fail(`expected \`${value}\`, found \`${token?.value}\``);
  }
  eat(value) {
    this.comments();
    if (this.peek()?.value === value) { this.index += 1; return true; }
    return false;
  }
  list(close) {
    const items = [];
    for (;;) {
      const comments = this.comments();
      if (this.eat(close)) return items;
      const item = this.value();
      if (comments.length > 0 && item && typeof item === "object") item.comments = comments;
      items.push(item);
      if (!this.eat(",")) { this.expect(close); return items; }
    }
  }
  value() {
    this.comments();
    const token = this.tokens[this.index++];
    if (!token) fail("unexpected end of table");
    if (token.kind === "string" || token.kind === "number") return token.value;
    if (token.kind === "punct" && token.value === "&") { this.expect("["); return { list: this.list("]") }; }
    if (token.kind === "punct" && token.value === "[") {
      const type = this.tokens[this.index++].value;
      this.expect("]");
      this.expect("{");
      return { list: this.list("}"), type };
    }
    if (token.kind !== "ident") fail(`unexpected \`${token.value}\``);
    if (token.value === "true" || token.value === "false") return token.value === "true";
    if (token.value === "None") return null;
    if (token.value === "Some" || token.value === "Val") {
      this.expect("(");
      const inner = this.value();
      this.eat(",");
      this.expect(")");
      return inner;
    }
    if (this.eat("{")) {
      const fields = {};
      for (;;) {
        if (this.eat("}")) break;
        const name = this.tokens[this.index++].value;
        this.expect(":");
        fields[name] = this.value();
        if (!this.eat(",")) { this.expect("}"); break; }
      }
      return { struct: token.value, fields };
    }
    if (this.eat("(")) return { call: token.value, args: this.list(")") };
    if (token.value in this.constants) return this.constants[token.value];
    return { path: token.value };
  }
}

export function literalAt(text, marker, language, constants = {}) {
  const start = text.indexOf(marker);
  if (start < 0) fail(`missing \`${marker}\``);
  const parser = new Parser(tokenize(text.slice(start + marker.length), language), constants);
  return parser.value();
}

function stringField(row, name, where) {
  const value = row.fields[name];
  if (typeof value !== "string") fail(`${where}: field \`${name}\` is not a string`);
  return value;
}

function boolField(row, name, where) {
  const value = row.fields[name];
  if (typeof value !== "boolean") fail(`${where}: field \`${name}\` is not a Bool`);
  return value;
}

// The `match self { Self::X => "..." }` arms of one InspectPlane/HandlerKey method.
function matchArms(text, signature) {
  const start = text.indexOf(signature);
  if (start < 0) fail(`missing \`${signature}\``);
  const body = text.slice(start, text.indexOf("\n    }\n", start));
  const arms = new Map();
  for (const match of body.matchAll(/Self::(\w+) => "((?:[^"\\]|\\.)*)"/g)) arms.set(match[1], match[2]);
  return arms;
}

function enumVariants(text, name) {
  const start = text.indexOf(`pub enum ${name} {`);
  if (start < 0) fail(`missing \`pub enum ${name}\``);
  const body = text.slice(start, text.indexOf("\n}", start));
  return [...body.matchAll(/^    (\w+),$/gm)].map((match) => match[1]);
}

export function readRustTable(text) {
  const constants = {};
  for (const match of text.matchAll(/^(?:pub )?const ([A-Z][A-Z0-9_]*): &str = "((?:[^"\\]|\\.)*)";/gm)) constants[match[1]] = match[2];
  const planes = new Map();
  const planeNames = matchArms(text, "pub const fn name(self)");
  const planeUsages = matchArms(text, "pub const fn usage(self)");
  const planeSummaries = matchArms(text, "pub const fn summary(self)");
  for (const variant of enumVariants(text, "InspectPlane")) {
    planes.set(variant, { name: planeNames.get(variant), usage: planeUsages.get(variant), summary: planeSummaries.get(variant), handler: "InspectPlane", also_canonical_top_level: false });
  }
  const words = matchArms(text, "pub const fn dispatch_word(self)");
  const keepsStart = text.indexOf("pub const fn keeps_group(self)");
  const keepsBody = text.slice(keepsStart, text.indexOf("\n    }\n", keepsStart));
  const keeps = new Set([...keepsBody.matchAll(/Self::(\w+)/g)].map((match) => match[1]));
  const handlers = enumVariants(text, "HandlerKey").map((key) => {
    if (!words.has(key)) fail(`HandlerKey::${key} has no dispatch word`);
    return { key, dispatch_word: words.get(key), keeps_group: keeps.has(key) };
  });
  const action = (value, where) => {
    if (value.call === "inspect_plane_action") {
      const plane = planes.get(value.args[0].path.replace("InspectPlane::", ""));
      if (!plane) fail(`${where}: unknown inspect plane`);
      return { ...plane };
    }
    return {
      name: stringField(value, "name", where),
      usage: stringField(value, "usage", where),
      summary: stringField(value, "summary", where),
      handler: value.fields.handler.path.replace("HandlerKey::", ""),
      also_canonical_top_level: boolField(value, "also_canonical_top_level", where),
    };
  };
  const actionArrays = {};
  for (const match of text.matchAll(/^const ([A-Z][A-Z0-9_]*)_ACTIONS: &\[NestedCommandSpec\] = /gm)) {
    actionArrays[`${match[1]}_ACTIONS`] = literalAt(text, match[0], "rust", constants).list.map((value) => action(value, `${match[1]}_ACTIONS`));
  }
  const commands = literalAt(text, "pub const COMMANDS: &[CommandSpec] = ", "rust", constants).list.map((row) => {
    const name = stringField(row, "name", "COMMANDS");
    const actions = row.fields.actions.path ? actionArrays[row.fields.actions.path] : row.fields.actions.list.map((value) => action(value, name));
    if (!actions) fail(`${name}: unknown action array`);
    return {
      name,
      summary: stringField(row, "summary", name),
      headline: boolField(row, "headline", name),
      frequency_rank: row.fields.frequency_rank,
      exhaustive: boolField(row, "exhaustive", name),
      usage: row.fields.usage,
      actions,
    };
  });
  const flags = literalAt(text, "const BASE_FLAGS: &[FlagSpec] = ", "rust", constants).list.map((row) => ({
    long: stringField(row, "long", "BASE_FLAGS"),
    help: stringField(row, "help", "BASE_FLAGS"),
  }));
  return { handlers, commands, flags };
}

export function readJetTable(text) {
  const handlers = literalAt(text, "JET_CLI_HANDLERS :: prep { ", "jet").list;
  const commands = literalAt(text, "JET_CLI_COMMANDS :: prep { ", "jet").list;
  const flags = literalAt(text, "JET_CLI_FLAGS :: prep { ", "jet").list;
  return {
    handlers: handlers.map((row) => ({
      key: stringField(row, "key", "JET_CLI_HANDLERS"),
      dispatch_word: stringField(row, "dispatch_word", "JET_CLI_HANDLERS"),
      keeps_group: boolField(row, "keeps_group", "JET_CLI_HANDLERS"),
    })),
    commands: commands.map((row) => {
      const name = stringField(row, "name", "JET_CLI_COMMANDS");
      return {
        name,
        summary: stringField(row, "summary", name),
        headline: boolField(row, "headline", name),
        frequency_rank: row.fields.frequency_rank,
        exhaustive: boolField(row, "exhaustive", name),
        usage: row.fields.usage,
        actions: row.fields.actions.list.map((value) => ({
          name: stringField(value, "name", name),
          usage: stringField(value, "usage", name),
          summary: stringField(value, "summary", name),
          handler: stringField(value, "handler", name),
          also_canonical_top_level: boolField(value, "also_canonical_top_level", name),
        })),
      };
    }),
    flags: flags.map((row) => ({
      long: stringField(row, "long", "JET_CLI_FLAGS"),
      help: stringField(row, "help", "JET_CLI_FLAGS"),
    })),
  };
}

// Section-by-section differences, each row named by its key.
export function tableDifferences(jet, rust) {
  const differences = [];
  for (const [section, key] of [["handlers", "key"], ["commands", "name"], ["flags", "long"]]) {
    const length = Math.max(jet[section].length, rust[section].length);
    for (let index = 0; index < length; index += 1) {
      const left = JSON.stringify(jet[section][index] ?? null);
      const right = JSON.stringify(rust[section][index] ?? null);
      if (left !== right) {
        differences.push(`${section}[${index}] (${jet[section][index]?.[key] ?? "missing"} vs ${rust[section][index]?.[key] ?? "missing"})\n  Jet:  ${left}\n  Rust: ${right}`);
      }
    }
  }
  return differences;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const jet = readJetTable(readFileSync(resolve(repoRoot, JET_TABLE), "utf8"));
  const rust = readRustTable(readFileSync(resolve(repoRoot, RUST_TABLE), "utf8"));
  const differences = tableDifferences(jet, rust);
  if (differences.length > 0) {
    console.error(`command-table: ${JET_TABLE} and the frozen Rust copy in ${RUST_TABLE} differ:\n${differences.slice(0, 20).join("\n")}`);
    process.exit(1);
  }
  console.log(`command-table: ${jet.commands.length} commands, ${jet.commands.reduce((sum, command) => sum + command.actions.length, 0)} actions, ${jet.handlers.length} handlers, ${jet.flags.length} flags agree`);
}
