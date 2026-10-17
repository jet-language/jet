#!/usr/bin/env node
// Compiler stop facts are a projection of the same composition's runtime
// sources. Follow named calls, including private helpers; never author rows.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseRuntimeExports } from "./runtime-route-abi.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const outputPath = "Compiler/JetFoundation/Source/Registry/RuntimeStopExports.jet";

// Preserve source offsets while masking comments/literal text. Ordinary code
// retains LF statement boundaries; interpolation uses the lexer's raw-token
// expression mode, so its LF is whitespace rather than a synthetic Semi.
function codeOnly(source) {
  const code = source.split("");
  function blank(start, end) {
    for (let at = start; at < end; ++at) if (source[at] !== "\n" && source[at] !== "\r") code[at] = " ";
  }
  function scan(start, interpolation = false) {
    let at = start, braces = 0;
    while (at < source.length) {
      if (source.startsWith("//", at)) {
        const end = source.indexOf("\n", at);
        const next = end < 0 ? source.length : end;
        blank(at, next); at = next;
      } else if (source.startsWith("/*", at)) {
        const begin = at;
        at += 2;
        let depth = 1;
        while (at < source.length && depth) {
          if (source.startsWith("/*", at)) { ++depth; at += 2; }
          else if (source.startsWith("*/", at)) { --depth; at += 2; }
          else ++at;
        }
        if (depth) throw new Error("unclosed runtime source comment");
        blank(begin, at);
      } else if (source[at] === '"' || source[at] === "'") {
        const quote = source[at], triple = quote === '"' && source.startsWith('"""', at);
        const delimiter = triple ? '"""' : quote;
        let literal = at;
        at += delimiter.length;
        while (at < source.length && !source.startsWith(delimiter, at)) {
          if (source[at] === "\\") { at += 2; }
          else if (quote === '"' && source.startsWith("{{", at)) { at += 2; }
          else if (quote === '"' && source[at] === "{") {
            blank(literal, at + 1);
            const expression = at + 1;
            at = scan(expression, true);
            for (let cursor = expression; cursor < at; ++cursor) {
              if (code[cursor] === "\n") code[cursor] = " ";
            }
            literal = at - 1;
          } else ++at;
        }
        if (at >= source.length) throw new Error("unclosed runtime source literal");
        at += delimiter.length;
        blank(literal, at);
      } else if (source[at] === "{") { ++braces; ++at; }
      else if (source[at] === "}") {
        if (interpolation && braces === 0) return at + 1;
        --braces; ++at;
      } else ++at;
    }
    if (interpolation) throw new Error("unclosed runtime source interpolation");
    return at;
  }
  scan(0);
  return code.join("");
}

// This projection supports nongeneric declarations and direct named calls.
// Foreign/member calls and named address-call transports are not semantic
// callback edges; their stop facts belong to the compiler's callback analysis.
function rejectUnsupportedCalls(code, path) {
  // Mirrors Expressions.jet parser_turbofish_ahead: the head `name<` and the
  // closing `>` must touch the call's `(`; spaced `a < b > (c)` is a comparison.
  for (const match of code.matchAll(/\b([A-Za-z_]\w*)<(?![<=])/g)) {
    let at = match.index + match[0].length, depth = 1;
    while (at < code.length && depth && !/[{};]/.test(code[at])) {
      if (code[at] === "<") ++depth;
      else if (code[at] === ">") --depth;
      ++at;
    }
    if (!depth && code[at] === "(") {
      throw new Error(`${path}: unsupported generic call ${match[1]}`);
    }
  }
  // Lexer/Terminators (Rust 633-669, Jet 444-463): )/] followed by a
  // physical LF and ( always gets Semi; LParen does not suppress insertion.
  // Interpolation code has already normalized LF because it uses lex_raw.
  if (/[)\]][^\S\n]*\(/.test(code)) throw new Error(`${path}: unsupported computed call`);
}

export function runtimeStopExports(sources) {
  const functions = new Map(), exports = new Set();
  for (const [path, source] of sources) {
    const code = codeOnly(source);
    rejectUnsupportedCalls(code, path);
    for (const name of parseRuntimeExports(code, path).keys()) {
      if (exports.has(name)) throw new Error(`duplicate runtime #Export(c) ${name}`);
      exports.add(name);
    }
    const declarations = [...code.matchAll(/\bfn\s+([A-Za-z_]\w*)\s*/g)];
    for (let index = 0; index < declarations.length; ++index) {
      const declaration = declarations[index], name = declaration[1];
      const limit = declarations[index + 1]?.index ?? code.length;
      let opener = declaration.index + declaration[0].length;
      if (code[opener] !== "(") throw new Error(`${path}: unsupported function declaration ${name}`);
      let at = opener + 1, depth = 1;
      while (at < limit && depth) {
        if (code[at] === "(") ++depth;
        else if (code[at] === ")") --depth;
        ++at;
      }
      if (depth) throw new Error(`${path}: unclosed ${name} parameters`);
      const parameters = code.slice(opener + 1, at - 1);
      while (at < limit && code[at] !== "{" && code[at] !== "=") ++at;
      if (code[at] === "=") continue; // Imported symbol, not a Jet body.
      if (at >= limit) throw new Error(`${path}: missing ${name} body`);
      const start = ++at;
      depth = 1;
      while (at < limit && depth) {
        if (code[at] === "{") ++depth;
        else if (code[at] === "}") --depth;
        ++at;
      }
      if (depth) throw new Error(`${path}: unclosed ${name} body`);
      if (functions.has(name)) throw new Error(`duplicate runtime function ${name}`);
      const body = code.slice(start, at - 1);
      const boundNames = new Set([
        ...[...parameters.matchAll(/\b([A-Za-z_]\w*)\s*:/g)].map((match) => match[1]),
        ...[...body.matchAll(/\b([A-Za-z_]\w*)\s*(?::=|::)/g)].map((match) => match[1]),
      ]);
      const calls = [];
      for (const call of body.matchAll(/\b([A-Za-z_]\w*)[^\S\n]*\(/g)) {
        // A member name is outside this source-level named-function graph.
        if (body.slice(0, call.index).trimEnd().endsWith(".")) continue;
        if (boundNames.has(call[1])) throw new Error(`${path}: unsupported bound-value call ${call[1]} in ${name}`);
        calls.push(call[1]);
      }
      functions.set(name, calls);
    }
  }
  if (!functions.has("scheduler_panic_publish_record")) throw new Error("missing runtime panic publication root");
  const callers = new Map();
  for (const [name, calls] of functions) {
    for (const callee of new Set(calls)) {
      if (!functions.has(callee)) continue;
      if (!callers.has(callee)) callers.set(callee, []);
      callers.get(callee).push(name);
    }
  }
  const reached = new Set(["scheduler_panic_publish_record"]), queue = [...reached];
  for (const callee of queue) {
    for (const caller of callers.get(callee) ?? []) {
      if (reached.has(caller)) continue;
      reached.add(caller); queue.push(caller);
    }
  }
  return [...exports].filter((name) => reached.has(name)).sort();
}

export function generatedRuntimeStopRegistry(symbols) {
  return `// Generated by Tools/agent/gen-runtime-stop-exports.mjs; do not edit.
// Named-call closure from scheduler_panic_publish_record in JetRuntime/Source/*.jet.
// Direct named calls only; foreign/dynamic callback effects are separate.
// Both compiler halves consume these Panic stop facts, not returned Results.
// The symbols land as literal data: no compile-time execution host is needed.
RUNTIME_STOP_EXPORTS :: prep { [String]{
${symbols.map((symbol) => `    "${symbol}",\n`).join("")}} }

fn runtime_export_can_stop(symbol: String) -> Bool Never! {
    loop candidate in RUNTIME_STOP_EXPORTS {
        if candidate == symbol -> return true
    }
    false
}
`;
}

export function checkRuntimeStopRegistry(actual, expected) {
  if (actual !== expected) throw new Error(`${outputPath} is stale; run node Tools/agent/gen-runtime-stop-exports.mjs --write`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const args = process.argv.slice(2);
  if (args.length > 1 || args.some((arg) => !["--check", "--print", "--write"].includes(arg))) {
    throw new Error("usage: gen-runtime-stop-exports.mjs [--check|--print|--write]");
  }
  const directory = join(root, "Compiler/JetRuntime/Source");
  const sources = readdirSync(directory).filter((name) => name.endsWith(".jet")).sort()
    .map((name) => [name, readFileSync(join(directory, name), "utf8")]);
  const text = generatedRuntimeStopRegistry(runtimeStopExports(sources));
  if (args[0] === "--print") process.stdout.write(text);
  else if (args[0] === "--check") checkRuntimeStopRegistry(readFileSync(join(root, outputPath), "utf8"), text);
  else writeFileSync(join(root, outputPath), text);
}
