#!/usr/bin/env node
import { runtimeAbiAtom, emitRuntimeAbiAtom } from "./runtime-abi-symbols.mjs";
// Generate the runtime's C-ABI route layer (D-EXEC1, I8) from the Core call
// registry: one `#[no_mangle] extern "C"` adapter per Prelude route whose Rust
// kernel signature has a C carrier, plus the backend's matching route table.
//
// usage: node Tools/agent/gen-c-abi-routes.mjs <runtime.rs> [--check]
//   <runtime.rs> is the compiled runtime's Rust text as emitted for the
//   all-parts seed (Compiler/JetBackend/Tests/emit-allparts-runtime.mjs <jet> <out.rs>;
//   emission selects runtime parts by the Core modules a program uses, and the
//   seed imports every one): the exact items the `jet_runtime` crate holds, so
//   every kernel's module path and signature come from the runtime itself,
//   not from a second list.
//
// Routes (the registry):
//   - crates/jet-foundation/src/Syntax/core_calls.rs: every CoreCallRecord row,
//     keyed by its symbol (a Prelude or a Rust-crate symbol: both are one
//     runtime function generated Rust calls), except a non-direct row that
//     MIRRust.rs `core_call` / `source_math_call` emits in a form of its own;
//     a symbol MIRRust.rs `exact_prelude_adapter` maps to an owned exact-Int
//     kernel calls that kernel;
//   - crates/jet-codegen/src/Codegen/TIR/routes.rs: every StaticPrelude route
//     row (`prelude_route_row(MirPreludeFamily::StaticPrelude, ...)`), builtin
//     and handle method rows, and every other Prelude symbol the TIR sources
//     spell that names a runtime function (an inherent method included).
//
// Outputs:
//   - crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs: the adapters, module
//     `jet_c_abi_routes`, appended after CAbi.rs only in the all-parts native
//     runtime crate (Compiler/JetBackend/Tests/prepare-call-runtime.mjs --allparts);
//   - Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet: the carrier table
//     the lowering checks a call against before it calls the adapter.
//
// Carriers (Docs/research/jet-backend-design-2026-10-01.md, "Calls into the
// compiled runtime"): Int is the exact-Int carrier word (kernels taking a
// fixed-width integer get `native` + a checked narrowing, the way generated
// Rust adapts them); Bool is a 0/1 word; Float is xmm; String is a borrowed
// pointer to the C-layout `JetCText {ptr,len,cap}` header (a result owns a new
// header and buffer in the shared runtime allocation domain); a Jet type generated
// Rust holds as a native runtime type (its Context.rs / predicates.rs name
// tables) is an owned `Box<T>` handle (`jet_rt_handle_drop_<T>` frees it), a
// borrowed one as a parameter and a lent mutable one (`HandleMut`) for
// `&mut T`. `Option<T>` and `Result<T, E>` results return the tag (1 for
// Some/Ok) and write the payload through one out pointer per non-Unit side; a
// tuple writes each item through its own out pointer; the lowering builds its
// own value.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { generatedRecordPath, nativeProviderHandles, recordApplication, recordApplicationsIn, recordBindings, recordInstanceName, substituteRecordType, runtimeExportSignatures, validateRuntimeRoute } from "./runtime-route-abi.mjs";
import { nativeEnvironmentRoutes, emitNativeEnvironmentRoutes } from "./native-route-environment.mjs";
import { nativeHandleExports } from "./native-route-handles.mjs";
import { nativeRecordExports } from "./native-route-records.mjs";
import { excludeNativeSeedBodies, nativeSeedOnlyExports, sourceOwnedAdapterProofs } from "./native-route-partition.mjs";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const runtimePath = process.argv[2];
const check = process.argv.includes("--check");
const nativeListOnly = process.argv.includes("--native-list-only");
if (!runtimePath) {
  console.error("usage: gen-c-abi-routes.mjs <runtime.rs> [--check | --native-list-only]");
  process.exit(64);
}
const rsOut = "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs";
const jetOut = "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet";

// ---------------------------------------------------------------------------
// 1. The registry's routes: symbol -> { native_int_result, rows }.

// The body of `fn name(` in Rust `source`, through its matching brace.
const fnBody = (source, name) => {
  const at = source.search(new RegExp(`\\bfn ${name}\\(`));
  if (at < 0) throw new Error(`gen-c-abi-routes: no fn ${name}`);
  const open = source.indexOf("{", at);
  let depth = 0;
  for (let k = open; k < source.length; k++) {
    if (source[k] === '"') { k++; while (source[k] !== '"') k += source[k] === "\\" ? 2 : 1; continue; }
    if (source[k] === "{") depth++;
    else if (source[k] === "}" && --depth === 0) return source.slice(open + 1, k);
  }
  throw new Error(`gen-c-abi-routes: unterminated fn ${name}`);
};
const mirRustRs = readFileSync(`${repo}/crates/jet-codegen/src/Codegen/MIRRust.rs`, "utf8");
const stringsIn = (s) => [...s.matchAll(/"([^"]*)"/g)].map((m) => m[1]);

// The Core rows generated Rust (MIRRust.rs `core_call`) emits through a form
// of their own instead of the row's Prelude symbol, whatever the call's type
// arguments or arity: a route to that symbol would be a second meaning (I9).
// A form chosen by the call's type arguments or arity leaves the plain call
// on the symbol, so its row keeps a route.
const ownForm = new Set();
for (const m of fnBody(mirRustRs, "core_call").matchAll(/\bif (row\.module == "([^"]+)"[^{]*)\{/g)) {
  const condition = m[1];
  if (/type_args|args\.len\(\)/.test(condition)) continue;
  const members = [...condition.matchAll(/row\.member == "([^"]+)"/g)].map((k) => k[1]);
  const listed = /matches!\(\s*row\.member\.as_str\(\),([^)]*)\)/.exec(condition);
  if (listed) members.push(...stringsIn(listed[1]));
  for (const member of members) ownForm.add(`${m[2]}.${member}`);
}
// `source_math_call`: a non-direct core.math row it lists calls its Jet source
// function, never a runtime kernel.
const sourceMath = /row\.module != "core\.math"[\s\S]*?!matches!\(\s*row\.member\.as_str\(\),([^)]*)\)/.exec(fnBody(mirRustRs, "source_math_call"));
if (!sourceMath || ownForm.size === 0) throw new Error("gen-c-abi-routes: MIRRust core_call / source_math_call forms not found");
const sourceMathMembers = new Set(stringsIn(sourceMath[1]));

// MIRRust.rs `exact_prelude_adapter`: a Prelude symbol generated Rust calls as
// its owned exact-Int kernel, the listed argument positions borrowed as
// `&JetInt`. The route keeps the symbol; its adapter calls the kernel.
const exactAdapters = new Map();
for (const m of fnBody(mirRustRs, "exact_prelude_adapter").matchAll(/"([^"]+)"\s*=>\s*\{?\s*\("([^"]+)",\s*&\[[^\]]*\]\)/g)) exactAdapters.set(m[1], m[2]);
if (exactAdapters.size === 0) throw new Error("gen-c-abi-routes: no exact_prelude_adapter rows");

const routes = new Map();
// Canonical records decide ownership even when their value is false. A TIR
// handle/builtin route identifies a call, not a different provider ABI.
const canonicalIntResults = new Map();
const addRoute = (symbol, nativeInt, row) => {
  const entry = routes.get(symbol) ?? { nativeInt: false, rows: [] };
  entry.nativeInt = canonicalIntResults.get(symbol) ?? (entry.nativeInt || nativeInt);
  entry.rows.push(row);
  routes.set(symbol, entry);
};
// Every Core call row: generated Rust calls a direct row's symbol, and a
// non-direct row's symbol too unless `core_call` gives it its own form.
const coreCalls = readFileSync(`${repo}/crates/jet-foundation/src/Syntax/core_calls.rs`, "utf8");
for (const line of coreCalls.split("\n")) {
  const plain = /^\s*CoreCallRecord::new\(\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]*)",\s*(?:true|false)/.exec(line);
  const receiver = /^\s*CoreCallRecord::receiver_with_symbol\(\s*&\[([^\]]*)\],\s*"([^"]*)",\s*"([^"]*)",\s*(?:true|false)/.exec(line);
  if (!plain && !receiver) continue;
  const [, module, member, symbol] = plain ?? receiver;
  if (!symbol) continue;
  const nativeInt = line.includes(".with_native_int_result()");
  if (canonicalIntResults.has(symbol) && canonicalIntResults.get(symbol) !== nativeInt) {
    throw new Error(`gen-c-abi-routes: conflicting native_int_result records for ${symbol}`);
  }
  canonicalIntResults.set(symbol, nativeInt);
  if (line.includes(".without_direct_aot()")) {
    if (ownForm.has(`${module}.${member}`)) continue;
    if (module === "core.math" && sourceMathMembers.has(member)) continue;
  }
  addRoute(symbol, nativeInt, `${module}.${member}`);
}
const tirRoutes = readFileSync(`${repo}/crates/jet-codegen/src/Codegen/TIR/routes.rs`, "utf8");
for (const match of tirRoutes.matchAll(/prelude_route_row\(\s*MirPreludeFamily::StaticPrelude,\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]*)"/g)) {
  addRoute(match[3], false, `${match[1]}.${match[2]}`);
}
// Builtin method routes (`b(member, symbol, ...)` in the TBuiltinOp route
// table). Rows without a canonical provider record retain their native-word
// result convention; a record's native_int_result overrides it in addRoute.
for (const match of tirRoutes.matchAll(/\bb\(\s*"([^"]*)",\s*"([^"]*)",/g)) {
  addRoute(match[2], true, `core.builtin.${match[1]}`);
}
// Handle method routes (`h(member, symbol, ...)` rows and
// `prelude(MirPreludeFamily::HandleMethod, module, member, symbol, ...)`).
// These fallback rows do not override a canonical provider record, including
// an already-owned tagged Int result.
for (const match of tirRoutes.matchAll(/\bh\(\s*"([^"]*)",\s*"([^"]*)",/g)) {
  addRoute(match[2], true, `core.handle.${match[1]}`);
}
for (const match of tirRoutes.matchAll(/prelude\(\s*MirPreludeFamily::HandleMethod,\s*"([^"]*)",\s*"([^"]*)",\s*(?:if [^{]*\{\s*"([^"]*)"\s*\}\s*else\s*\{\s*"([^"]*)"\s*\}|"([^"]*)")/g)) {
  for (const symbol of [match[3], match[4], match[5]].filter(Boolean)) addRoute(symbol, true, `${match[1]}.${match[2]}`);
}

// ---------------------------------------------------------------------------
// 2. The runtime's functions: module path, visibility, signature, cfg.

// Discover declarations emitted by invoked, non-repeating macro rules before
// scanning signatures. Declarations keep the invocation's module and cfg:
// no API/arithmetic signature table is duplicated in the generator.
const macroTokens = (source) => source.match(/"(?:\\.|[^"\\])*"|[A-Za-z_][A-Za-z0-9_]*|[0-9]+|::|=>|[^\s]/g) ?? [];
const rustGroup = (source, start) => {
  const closes = { "(": ")", "{": "}", "[": "]" };
  const stack = [closes[source[start]]];
  if (!stack[0]) return null;
  for (let end = start + 1; end < source.length; end++) {
    if (source.startsWith("//", end)) { end = source.indexOf("\n", end); if (end < 0) break; continue; }
    if (source.startsWith("/*", end)) {
      let depth = 1; end += 2;
      while (end < source.length && depth) {
        if (source.startsWith("/*", end)) { depth++; end += 2; }
        else if (source.startsWith("*/", end)) { depth--; end += 2; }
        else end++;
      }
      end--;
      continue;
    }
    const raw = /^(?:b)?r(#+)?"/.exec(source.slice(end, end + 32));
    if (raw) {
      const close = `"${raw[1] ?? ""}`;
      const last = source.indexOf(close, end + raw[0].length);
      if (last < 0) throw new Error("gen-c-abi-routes: unterminated macro raw string");
      end = last + close.length - 1;
      continue;
    }
    if (source[end] === '"') {
      end++;
      while (end < source.length && source[end] !== '"') end += source[end] === "\\" ? 2 : 1;
      continue;
    }
    if (source[end] === "'") {
      const character = /^'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'])'/.exec(source.slice(end, end + 16));
      if (character) { end += character[0].length - 1; continue; }
    }
    if (closes[source[end]]) stack.push(closes[source[end]]);
    else if (source[end] === stack[stack.length - 1]) {
      stack.pop();
      if (!stack.length) return { body: source.slice(start + 1, end), end };
    }
  }
  throw new Error("gen-c-abi-routes: unterminated macro token group");
};
const expandDeclarations = (source) => {
  const macros = new Map(), definitions = [];
  for (const declaration of source.matchAll(/\bmacro_rules!\s*([A-Za-z_]\w*)\s*\{/g)) {
    const group = rustGroup(source, declaration.index + declaration[0].length - 1);
    definitions.push([declaration.index, group.end + 1]);
    const rules = [];
    for (let at = 0; at < group.body.length;) {
      const next = /\S/.exec(group.body.slice(at));
      if (!next) break;
      at += next.index;
      if (group.body[at] === ";") { at++; continue; }
      const pattern = rustGroup(group.body, at);
      if (!pattern) break;
      const arrow = /^\s*=>\s*/.exec(group.body.slice(pattern.end + 1));
      if (!arrow) break;
      const body = rustGroup(group.body, pattern.end + 1 + arrow[0].length);
      if (!body) break;
      if (!/\$\s*\(/.test(pattern.body))
        rules.push({ pattern: macroTokens(pattern.body), body: body.body });
      at = body.end + 1;
    }
    macros.set(declaration[1], rules);
  }
  const match = (pattern, input) => {
    const bindings = new Map();
    let i = 0;
    for (let p = 0; p < pattern.length; p++) {
      if (pattern[p] === "$" && pattern[p + 2] === ":") {
        const name = pattern[p + 1], kind = pattern[p + 3], token = input[i];
        if (!token || !["ident", "ty", "expr", "literal", "path"].includes(kind)) return null;
        if (kind === "ident" && !/^[A-Za-z_]\w*$/.test(token)) return null;
        const value = [token];
        i++;
        if (kind === "path" || kind === "ty")
          while (input[i] === "::" && /^[A-Za-z_]\w*$/.test(input[i + 1] ?? "")) value.push(input[i++], input[i++]);
        if (bindings.has(name) && bindings.get(name) !== value.join("")) return null;
        bindings.set(name, value.join(""));
        p += 3;
      } else if (pattern[p] !== input[i++]) return null;
    }
    return i === input.length ? bindings : null;
  };
  const expand = (input, depth = 0) => {
    if (depth > 32) throw new Error("gen-c-abi-routes: macro declaration expansion depth exceeded");
    const edits = [];
    for (const invocation of input.matchAll(/\b([A-Za-z_]\w*)!\s*([({[])/g)) {
      if (edits.some((edit) => invocation.index < edit.end)) continue;
      const rules = macros.get(invocation[1]);
      if (!rules?.length) continue;
      const group = rustGroup(input, invocation.index + invocation[0].length - 1);
      for (const rule of rules) {
        const bindings = match(rule.pattern, macroTokens(group.body));
        if (!bindings) continue;
        const body = rule.body.replace(/\$([A-Za-z_]\w*)/g, (whole, name) => bindings.get(name) ?? whole);
        edits.push({ start: invocation.index, end: group.end + 1, body: expand(body, depth + 1) });
        break;
      }
    }
    let result = input;
    for (const edit of edits.reverse()) result = result.slice(0, edit.start) + edit.body + result.slice(edit.end);
    return result;
  };
  // Definition bodies aren't declarations and cannot be invocation sites.
  let declarations = source;
  for (const [start, end] of definitions.reverse()) declarations = declarations.slice(0, start) + declarations.slice(end);
  return expand(declarations);
};
const text = expandDeclarations(readFileSync(runtimePath, "utf8"));
const fns = new Map(); // "path::name" -> [{ pub, sig, wasmOnly, gated, path }]
const types = new Map(); // "path::Name" -> [{ visible, generic, gated, derives }] (module-level struct / enum)
const globs = new Map(); // module path -> [module paths it glob-imports (`use <module>::*;`)]
const renames = new Map(); // "path::Alias" -> "path::Name" (`use path::{Name as Alias}`)
// The attribute lines (`#[...]`) on the item whose line starts at `lineStart`
// (the attribute and doc-comment lines directly above it).
const attributesBefore = (lineStart) => {
  const attributes = [];
  let end = lineStart - 1;
  while (end > 0) {
    const start = text.lastIndexOf("\n", end - 1) + 1;
    const line = text.slice(start, end).trim();
    if (!(line.startsWith("#[") || line.startsWith("//"))) break;
    if (line.startsWith("#[")) attributes.push(line);
    end = start - 1;
  }
  return attributes;
};
const cfgsBefore = (lineStart) => attributesBefore(lineStart).filter((line) => line.startsWith("#[cfg("));
const noRustPrelude = new Set();
{
  // frames: { mod: name | null, impl: type name | null, gated }. An inherent
  // `impl Name {` of a non-generic type holds methods keyed `path::Name::method`.
  const stack = [];
  let i = 0;
  let pendingMod = null;
  let pendingImpl = null;
  let pendingGated = false;
  // An item scope: every frame a module, or modules then one inherent impl.
  const itemScope = () => stack.every((frame, k) => frame.mod || (frame.impl && k === stack.length - 1));
  const n = text.length;
  const isIdent = (c) => /[A-Za-z0-9_]/.test(c);
  while (i < n) {
    const c = text[i];
    if (c === "#" && stack.every((frame) => frame.mod)) {
      const attribute = /^#(!?)\[\s*(no_implicit_prelude|no_std)\s*\]/.exec(text.slice(i, i + 80));
      if (attribute?.[1]) noRustPrelude.add(stack.map((frame) => frame.mod).join("::"));
    }
    if (c === "/" && text[i + 1] === "/") { i = text.indexOf("\n", i); if (i < 0) break; continue; }
    if (c === "/" && text[i + 1] === "*") {
      let depth = 1; i += 2;
      while (i < n && depth > 0) {
        if (text[i] === "/" && text[i + 1] === "*") { depth++; i += 2; } else if (text[i] === "*" && text[i + 1] === "/") { depth--; i += 2; } else i++;
      }
      continue;
    }
    if ((c === "r" || (c === "b" && text[i + 1] === "r")) && !isIdent(text[i - 1] ?? " ")) {
      const m = /^b?r(#*)"/.exec(text.slice(i, i + 40));
      if (m) {
        const close = `"${m[1]}`;
        const end = text.indexOf(close, i + m[0].length);
        i = end < 0 ? n : end + close.length;
        continue;
      }
    }
    if (c === '"') {
      i++;
      while (i < n && text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
      i++;
      continue;
    }
    if (c === "'") {
      // A char literal ('x', '\n', '\u{..}', b'x') or a lifetime ('a).
      const m = /^'(\\(u\{[0-9A-Fa-f]+\}|x[0-9A-Fa-f]{2}|.)|[^\\'])'/u.exec(text.slice(i, i + 16));
      i += m ? m[0].length : 1;
      continue;
    }
    if (c === "{") {
      stack.push({ mod: pendingMod, impl: pendingImpl, gated: pendingGated });
      pendingMod = null;
      pendingImpl = null;
      pendingGated = false;
      i++;
      continue;
    }
    if (c === "}") { stack.pop(); i++; continue; }
    if (c === ";") { pendingMod = null; pendingImpl = null; i++; continue; }
    if (isIdent(c) && !isIdent(text[i - 1] ?? " ")) {
      let j = i;
      while (j < n && isIdent(text[j])) j++;
      const word = text.slice(i, j);
      if (word === "mod") {
        const m = /^\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{/.exec(text.slice(j, j + 120));
        if (m) {
          pendingMod = m[1];
          pendingGated = cfgsBefore(text.lastIndexOf("\n", i) + 1).some((cfg) => cfg !== '#[cfg(not(target_arch = "wasm32"))]');
          if (attributesBefore(text.lastIndexOf("\n", i) + 1).includes("#[no_implicit_prelude]")) {
            noRustPrelude.add([...stack.filter((frame) => frame.mod).map((frame) => frame.mod), m[1]].join("::"));
          }
        }
      } else if (word === "impl" && stack.every((frame) => frame.mod)) {
        const m = /^\s+([A-Z][A-Za-z0-9_]*)\s*\{/.exec(text.slice(j, j + 120));
        if (m) {
          pendingImpl = m[1];
          pendingGated = cfgsBefore(text.lastIndexOf("\n", i) + 1).some((cfg) => cfg !== '#[cfg(not(target_arch = "wasm32"))]');
        }
      } else if (word === "fn" && itemScope()) {
        const m = /^\s+([A-Za-z_][A-Za-z0-9_]*)/.exec(text.slice(j, j + 120));
        if (m) {
          // The signature runs to the body's `{` (or `;`) outside parentheses
          // and angle brackets.
          let k = j;
          let depth = 0;
          while (k < n) {
            const d = text[k];
            if (d === "(" || d === "[") depth++;
            else if (d === ")" || d === "]") depth--;
            else if ((d === "{" || d === ";") && depth === 0) break;
            k++;
          }
          const lineStart = text.lastIndexOf("\n", i) + 1;
          const head = text.slice(lineStart, i);
          const cfgs = cfgsBefore(lineStart);
          const impl = stack.length > 0 ? stack[stack.length - 1].impl : null;
          const path = stack.filter((frame) => frame.mod).map((frame) => frame.mod).join("::");
          const owner = impl ? (path ? `${path}::${impl}` : impl) : path;
          const key = owner ? `${owner}::${m[1]}` : m[1];
          const list = fns.get(key) ?? [];
          list.push({
            pub: /\bpub\b/.test(head),
            sig: `fn ${text.slice(j, k)}`.replace(/\s+/g, " ").trim(),
            wasmOnly: cfgs.includes('#[cfg(target_arch = "wasm32")]'),
            // Gated by a platform or feature cfg (the adapter module itself is
            // `not(target_arch = "wasm32")`), here or on an enclosing module.
            gated: cfgs.some((cfg) => cfg !== '#[cfg(not(target_arch = "wasm32"))]') || stack.some((frame) => frame.gated),
            path,
            at: i,
            // The inherent impl's type for a method (`Self` and the receiver).
            selfType: impl,
          });
          fns.set(key, list);
          i = k;
          continue;
        }
      } else if ((word === "struct" || word === "enum") && stack.every((frame) => frame.mod)) {
        const m = /^\s+([A-Za-z_][A-Za-z0-9_]*)\s*(<)?/.exec(text.slice(j, j + 120));
        if (m) {
          const lineStart = text.lastIndexOf("\n", i) + 1;
          const path = stack.map((frame) => frame.mod).join("::");
          const key = path ? `${path}::${m[1]}` : m[1];
          const list = types.get(key) ?? [];
          const attributes = attributesBefore(lineStart);
          list.push({
            // The adapter module reaches a crate-root item, or a `pub` /
            // `pub(crate)` one in a module.
            visible: !path || /\bpub\b/.test(text.slice(lineStart, i)),
            generic: Boolean(m[2]),
            gated: attributes.some((line) => line.startsWith("#[cfg(") && line !== '#[cfg(not(target_arch = "wasm32"))]') || stack.some((frame) => frame.gated),
            derives: attributes.filter((line) => line.startsWith("#[derive(")).flatMap((line) => line.slice(9).split(/[,)\]]/).map((s) => s.trim().split("::").pop())).filter(Boolean),
            // The item's text from its keyword (its body follows): record exports read its fields.
            kind: word,
            at: i,
            path,
          });
          types.set(key, list);
        }
      } else if (word === "type" && stack.every((frame) => frame.mod)) {
        // Actual emitted Result aliases, resolved through the same import
        // table as records; an alias spelling alone conveys no carrier.
        const alias = /^\s+([A-Z]\w*)\s*<([^<>]+)>\s*=\s*((?:(?:std|core)::result::)?Result)\s*<([^;]+)>\s*;/.exec(text.slice(j, j + 2000));
        const lineStart = text.lastIndexOf("\n", i) + 1;
        if (alias && /\bpub\b/.test(text.slice(lineStart, i))) {
          const path = stack.map((frame) => frame.mod).join("::");
          const key = path ? `${path}::${alias[1]}` : alias[1];
          const list = types.get(key) ?? [];
          list.push({ visible: true, generic: true, kind: "rust-sum-alias", at: i, path, derives: [],
            alias: { parameters: alias[2], target: alias[3], arguments: alias[4], definition: `pub type ${alias[1]}<${alias[2]}> = ${alias[3]}<${alias[4]}>` },
            gated: cfgsBefore(lineStart).some((cfg) => cfg !== '#[cfg(not(target_arch = "wasm32"))]') || stack.some((frame) => frame.gated) });
          types.set(key, list);
        }
      } else if (word === "use" && stack.every((frame) => frame.mod)) {
        const here = stack.map((frame) => frame.mod);
        // A `use` path from the crate root; Rust standard-library namespaces
        // are absolute even inside a module.
        const resolve = (prefix) => {
          const target = prefix.split("::").filter(Boolean);
          const base = /^(?:crate|std|core|alloc)$/.test(target[0] ?? "") ? [] : [...here];
          for (const segment of target[0] === "crate" ? target.slice(1) : target) {
            if (segment === "super") base.pop();
            else if (segment !== "self") base.push(segment);
          }
          return base;
        };
        const m = /^\s+((?:[A-Za-z_][A-Za-z0-9_]*::)*)\*\s*;/.exec(text.slice(j, j + 120));
        if (m) {
          const key = here.join("::");
          globs.set(key, [...(globs.get(key) ?? []), resolve(m[1]).join("::")]);
        }
        // Explicit type imports, with or without an `as` spelling.
        const renamed = /^\s+((?:[A-Za-z_][A-Za-z0-9_]*::)*)(?:\{([^{}]*)\}|([A-Z][A-Za-z0-9_]*(?:\s+as\s+[A-Z][A-Za-z0-9_]*)?))\s*;/.exec(text.slice(j, j + 2000));
        if (renamed) {
          const base = resolve(renamed[1]);
          for (const entry of (renamed[2] ?? renamed[3]).split(",")) {
            const rename = /^\s*([A-Z][A-Za-z0-9_]*)(?:\s+as\s+([A-Z][A-Za-z0-9_]*))?\s*$/.exec(entry);
            if (rename) renames.set([...here, rename[2] ?? rename[1]].join("::"), [...base, rename[1]].join("::"));
          }
        }
      }
      i = j;
      continue;
    }
    i++;
  }
}

// The declaration a runtime type path names: `{ key, decls }` for the item at
// that module path, one a renamed import names, or one a module's glob import
// (`use super::*;`, `pub(crate) use jet_sync::*;`) brings in; null if none.
// External std/core/alloc paths are marked, never inferred to be Jet records.
const declarationOf = (rustPath, seen = new Set()) => {
  rustPath = rustPath.replace(/<[\s\S]*>$/, "");
  if (types.has(rustPath)) return { key: rustPath, decls: types.get(rustPath) };
  if (renames.has(rustPath)) return declarationOf(renames.get(rustPath), seen);
  if (/^(?:std|core|alloc)::/.test(rustPath)) return { key: rustPath, decls: [], external: true };
  const segments = rustPath.split("::");
  const name = segments.pop();
  const module = segments.join("::");
  if (seen.has(module)) return null;
  seen.add(module);
  let external = null;
  for (const target of globs.get(module) ?? []) {
    const found = declarationOf(target ? `${target}::${name}` : name, seen);
    if (found && !found.external) return found;
    if (found) external ??= found;
  }
  return external;
};
const nominalOutcome = [...types].some(([key, decls]) => key.split("::").pop() === "JetOutcome" &&
  decls.some((decl) => decl.visible && !decl.gated && (decl.kind === "struct" || decl.kind === "enum")));
const rustOutcomeAlias = (rustPath) => {
  const found = declarationOf(rustPath.replace(/^crate::/, ""));
  if (!found?.decls.some((decl) => decl.kind === "rust-sum-alias")) return false;
  if (found.decls.length !== 1 || found.key.split("::").pop() === "JetOutcome" && nominalOutcome) {
    classificationFailure(new Error(`${found.key} sum alias: visible nominal declaration is ambiguous`), rustPath, "");
    return false;
  }
  const decl = found.decls[0];
  const target = decl.alias.target;
  const shadow = target === "Result" ? declarationOf(decl.path ? `${decl.path}::Result` : "Result") : null;
  const disabled = [...noRustPrelude].some((module) => !module || decl.path === module || decl.path.startsWith(`${module}::`));
  return decl.visible && !decl.gated && (target !== "Result" || !shadow && !disabled);
};

// Computed route families (TIR routes.rs): precise_builtin_route spells every
// precise-numeric entry point `jet_<type>_<func>` and geometry_method_route
// every coordinate-space method `jet_math_<Type>_<method>`, both at the
// runtime root, so their rows are the root functions of those names.
for (const key of fns.keys()) {
  if (/^jet_(?:decimal|fraction)_[a-z0-9_]+$/.test(key)) addRoute(key, false, `core.precise.${key.slice(4)}`);
  else if (/^jet_math_[A-Z][A-Za-z0-9]*_[a-z0-9_]+$/.test(key)) addRoute(key, false, `core.math.${key.slice(9)}`);
}

// Every other Prelude symbol the TIR -> MIR lowering spells as a literal
// (handle-method, Dyn, HTTP request and Core-type associated rows written
// as `("Type", "member") => "symbol"` arms rather than registry rows) that
// names a runtime function. MIRRust.rs calls such a row's symbol with its
// arguments and widens an Int result from the kernel word, as for the handle
// rows above. A symbol a family above already lists keeps its own row.
{
  const tirDir = `${repo}/crates/jet-codegen/src/Codegen/TIR`;
  const spelled = /^(?:[a-z_][a-z0-9_]*::)*(?:jet_[a-z0-9_]+|[A-Z][A-Za-z0-9_]*::[a-z_][a-z0-9_]*)$/;
  for (const file of readdirSync(tirDir, { recursive: true }).filter((f) => f.endsWith(".rs")).sort()) {
    for (const m of readFileSync(`${tirDir}/${file}`, "utf8").matchAll(/"([A-Za-z_][A-Za-z0-9_:]*)"/g)) {
      const symbol = m[1];
      if (routes.has(symbol) || !spelled.test(symbol) || !fns.has(symbol)) continue;
      addRoute(symbol, true, `TIR/${file}`);
    }
  }
}

// ---------------------------------------------------------------------------
// 3. Carriers.

const handDir = `${repo}/crates/jet-codegen/src/Prelude/Core`;
const handTable = readdirSync(handDir).filter((name) => /^CAbi.*\.rs$/.test(name) && name !== "CAbiRoutes.rs")
  .sort().map((name) => readFileSync(`${handDir}/${name}`, "utf8")).join("\n");
const handExports = new Set([...handTable.matchAll(/pub(?:\(crate\))? (?:unsafe )?extern "C" fn ([A-Za-z0-9_]+)/g)].map((m) => m[1]));
for (const m of handTable.matchAll(/exact_int_(?:binary|located)!\s*\{?\(?([^)}]*)/g)) {
  for (const name of m[1].split(",").map((s) => s.trim()).filter(Boolean)) handExports.add(name);
}
// The Jet runtime object owns every `#Export(c)` of Compiler/JetRuntime's
// sources.list (U3: the declarations are the export set). Route carrier
// metadata below stays shared; only the Rust adapter bodies disappear.
const jetRuntimeSignatures = runtimeExportSignatures(repo);
const jetRuntimeExports = new Set(jetRuntimeSignatures.keys());
const nativeProviders = nativeProviderHandles(jetRuntimeSignatures);
const fixedInts = new Set(["u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "isize"]);

// A handle type whose runtime declaration derives `Clone` also gets a clone
// (a copy of an owned value the lowering must duplicate, e.g. a payload read
// out of a place that keeps its own), and may be passed by value (the adapter
// clones the borrowed handle, as a by-value String parameter copies its text).
// One deriving (or implementing) `PartialEq` gets an equality adapter: the
// structural `==` / `jet_eq` of a value holding it calls the runtime's own.
// A type name declared twice is ambiguous wherever only its leaf names it.
const declared = new Map();
for (const match of text.matchAll(/^\s*(?:pub(?:\([a-z]+\))? )?(?:struct|enum) ([A-Z][A-Za-z0-9_]*)\b/gm)) declared.set(match[1], (declared.get(match[1]) ?? 0) + 1);
const lines = text.split("\n");
const derivedKnown = new Map();
const derives = (full, trait) => {
  const key = `${trait} ${full}`;
  if (derivedKnown.has(key)) return derivedKnown.get(key);
  const name = recordApplication(full)?.base.split("::").pop() ?? full.split("::").pop();
  const manual = new RegExp(`^\\s*impl (?:(?:std|core)::(?:cmp|clone)::)?${trait} for (?:(?:super|crate|[a-z_][a-z0-9_]*)::)*${name}\\s*\\{`, "m").test(text);
  const found = declarationOf(full.replace(/^crate::/, ""));
  let result;
  if (found && found.decls.length === 1) {
    // The declaration's own derive list; a manual impl names the type by its
    // leaf, so it decides only for a leaf declared once.
    result = found.decls[0].derives.includes(trait) || (manual && declared.get(name) === 1);
  } else {
    // A type a macro declares has no declaration row: its leaf's derive or impl decides.
    const head = new RegExp(`^\\s*pub (struct|enum) ${name}\\b`);
    const derive = new RegExp(`#\\[derive\\([^)]*\\b${trait}\\b`);
    result = manual;
    for (let i = 0; i < lines.length && !result; i++) {
      if (!head.test(lines[i])) continue;
      for (let k = i - 1; k >= 0 && /^\s*(#\[|\/\/)/.test(lines[k]); k--) {
        if (derive.test(lines[k])) result = true;
      }
    }
  }
  derivedKnown.set(key, result);
  return result;
};
const derivesClone = (full) => derives(full, "Clone");

// The crate-root render traits (`JetDisplay` for `{value}`, `JetShow` for
// `{value:show}`, `JetDebug` for Debug text) a handle type implements: its
// runtime display, show and debug adapters call that one implementation (I9).
// An ambiguous type name gets none.
const renders = { JetDisplay: new Set(), JetShow: new Set(), JetDebug: new Set() };
for (const match of text.matchAll(/^\s*impl (?:(?:super|crate)::)*(JetDisplay|JetShow|JetDebug) for (?:(?:super|crate|[a-z_][a-z0-9_]*)::)*([A-Z][A-Za-z0-9_]*)\s*\{/gm)) {
  if (declared.get(match[2]) === 1) renders[match[1]].add(match[2]);
}

// A Rust type written in module `path` as a type the adapter module names.
const qualify = (ty, path) => {
  if (/^(?:crate|std|core|alloc)::/.test(ty)) return ty;
  if (ty.startsWith("super::")) return `crate::${path.split("::").slice(0, -1).filter(Boolean).concat(ty.slice(7)).join("::")}`;
  return path ? `crate::${path}::${ty}` : `crate::${ty}`;
};
// Expand only a declaration-proven Rust Result alias on the result edge.
// Actual arguments resolve in the caller's module; fixed alias payloads
// resolve in the declaration's module, even through renamed/glob imports.
const resultAlias = (type, path) => {
  const application = /^((?:[A-Za-z_]\w*::)*[A-Z]\w*)<([\s\S]*)>$/.exec(type);
  if (!application) return null;
  const full = qualify(application[1], path);
  if (!rustOutcomeAlias(full)) return null;
  const found = declarationOf(full.replace(/^crate::/, ""));
  if (found?.decls.length !== 1) return null;
  const decl = found.decls[0];
  const parameters = recordBindings(`struct ${application[1].split("::").pop()}<${decl.alias.parameters}>`, splitTop(application[2]), type);
  const qualified = (ty, module) => {
    if (ty === "()" || ty === "String" || ty === "bool" || ty === "f64" || unsupportedRawArguments.has(ty)) return ty;
    const nested = /^([^<]+)<([\s\S]*)>$/.exec(ty);
    if (nested) {
      const base = nested[1] === "Option" || nested[1] === "Result" || nested[1] === "Vec" ? nested[1] : qualify(nested[1], module);
      return `${base}<${splitTop(nested[2]).map((arg) => qualified(arg, module)).join(",")}>`;
    }
    return qualify(ty, module);
  };
  for (const [name, argument] of parameters) parameters.set(name, qualified(argument, path));
  const args = splitTop(decl.alias.arguments.replace(/\s+/g, ""));
  if (args.length !== 2) return null;
  return `Result<${args.map((arg) => qualified(substituteRecordType(arg, parameters), decl.path)).join(",")}>`;
};
const recordDeclUnchecked = (full) => {
  const found = declarationOf(full.replace(/^crate::/, ""));
  if (!found || found.decls.length !== 1) return null;
  const decl = found.decls[0];
  if (decl.gated || decl.at === undefined) return null;
  const open = text.indexOf("{", decl.at);
  const head = text.slice(decl.at, open);
  if (open < 0 || /[;(]/.test(head) || /\bwhere\b/.test(head)) return null;
  const application = recordApplication(full);
  if (application.args.some((arg) => arg.startsWith("'"))) return null;
  const bindings = recordBindings(head, application.args, full);
  let depth = 0;
  let close = open;
  for (; close < text.length; close++) {
    if (text[close] === "{") depth++;
    else if (text[close] === "}" && --depth === 0) break;
  }
  const body = text.slice(open + 1, close).replace(/\/\/[^\n]*/g, "");
  if (/#\[cfg/.test(body)) return null;
  return { kind: decl.kind, path: decl.path, items: splitTop(body.replace(/#\[[^\]]*\]/g, "")).map((item) => substituteRecordType(item, bindings)) };
};
const recordDecl = (full) => {
  try { return recordDeclUnchecked(full); }
  catch (error) { classificationFailure(error, full, "", { phase: "record declaration" }); return null; }
};
// Canonical C carriers shared with Jet host kernels. Match resolved
// declarations, never a leaf spelling that could name a user type.
const handleBridges = new Map();
for (const [rust, type, stem, borrowedBytes = 0] of [
  ["jet_std::IOError", "JetCIOError", "error"],
  ["jet_std::IOContext", "JetCIOContext", "context"],
  ...(jetRuntimeExports.has("jet_rt_handle_drop_jet_task_failure")
    ? [["JetTaskFailure", "JetCTaskFailure", "failure"]] : []),
  ...(jetRuntimeExports.has("jet_rt_handle_drop_jet_resolve_error")
    ? [["Jet__ResolveError", "JetCResolveError", "resolve_error"]] : []),
  ...(jetRuntimeExports.has("jet_rt_handle_drop_jet_ip_addr")
    ? [["JetIpAddr", "JetCIpAddr", "ip", 24]] : []),
  ...(jetRuntimeExports.has("jet_rt_handle_drop_jet_socket_addr")
    ? [["JetSocketAddr", "JetCSocketAddr", "socket", 56]] : []),
]) {
  const key = declarationOf(rust)?.key;
  if (key !== undefined) handleBridges.set(key, { type, stem, borrowedBytes });
}
const handleBridge = (full) => handleBridges.get(declarationOf(full.replace(/^crate::/, ""))?.key);
const handleType = (full) => handleBridge(full) ? `super::jet_c_abi_host::${handleBridge(full).type}` : full;
const handleBorrow = (full, value) => handleBridge(full) ? `&super::jet_c_abi_host::${handleBridge(full).stem}_view(${value})` : `unsafe { &*${value} }`;
const handleCopy = (full, value) => handleBridge(full) ? `super::jet_c_abi_host::${handleBridge(full).stem}_view(${value})` : `unsafe { &*${value} }.clone()`;
const handleTake = (full, value) => handleBridge(full) ? `super::jet_c_abi_host::${handleBridge(full).stem}_take(${value})` : `*unsafe { Box::from_raw(${value}) }`;
const handleOut = (full, value) => handleBridge(full) ? `super::jet_c_abi_host::${handleBridge(full).stem}_handle(${value})` : `Box::into_raw(Box::new(${value}))`;
const opaqueName = /^(?:[A-Za-z_][A-Za-z0-9_]*::)*([A-Z][A-Za-z0-9_]*|__jet_[A-Za-z0-9_]+)(?:<[\s\S]*>)?$/;
// Scalar records keep native storage in Jet; only the exact-Int field crosses
// this ABI. Resolve the declaration, never a route symbol or leaf-name alias.
const wordRecords = new Map();
function wordRecord(ty, path) {
  if (!opaqueName.test(ty) || ty.includes("<")) return null;
  const full = qualify(ty, path);
  const found = declarationOf(full.replace(/^crate::/, ""));
  if (!found || found.decls.length !== 1) return null;
  const declaration = found.decls[0];
  if (!declaration.visible || declaration.gated || declaration.generic || declaration.kind !== "struct") return null;
  const record = recordDecl(full);
  if (!record || record.items.length !== 1) return null;
  const field = /^(pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*|__jet_[A-Za-z0-9_]+)\s*:\s*(.+)$/.exec(record.items[0]);
  if (!field || (record.path && !field[1])) return null;
  const fieldType = field[3].replace(/\s+/g, "");
  let resolvedField = qualify(fieldType, record.path).replace(/^crate::/, "");
  const seen = new Set();
  while (renames.has(resolvedField) && !seen.has(resolvedField)) {
    seen.add(resolvedField);
    resolvedField = renames.get(resolvedField).replace(/^crate::/, "");
  }
  const exact = resolvedField === "jet_foundation::Numeric::JetInt";
  if (fieldType !== "i64" && !exact) return null;
  const result = { full: `crate::${found.key}`, declaration: found.key, field: field[2],
    member: field[2].replace(/^__jet_/, ""), exact };
  wordRecords.set(found.key, result);
  return result;
}
const concreteHandles = new Map(); // specialization name -> resolved declaration application
const borrowedHandles = new Map(); // seed-only Rust lifetime carriers, not Jet type instances
const unsupportedSumCarriers = new Map(); // declaration-proved seed alias, no plain C codec
const unsupportedRustCarriers = new Map(); // std/core/alloc provenance, no C record codec
const unsupportedGenericCarriers = new Map(); // proved seed TYPEs without a record codec
const unsupportedRawArguments = new Set([...fixedInts, "i64", "i128", "u128", "f32", "char"]);
const classificationErrors = [];
let classificationSite = null;
function declarationParameters(at) {
  const opener = /^(?:struct|enum)\s+\w+\s*</.exec(text.slice(at, at + 2000));
  if (!opener) return null;
  const start = at + opener[0].length;
  const stack = [">"];
  const closing = { "<": ">", "(": ")", "[": "]" };
  for (let end = start; end < text.length; end++) {
    const char = text[end];
    if (closing[char]) stack.push(closing[char]);
    else if (char === stack[stack.length - 1] && !(char === ">" && text[end - 1] === "-")) {
      stack.pop();
      if (!stack.length) return splitTop(text.slice(start, end));
    } else if (char === "{" || char === ";") break;
  }
  throw new Error("generic record: unterminated declaration parameters");
}
function unsupportedSeedRecord(type, path, reason) {
  const found = declarationOf(qualify(type.replace(/<[\s\S]*>$/, ""), path).replace(/^crate::/, ""));
  if (!found || found.decls.length !== 1 || found.key.split("::").pop().startsWith("__jet_")) return false;
  const decl = found.decls[0];
  const key = `${found.key}:${type}`;
  const previous = unsupportedGenericCarriers.get(key)?.reason;
  unsupportedGenericCarriers.set(key, { type, qualified: `crate::${found.key}`,
    declaration: { kind: decl.kind, path: decl.path, at: decl.at },
    reason: previous && !previous.includes(reason) ? `${previous}; ${reason}` : `${reason}; actual seed declaration proved, no native record codec` });
  return true;
}
function classificationFailure(error, type, path, consumer = classificationSite) {
  if ((consumer?.phase?.includes("record") || consumer?.phase === "source identity") &&
      unsupportedSeedRecord(type, path, error.message)) return;
  if (!nativeListOnly) throw error;
  const declaration = declarationOf(qualify(type.replace(/<[\s\S]*>$/, ""), path).replace(/^crate::/, ""));
  classificationErrors.push({ type, reason: error.message, site: {
    runtime: resolve(runtimePath), module: path, consumer,
    offsetUnits: "UTF-16 code units in emitted runtime text",
    declaration: declaration ? { key: declaration.key, offsets: declaration.decls.map((decl) => decl.at) } : null,
  } });
}
function runtimeHandle(ty, path) {
  if (!nativeListOnly) return runtimeHandleUnchecked(ty, path);
  try { return runtimeHandleUnchecked(ty, path); }
  catch (error) { classificationFailure(error, ty, path); return null; }
}
const genericArgumentKinds = new Map([["f64", "Float"], ["bool", "Bool"], ["String", "String"], ["crate::jet_foundation::Numeric::JetInt", "Int"]]);
function runtimeHandleUnchecked(ty, path) {
  const aliasPath = qualify(ty.replace(/<[\s\S]*>$/, ""), path);
  // RUST-EXIT S5: resolve imports before nominal TYPE structural validation.
  let resolved = declarationOf(aliasPath.replace(/^crate::/, ""));
  // Rust's default prelude supplies Box only after local/import shadowing.
  // Explicit crate::Box never inherits this fallback; disabled preludes do not.
  if (!resolved && ty.replace(/<[\s\S]*>$/, "") === "Box" &&
      ![...noRustPrelude].some((module) => !module || path === module || path.startsWith(`${module}::`))) {
    resolved = { key: "std::boxed::Box", decls: [], external: true };
  }
  if (resolved?.external && !/^crate::(?:std|core|alloc)::/.test(ty) &&
      !aliasPath.split("::").pop().startsWith("__jet_")) {
    const qualified = resolved.key + ty.slice(ty.replace(/<[\s\S]*>$/, "").length);
    unsupportedRustCarriers.set(qualified, { type: ty, qualified,
      reason: /dynFn(?:Mut|Once)?\b/.test(ty) ? "Rust closure carrier, no C callback codec; seed-only"
        : "Rust standard-library carrier, no C record codec; seed-only" });
    return null;
  }
  if (rustOutcomeAlias(aliasPath)) {
    unsupportedSumCarriers.set(`${aliasPath}:${ty}`, { type: ty, qualified: aliasPath,
      declaration: { key: resolved.key, at: resolved.decls[0].at },
      reason: `resolved ${resolved.decls[0].alias.definition}; plain/tuple/list-element carrier has no sum C codec; seed-only` });
    return null;
  }
  const structural = /^((?:[A-Za-z_]\w*::)*(?:[A-Z]\w*|__jet_\w+))<([\s\S]*)>$/.exec(ty);
  if (structural && /[<>()[\]]/.test(structural[2].replace(/\(\)/g, "")) && resolved?.decls.length === 1) {
    const decl = resolved.decls[0];
    const params = declarationParameters(decl.at);
    if (params && decl.visible && !decl.gated) {
      const supplied = splitTop(structural[2]);
      const required = params.filter((param) => !param.includes("=")).length;
      if (supplied.length < required || supplied.length > params.length) throw new Error(`generic record ${ty}: expected ${required}..${params.length} declaration arguments, got ${supplied.length}`);
      if (unsupportedSeedRecord(ty, path, "nested/structural TYPE specialization unsupported")) return null;
    }
  }
  const application = recordApplication(ty);
  if (!application) return null;
  if (application.args.length === 0) return { name: application.base.split("::").pop(), full: qualify(ty, path) };
  const requested = qualify(application.base, path);
  const found = declarationOf(requested.replace(/^crate::/, ""));
  if (!found || found.decls.length !== 1) throw new Error(`generic record ${ty}: unresolved or ambiguous declaration ${requested}`);
  const decl = found.decls[0];
  if (!decl.visible || decl.gated) throw new Error(`generic record ${ty}: inaccessible or gated declaration`);
  // RUST-EXIT S5: retain the existing opaque seed codec's Rust lifetime
  // spelling. Never turn a lifetime into a Jet TYPE argument or native owner.
  if (application.args.some((arg) => arg.startsWith("'"))) {
    const open = text.indexOf("{", decl.at);
    const head = text.slice(decl.at, open);
    const parameters = /^(?:struct|enum)\s+\w+\s*<([^<>]*)>/.exec(head)?.[1].split(",").map((arg) => arg.trim());
    if (!application.args.every((arg) => /^'[A-Za-z_]\w*$/.test(arg)) ||
        !parameters || parameters.length !== application.args.length ||
        !parameters.every((arg) => /^'[A-Za-z_]\w*$/.test(arg))) {
      throw new Error(`generic record ${ty}: mixed lifetime and TYPE arguments are unsupported`);
    }
    const handle = { name: application.base.split("::").pop(), full: qualify(ty, path) };
    borrowedHandles.set(handle.name, { runtime: handle.name, qualified: handle.full,
      reason: "actual Rust lifetime-only declaration; borrowed seed codec, not native-admissible Jet TYPE specialization" });
    return handle;
  }
  // Arguments name types in the caller's module, not the declaration's.
  const args = application.args.map((arg) => {
    if (arg === "()" || unsupportedRawArguments.has(arg)) return arg;
    if (genericArgumentKinds.has(arg)) return arg;
    if (/^(?:(?:crate|super|[a-z_][a-z0-9_]*)::)*(?:Numeric::)?JetInt$/.test(arg)) return "crate::jet_foundation::Numeric::JetInt";
    const argument = declarationOf(qualify(arg, path).replace(/^crate::/, ""));
    if (!argument || argument.decls.length !== 1 || argument.decls[0].generic || argument.decls[0].gated || !argument.decls[0].visible) {
      throw new Error(`generic record ${ty}: unresolved or unsupported argument ${arg}`);
    }
    return `crate::${argument.key}`;
  });
  const open = text.indexOf("{", decl.at);
  recordBindings(text.slice(decl.at, open), args, ty);
  const unsupportedArgs = args.filter((arg) => arg === "()" || unsupportedRawArguments.has(arg));
  if (unsupportedArgs.length) {
    if (found.key.split("::").pop().startsWith("__jet_")) {
      throw new Error(`generic record ${ty}: SOURCE ${unsupportedArgs.includes("()") ? "TYPE-Unit" : "raw-machine TYPE"} specialization requires a typed ${unsupportedArgs.includes("()") ? "Unit" : "identity/record"} codec`);
    }
    // RUST-EXIT S5: prior seed routes skip these TYPEs. Keep them unadmitted
    // until a typed identity/record codec exists; never invent a layout.
    unsupportedGenericCarriers.set(`${requested}:${ty}`, { type: ty, qualified: requested,
      declaration: { kind: decl.kind, path: decl.path, at: decl.at },
      reason: `${unsupportedArgs.includes("()") ? "seed TYPE-Unit specialization unsupported" : "seed raw-machine TYPE specialization unsupported"}: actual generic declaration and arity proved, no typed identity/record codec for ${unsupportedArgs.join(",")}; seed-only` });
    return null;
  }
  const name = recordInstanceName(found.key, args);
  const full = `crate::${found.key}<${args.join(",")}>`;
  const handle = { name, full, args };
  const known = concreteHandles.get(name);
  if (known && known.full !== full) throw new Error(`generic record ${ty}: specialization key collision ${name}`);
  concreteHandles.set(name, handle);
  return handle;
}
const expressionType = (full) => full.replace("<", "::<");

// A fieldless runtime enum crosses as its variant index in a word; the Jet
// table names its variants in declaration order so the lowering checks that
// the Jet type's discriminants (variant indices) mean the same variants.
const fieldlessEnums = new Map(); // name -> [variant] (null: two different declarations)
for (const match of text.matchAll(/^\s*pub enum ([A-Z][A-Za-z0-9_]*)\s*\{([^{}]*)\}/gm)) {
  const body = match[2].replace(/\/\/[^\n]*/g, "").replace(/#\[[^\]]*\]/g, "");
  const variants = body.split(",").map((s) => s.trim()).filter(Boolean);
  if (variants.length === 0 || !variants.every((v) => /^[A-Z][A-Za-z0-9_]*$/.test(v))) continue;
  const known = fieldlessEnums.get(match[1]);
  fieldlessEnums.set(match[1], known !== undefined && (known === null || known.join(",") !== variants.join(",")) ? null : variants);
}
function tagCarrier(ty, path) {
  if (!opaqueName.test(ty) || ty.includes("<")) return null;
  const name = opaqueName.exec(ty)[1];
  const variants = fieldlessEnums.get(name);
  if (!variants) return null;
  const full = qualify(ty, path);
  return {
    jet: `Tag{name: "${name}", variants: "${variants.join(",")}"}`,
    read: (a) => `match ${a} { ${variants.map((v, i) => `${i} => ${full}::${v}`).join(", ")}, _ => range_stop() }`,
    write: (v) => `match ${v} { ${variants.map((variant, i) => `${full}::${variant} => ${i}`).join(", ")} }`,
  };
}

// A List element: its carrier, the Rust value of a slot word `w`, and the slot
// word of a Rust value `e` (List slots are eight bytes; Lists.jet). A Char is
// its Unicode scalar value in a word.
function elementCarrier(ty) {
  if (ty === "String") return { jet: "Text", read: "view(w as JetCString).to_owned()", write: "handle(e) as u64" };
  if (ty === "i64") return { jet: "Word", read: "native(w as i64)", write: "e as u64" };
  if (fixedInts.has(ty)) return { jet: "Word", slot: ty, read: `w as ${ty}`, write: "e" };
  if (ty === "bool") return { jet: "Flag", read: "w != 0", write: "e as u64" };
  if (ty === "f64") return { jet: "Float", read: "f64::from_bits(w)", write: "e.to_bits()" };
  if (ty === "char") return { jet: "Word", read: "scalar(w as i64)", write: "e as u64" };
  return null;
}

// A List<U8> element of an outer List: a native 24-byte [len, cap, data] header
// with packed inner bytes. Seed Rust adapters have no nested slot codec, so this
// element is admitted only for a Jet runtime #Export(c) (`nativeOnly`), whose
// route row is emitted without a Rust body.
function nestedByteElement(ty) {
  return ty === "Vec<u8>" ? { jet: "List{elem: LowerRouteCarrier.Word}", nativeOnly: true } : null;
}

// A List parameter element of a cloneable runtime type: each slot word is a
// borrowed handle, cloned into the Rust Vec (the lowering lends a handle per
// element: a record element converts through Records.jet for the call).
function handleElement(ty, path) {
  if (!opaqueName.test(ty) || tagCarrier(ty, path)) return null;
  const handle = runtimeHandle(ty, path);
  if (!handle) return null;
  const { name, full } = handle;
  if (name === "String" || name === "Self" || !derivesClone(full)) return null;
  return { jet: `Handle{name: "${name}"}`, read: handleCopy(full, `(w as *mut ${handleType(full)})`), handle: { name, full } };
}

// Parameter carrier: { jet, ctys: [C types], expr(names) } or null. A List
// parameter is two words: its slot data pointer and its length.
function paramCarrier(ty, path, symbol) {
  ty = ty.replace(/\s+/g, "");
  const one = (jet, cty, expr) => ({ jet, ctys: [cty], expr: ([a]) => expr(a) });
  if (ty === "&str") return one("Text", "JetCString", (a) => `view(${a})`);
  if (ty === "&String") return one("Text", "JetCString", (a) => `&borrowed(${a})`);
  if (ty === "Option<&String>") return one("Text", "JetCString", (a) => `(!${a}.is_null()).then(|| borrowed(${a})).as_ref()`);
  if (ty === "String") return one("Text", "JetCString", (a) => `view(${a}).to_owned()`);
  if (ty === "&i64") return one("Word", "i64", (a) => `&native(${a})`);
  // The prelude Ordering (by value or borrowed): the Jet enum's variant index.
  const ordering = /^(&?)__jet_Ordering$/.exec(ty);
  if (ordering) return one(`Tag{name: "Ordering", variants: "Less,Equal,Greater"}`, "i64", (a) => `${ordering[1]}match ${a} { 0 => crate::__jet_Ordering::__jet_Less, 1 => crate::__jet_Ordering::__jet_Equal, 2 => crate::__jet_Ordering::__jet_Greater, _ => range_stop() }`);
  if (ty === "i64") return one("Word", "i64", (a) => `native(${a})`);
  if (fixedInts.has(ty)) return one("Word", "i64", (a) => `fixed::<${ty}>(${a})`);
  if (ty === "bool") return one("Flag", "u8", (a) => `${a} != 0`);
  if (ty === "f64") return one("Float", "f64", (a) => a);
  if (ty === "char") return one("Word", "i64", (a) => `scalar(${a})`);
  // A fieldless enum by value: its variant index (results keep their carrier).
  const tag = tagCarrier(ty, path);
  if (tag) return one(tag.jet, "i64", tag.read);
  const scalarRecord = wordRecord(ty.startsWith("&") && !ty.startsWith("&mut") ? ty.slice(1) : ty, path);
  if (scalarRecord) {
    const read = (a) => scalarRecord.exact
      ? `unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }` : `native(${a})`;
    return one("Word", "i64", (a) => `${ty.startsWith("&") ? "&" : ""}${scalarRecord.full} { ${scalarRecord.field}: ${read(a)} }`);
  }
  // An exact Int by value: the borrowed carrier word, cloned into an owner.
  // The exact Int is spelled from the crate root wherever its kernel names it.
  if (/^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return one("Word", "i64", (a) => `unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }`);
  // A borrowed exact Int (an exact_prelude_adapter kernel): an owner of the
  // same carrier word, lent for the call.
  if (/^&(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return one("Word", "i64", (a) => `&unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }`);
  const list = /^(&?)(?:\[(.+)\]|Vec<(.+)>)$/.exec(ty);
  if (list && (list[1] === "&" || list[3])) {
    const element = elementCarrier(list[2] ?? list[3]) ?? handleElement(list[2] ?? list[3], path) ?? nestedByteElement(list[2] ?? list[3]);
    if (!element) return null;
    const items = (d, n) => `list_in(${d}, ${n}, |w| ${element.read})`;
    return { jet: `List{elem: LowerRouteCarrier.${element.jet}}`, ctys: [`*const ${element.slot ?? "u64"}`, "i64"], expr: ([d, n]) => (list[1] === "&" ? `&${items(d, n)}` : items(d, n)), handle: element.handle, nativeOnly: element.nativeOnly };
  }
  // A mutably borrowed byte List lends its own storage (data, length) to a Jet
  // provider that writes it in place. Seed adapters have no write-back codec,
  // and a Rust Vec copy would leave the caller's bytes untouched: native-only.
  if (ty === "&mutVec<u8>") return { jet: "List{elem: LowerRouteCarrier.Word}", ctys: ["*mut u8", "i64"], nativeOnly: true };
  // A mutably borrowed handle: the adapter mutates the runtime value in the
  // caller's own box (the lowering lends the handle of its Write place).
  const borrowedMut = /^&mut(.+)$/.exec(ty);
  if (borrowedMut && opaqueName.test(borrowedMut[1]) && !tagCarrier(borrowedMut[1], path)) {
    const handle = runtimeHandle(borrowedMut[1], path);
    if (!handle) return null;
    const { name, full } = handle;
    if (name === "String" || name === "Self") return null;
    if (handleBridge(full)) throw new Error(`${name} route ${symbol}: mutable Rust borrowing is not a C carrier`);
    return { ...one(`HandleMut{name: "${name}"}`, `*mut ${full}`, (a) => `unsafe { &mut *${a} }`), handle: { name, full } };
  }
  const borrowed = /^&([^&].*)$/.exec(ty);
  // A borrowed fieldless enum: its variant index too.
  const borrowedTag = borrowed ? tagCarrier(borrowed[1], path) : null;
  if (borrowedTag) return one(borrowedTag.jet, "i64", (a) => `&${borrowedTag.read(a)}`);
  if (borrowed && !borrowed[1].startsWith("mut") && opaqueName.test(borrowed[1])) {
    const handle = runtimeHandle(borrowed[1], path);
    if (!handle) return null;
    const { name, full } = handle;
    if (name === "String" || name === "Self") return null;
    return { ...one(`Handle{name: "${name}"}`, `*mut ${handleType(full)}`, (a) => handleBorrow(full, a)), handle: { name, full } };
  }
  // A cloneable handle by value: the adapter copies the borrowed handle.
  if (!borrowed && opaqueName.test(ty)) {
    const handle = runtimeHandle(ty, path);
    if (!handle) return null;
    const { name, full } = handle;
    if (name === "String" || name === "Self" || !derivesClone(full)) return null;
    return { ...one(`Handle{name: "${name}"}`, `*mut ${handleType(full)}`, (a) => handleCopy(full, a)), handle: { name, full } };
  }
  return null;
}

// A value carrier for a result side: { jet, cty, wrap(v) } or null; "()" is Unit.
function valueCarrier(ty, path, nativeInt, rawFixed = false) {
  ty = ty.replace(/\s+/g, "");
  if (ty === "()") return { jet: "Unit", cty: null, wrap: () => "()" };
  if (ty === "String") return { jet: "Text", cty: "JetCString", wrap: (v) => `handle(${v})` };
  if (ty === "&'staticstr") return { jet: "Text", cty: "JetCString", wrap: (v) => `text_handle(${v})` };
  if (ty === "i64") return { jet: "Word", cty: "i64", wrap: (v) => (nativeInt ? `crate::jet_std::jet_int_from_i64(${v})` : v) };
  // Fixed Reader results are raw machine bits, not exact-Int owners. Casts
  // sign-extend signed widths and preserve all 64 bits of unsigned u64.
  if (rawFixed && fixedInts.has(ty)) return { jet: "Word", cty: "i64", wrap: (v) => `(${v}) as i64` };
  // Lower's MIR F32 uses its low32 raw word with four-byte storage. Never
  // widen through f64: preserve every binary32 NaN/signaling payload bit.
  if (rawFixed && ty === "f32") return { jet: "Word", cty: "i64", wrap: (v) => `(${v}).to_bits() as i64` };
  if (fixedInts.has(ty)) return { jet: "Word", cty: "i64", wrap: (v) => nativeInt ? `crate::jet_std::jet_int_from_i64(i64::try_from(${v}).unwrap_or_else(|_| range_stop()))` : `(${v}) as i64` };
  if (ty === "bool") return { jet: "Flag", cty: "bool", wrap: (v) => v };
  if (ty === "f64") return { jet: "Float", cty: "f64", wrap: (v) => v };
  if (ty === "char") return { jet: "Word", cty: "i64", wrap: (v) => `(${v}) as i64` };
  // The prelude Ordering: the Jet enum's variant index (Less, Equal, Greater).
  if (ty === "__jet_Ordering") return { jet: `Tag{name: "Ordering", variants: "Less,Equal,Greater"}`, cty: "i64", wrap: (v) => `match ${v} { crate::__jet_Ordering::__jet_Less => 0, crate::__jet_Ordering::__jet_Equal => 1, crate::__jet_Ordering::__jet_Greater => 2 }` };
  // An owned exact Int: its carrier word, ownership passing to the caller.
  if (/^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return { jet: "Word", cty: "i64", wrap: (v) => `(${v}).into_raw()` };
  const scalarRecord = wordRecord(ty, path);
  if (scalarRecord) return { jet: "Word", cty: "i64", wrap: (v) => scalarRecord.exact
    ? `(${v}).${scalarRecord.field}.into_raw()` : `crate::jet_std::jet_int_from_i64((${v}).${scalarRecord.field})` };
  if (opaqueName.test(ty)) {
    const handle = runtimeHandle(ty, path);
    if (!handle) return null;
    const { name, full } = handle;
    if (name === "Self" || name === "String") return null;
    return { jet: `Handle{name: "${name}"}`, cty: `*mut ${handleType(full)}`, wrap: (v) => handleOut(full, v), handle: { name, full } };
  }
  return null;
}

// Split a comma list at depth 0 of <>, (), [].
function splitTop(s) {
  const parts = [];
  let depth = 0;
  let start = 0;
  for (let k = 0; k < s.length; k++) {
    const d = s[k];
    if ("<([".includes(d)) depth++;
    else if (">)]".includes(d) && !(d === ">" && s[k - 1] === "-")) depth--;
    else if (d === "," && depth === 0) { parts.push(s.slice(start, k)); start = k + 1; }
  }
  if (s.slice(start).trim()) parts.push(s.slice(start));
  return parts.map((p) => p.trim()).filter(Boolean);
}

// A runtime signature `fn name<generics>(params) -> result where ...`, split at
// its own brackets (a parameter or result may hold `(`, `)` and `->` itself).
function parseSignature(sig) {
  const head = /^fn [A-Za-z0-9_]+\s*/.exec(sig);
  if (!head) return null;
  let k = head[0].length;
  let generics = "";
  if (sig[k] === "<") {
    const start = k;
    let depth = 0;
    for (; k < sig.length; k++) {
      if (sig[k] === "<") depth++;
      else if (sig[k] === ">" && sig[k - 1] !== "-" && --depth === 0) { k++; break; }
    }
    generics = sig.slice(start, k);
    while (sig[k] === " ") k++;
  }
  if (sig[k] !== "(") return null;
  const open = k;
  let depth = 0;
  for (; k < sig.length; k++) {
    if (sig[k] === "(") depth++;
    else if (sig[k] === ")" && --depth === 0) break;
  }
  if (depth !== 0) return null;
  const rest = sig.slice(k + 1).replace(/\swhere\s.*$/, "").trim();
  if (rest && !rest.startsWith("->")) return null;
  return { generics, params: sig.slice(open + 1, k), result: rest ? rest.slice(2).trim() : undefined };
}

// The result shape: { jet, ret, outs: [{ name, cty }], body(call) } or null.
// A never-returning kernel (`!`) has the Unit shape: the call does not come back.
// A List inside an Option or Result comes back as two out words, its slot
// buffer (`some` / `ok`) and its length (`some_len` / `ok_len`); the lowering
// passes the second at the payload slot's next word. A tuple returns nothing
// and writes item `i` through out pointer `item<i>`, each to its own zeroed
// eight-byte slot.
function resultShape(ty, path, nativeInt, rawFixed = false) {
  ty = (ty ?? "()").replace(/\s+/g, "");
  if (ty === "!") ty = "()";
  // `JetOutcome<T, JetAbsent>` (= `Result<T, JetAbsent>`) is generated Rust's
  // `T?`: it crosses as an Option.
  const outcomeCandidate = /^(?:(?:[a-z_][a-z0-9_]*::)*JetOutcome|Result)<(.*),JetAbsent>$/.exec(ty);
  const outcome = outcomeCandidate && (ty.startsWith("Result<") || rustOutcomeAlias(qualify(ty.slice(0, ty.indexOf("<")), path)))
    ? outcomeCandidate : null;
  if (!outcome) {
    const expanded = resultAlias(ty, path);
    if (expanded) {
      const shape = resultShape(expanded, "", nativeInt, rawFixed);
      // Unsupported nested payloads retain the declaration-proven sum
      // inventory reason; expansion is not admission of a new codec.
      if (!shape) runtimeHandle(ty, path);
      return shape;
    }
  }
  const generic = outcome ? [ty, "Option", outcome[1]] : /^(Option|Result)<(.*)>$/.exec(ty);
  const [somePattern, nonePattern] = outcome ? ["Ok", "Err(_)"] : ["Some", "None"];
  // A List result element: a plain slot word, or a new owned handle of a
  // runtime value (the lowering's List keeps the handles as its elements).
  const resultElement = (t) => {
    const plain = elementCarrier(t) ?? nestedByteElement(t);
    if (plain) return plain;
    if (!opaqueName.test(t) || tagCarrier(t, path)) return null;
    const handle = runtimeHandle(t, path);
    if (!handle) return null;
    const { name, full } = handle;
    if (name === "String" || name === "Self") return null;
    return { jet: `Handle{name: "${name}"}`, write: `${handleOut(full, "e")} as u64`, handle: { name, full } };
  };
  const listOf = (t) => {
    const list = /^Vec<(.+)>$/.exec(t);
    return list ? resultElement(list[1]) : null;
  };
  const list = /^Vec<(.+)>$/.exec(ty);
  if (list) {
    // A List result: its length, with its slot buffer written to `data`.
    const element = resultElement(list[1]);
    if (!element) return null;
    return {
      jet: `Plain{carrier: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "data", cty: `*mut ${element.slot ?? "u64"}` }], handles: element.handle ? [element.handle] : [], nativeOnly: element.nativeOnly,
      body: (call) => `list_out(${call}.into_iter().map(|e| ${element.write}).collect(), data)`,
    };
  }
  const tuple = /^\((.*)\)$/.exec(ty);
  if (tuple && ty !== "()") {
    const tupleCarrier = (type, name) => {
      const option = /^Option<(.+)>$/.exec(type);
      if (option) {
        const some = tupleCarrier(option[1], `${name}_some`);
        if (!some) return null;
        return {
          jet: `Option{some: LowerRouteCarrier.${some.jet}}`,
          outs: [{ name: `${name}_tag`, cty: "i64" }, ...some.outs],
          handles: some.handles,
          write: (value) => `match ${value} { Some(value) => { ${some.write("value")} unsafe { ${name}_tag.write(1) }; }, None => { unsafe { ${name}_tag.write(0) }; } }`,
        };
      }
      const value = valueCarrier(type, path, nativeInt, rawFixed);
      if (!value?.cty) return null;
      return {
        jet: value.jet, outs: [{ name, cty: value.cty }],
        handles: value.handle ? [value.handle] : [],
        write: (expression) => `unsafe { ${name}.write(${value.wrap(expression)}) };`,
      };
    };
    const items = splitTop(tuple[1]).map((item, index) => tupleCarrier(item, `item${index}`));
    if (items.length < 2 || items.some((item) => !item)) return null;
    const names = items.map((_, i) => `e${i}`);
    const writes = items.map((item, i) => `            ${item.write(names[i])}`);
    return {
      jet: `Tuple{items: [LowerRouteCarrier]{${items.map((item) => `LowerRouteCarrier.${item.jet}`).join(", ")}}}`, ret: null,
      outs: items.flatMap((item) => item.outs), handles: items.flatMap((item) => item.handles),
      body: (call) => `{\n            let (${names.join(", ")}) = ${call};\n${writes.join("\n")}\n        }`,
    };
  }
  if (!generic) {
    const v = valueCarrier(ty, path, nativeInt, rawFixed);
    if (!v) return null;
    return { jet: `Plain{carrier: LowerRouteCarrier.${v.jet}}`, ret: v.cty, outs: [], handles: v.handle ? [v.handle] : [], body: (call) => (v.cty ? v.wrap(call) : `${call}`) };
  }
  const args = splitTop(generic[2]);
  const listArm = (name, element, pattern, tag) =>
    `${pattern}(value) => { let len = list_out(value.into_iter().map(|e| ${element.write}).collect(), ${name}); unsafe { ${name}_len.write(len) }; ${tag} }`;
  if (generic[1] === "Option") {
    if (args.length !== 1) return null;
    const element = listOf(args[0]);
    if (element) {
      return {
        jet: `Option{some: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "some", cty: `*mut ${element.slot ?? "u64"}` }, { name: "some_len", cty: "i64" }], handles: element.handle ? [element.handle] : [], nativeOnly: element.nativeOnly,
        body: (call) => `match ${call} {\n            ${listArm("some", element, somePattern, 1)}\n            ${nonePattern} => 0,\n        }`,
      };
    }
    const v = valueCarrier(args[0], path, nativeInt, rawFixed);
    if (!v || !v.cty) return null;
    return {
      jet: `Option{some: LowerRouteCarrier.${v.jet}}`, ret: "i64", outs: [{ name: "some", cty: v.cty }], handles: v.handle ? [v.handle] : [],
      body: (call) => `match ${call} {\n            ${somePattern}(value) => { unsafe { some.write(${v.wrap("value")}) }; 1 }\n            ${nonePattern} => 0,\n        }`,
    };
  }
  if (args.length !== 2) return null;
  const okList = listOf(args[0]);
  const ok = okList ? null : valueCarrier(args[0], path, nativeInt, rawFixed);
  const errList = listOf(args[1]);
  const err = errList ? null : valueCarrier(args[1], path, false);
  if ((!ok && !okList) || (!errList && (!err || !err.cty))) return null;
  const outs = [];
  if (okList) outs.push({ name: "ok", cty: `*mut ${okList.slot ?? "u64"}` }, { name: "ok_len", cty: "i64" });
  else if (ok.cty) outs.push({ name: "ok", cty: ok.cty });
  if (errList) outs.push({ name: "err", cty: `*mut ${errList.slot ?? "u64"}` }, { name: "err_len", cty: "i64" });
  else outs.push({ name: "err", cty: err.cty });
  const okArm = okList
    ? listArm("ok", okList, "Ok", 1)
    : ok.cty ? `Ok(value) => { unsafe { ok.write(${ok.wrap("value")}) }; 1 }` : "Ok(()) => 1,";
  const okJet = okList ? `List{elem: LowerRouteCarrier.${okList.jet}}` : ok.jet;
  const errJet = errList ? `List{elem: LowerRouteCarrier.${errList.jet}}` : err.jet;
  const errArm = errList
    ? listArm("err", errList, "Err", 0)
    : `Err(error) => { unsafe { err.write(${err.wrap("error")}) }; 0 }`;
  return {
    jet: `Result{ok: LowerRouteCarrier.${okJet}, err: LowerRouteCarrier.${errJet}}`, ret: "i64", outs, handles: [ok?.handle, okList?.handle, err?.handle, errList?.handle].filter(Boolean),
    nativeOnly: Boolean(okList?.nativeOnly || errList?.nativeOnly),
    body: (call) => `match ${call} {\n            ${okArm}\n            ${errArm}\n        }`,
  };
}

// ---------------------------------------------------------------------------
// 4. Adapters.

const adapters = [];
const skipped = [];
const exported = new Set(handExports);
const handles = new Map();
const handAbi = new Map();
for (const match of handTable.matchAll(/pub(?:\(crate\))? (?:unsafe )?extern "C" fn ([A-Za-z_]\w*)\s*([^{}]*?)\{/g)) {
  handAbi.set(match[1], parseSignature(`fn ${match[1]}${match[2]}`));
}
for (const match of handTable.matchAll(/exact_int_(binary|located)!\s*\{?\(?([^)}]*)/g)) {
  const macro = new RegExp(`macro_rules! exact_int_${match[1]}[\\s\\S]*?pub extern "C" fn \\$name([^{}]*?)\\{`).exec(handTable);
  if (!macro) throw new Error(`gen-c-abi-routes: no exact_int_${match[1]} ABI`);
  for (const name of match[2].split(",").map((part) => part.trim()).filter(Boolean)) handAbi.set(name, parseSignature(`fn ${name}${macro[1]}`));
}
for (const [symbol, route] of [...routes].sort((a, b) => a[0].localeCompare(b[0]))) {
  classificationSite = { phase: "route", symbol };
  // An exact-Int adapted symbol calls its owned kernel (MIRRust.rs
  // call_symbol_for_function); the route keeps the symbol.
  const target = exactAdapters.get(symbol) ?? symbol;
  const segments = symbol.split("::");
  // The logical Rust/kernel route stays nominal; only its internal C export
  // projects type atoms (`JetDateTime::date` -> `jet_date_time_date`).
  const name = runtimeAbiAtom(segments.length > 1 && /^[A-Z]/.test(segments[segments.length - 2]) ? segments.slice(-2).join("_") : segments[segments.length - 1]);
  const label = route.rows[0];
  if (handExports.has(name)) {
    if (jetRuntimeExports.has(name)) {
      const sig = handAbi.get(name);
      if (!sig || sig.generics) throw new Error(`Jet runtime route ${symbol}: no canonical hand C ABI for ${name}`);
      const params = splitTop(sig.params).map((param) => ({ ctys: [param.slice(param.indexOf(":") + 1).trim()] }));
      const result = { ret: sig.result === "()" ? null : sig.result, outs: [] };
      validateRuntimeRoute({ symbol, name, params, result }, jetRuntimeSignatures.get(name));
    }
    continue;
  }
  const skip = (reason) => {
    if (jetRuntimeExports.has(name)) {
      classificationFailure(new Error(`Jet runtime route ${symbol}: cannot validate canonical C ABI: ${reason}`), classificationSite?.signature ?? symbol, classificationSite?.module ?? "");
    }
    skipped.push(`${symbol} (${label}): ${reason}`);
  };
  if (exported.has(name)) { skip("export name taken"); continue; }
  const all = (fns.get(target) ?? []).filter((def) => !def.wasmOnly);
  if (all.length === 0) { skip("no runtime function"); continue; }
  const defs = all.filter((def) => !def.gated);
  if (defs.length === 0) { skip("platform-gated"); continue; }
  if (new Set(defs.map((def) => def.sig)).size !== 1) { skip("cfg-dependent signature"); continue; }
  const def = defs[0];
  const path = def.path;
  classificationSite = { phase: "route", symbol, target, signature: def.sig, module: path, offset: def.at };
  if (path && !def.pub) { skip(`not public in ${path}`); continue; }
  // A method's receiver is its first parameter; `Self` is the impl's type.
  const selfSig = def.selfType
    ? def.sig.replace(/\((\s*)(&\s*mut\s+|&\s*|mut\s+)?self\b/, (_, space, ref) => `(${space}self: ${(ref ?? "").replace(/^mut\s+$/, "")}Self`).replace(/\bSelf\b/g, def.selfType)
    : def.sig;
  const sig = parseSignature(selfSig);
  if (!sig) { skip(`unparsed signature ${def.sig}`); continue; }
  if (sig.generics) {
    // A generic route has no C adapter to replace. A Jet runtime #Export(c) of
    // the same name is the native lowering's monomorphic ABI (the flattened
    // task join, the World callback's env/invoke/out), owned by that lowering.
    if (jetRuntimeExports.has(name)) skipped.push(`${symbol} (${label}): generic; Jet runtime #Export(c) ${name} owns the lowered ABI`);
    else skip("generic");
    continue;
  }
  const params = splitTop(sig.params).map((param) => {
    const colon = param.indexOf(":");
    return colon < 0 ? null : paramCarrier(param.slice(colon + 1), path, symbol);
  });
  if (params.some((param) => param === null)) { skip(`parameter types ${sig.params}`); continue; }
  // A native-only carrier has no seed Rust adapter; a seed route keeps its skip.
  const jetProvider = jetRuntimeExports.has(name);
  if (!jetProvider && params.some((param) => param.nativeOnly)) { skip(`parameter types ${sig.params}`); continue; }
  const rawFixed = /^jet_reader_(?:read_[ui](?:8|16|32|64)(?:_le|_be)?|read_f32_(?:le|be)|peek)$/.test(name);
  const result = resultShape(sig.result, path, rawFixed ? false : route.nativeInt, rawFixed);
  if (!result || (!jetProvider && result.nativeOnly)) { skip(`result type ${sig.result}`); continue; }
  if (jetRuntimeExports.has(name) && (name === "jet_reader_over" || name === "jet_reader_over_owned")) {
    if (params.length !== 1 || params[0].ctys.length !== 2) throw new Error(`${name}: Reader.over must have one checked byte List parameter`);
    // Logical metadata stays one List; Lower appends the checked 1|8 stride.
    params[0].ctys.push("i64");
  } else if (jetRuntimeExports.has(name) && name === "jet_reader_take") {
    if (params.length !== 2 || params[1].ctys.length !== 1) throw new Error(`${name}: Reader.take must have handle and length parameters`);
    // The target element stride precedes ok-data/ok-length/error out slots.
    params[1].ctys.push("i64");
  }
  // A direct #Export(c) is the Jet adapter. Check the entire flattened ABI,
  // including Option/Result/Tuple/List out slots, before dropping its Rust body.
  if (jetRuntimeExports.has(name)) validateRuntimeRoute({ symbol, name, params, result }, jetRuntimeSignatures.get(name));
  exported.add(name);
  for (const handle of [...params.map((param) => param.handle).filter(Boolean), ...result.handles]) {
    const known = handles.get(handle.name);
    if (known && known !== handle.full) { skip(`handle ${handle.name} names two types`); exported.delete(name); }
    else handles.set(handle.name, handle.full);
  }
  if (!exported.has(name)) continue;
  // A nested native List carrier crosses only to its Jet provider: emit the
  // route row without a Rust adapter body.
  const external = params.some((param) => param.nativeOnly) || Boolean(result.nativeOnly);
  adapters.push({ symbol, target, name, label, params, result, rows: route.rows, ...(external ? { external: true } : {}) });
}
classificationSite = null;

// Finite scalar specializations of the existing canonical codec intrinsics.
// These adapters marshal native storage only; Encode/Decode implementations
// (including exact-number acceptance and diagnostics) remain the sole policy.
const codecScalars = [
  ["Int", "crate::jet_foundation::Numeric::JetInt", "Word", "i64", (a) => `unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }`, (v) => `${v}.into_raw()`],
  ["Bool", "bool", "Flag", "u8", (a) => `${a} != 0`, (v) => `${v} as u8`],
  ["Float", "f64", "Float", "f64", (a) => a, (v) => v],
  ["String", "String", "Text", "JetCString", (a) => `view(${a}).to_owned()`, (v) => `handle(${v})`],
  ["Char", "char", "Word", "i64", (a) => `scalar(${a})`, (v) => `${v} as i64`],
  ...[...fixedInts, "i64"].filter((ty) => ty !== "usize" && ty !== "isize").map((ty) => [ty.toUpperCase(), ty, "Word", "i64", (a) => `${a} as ${ty}`, (v) => `${v} as i64`]),
];
for (const [name, rust, jet, cty, read, write] of codecScalars) {
  const tree = { name: "Dyn", full: "crate::jet_std::Dyn" };
  const error = { name: "FieldError", full: "crate::jet_std::FieldError" };
  handles.set(tree.name, tree.full);
  handles.set(error.name, error.full);
  const encode = `jet_rt_codec_encode_${runtimeAbiAtom(name)}`;
  const decode = `jet_rt_codec_decode_${runtimeAbiAtom(name)}`;
  adapters.push({
    symbol: encode, target: "jet_codec_encode", name: encode, label: "canonical scalar Encode", rows: [],
    params: [{ jet, ctys: [cty], expr: ([a]) => name === "String" ? `0, ${paramCarrier("&String", "").expr([a])}` : `0, &(${read(a)})` }],
    result: { jet: 'Plain{carrier: LowerRouteCarrier.Handle{name: "Dyn"}}', ret: "*mut crate::jet_std::Dyn", outs: [], handles: [tree],
      body: (call) => `Box::into_raw(Box::new(${call}))` },
  });
  adapters.push({
    symbol: decode, target: `jet_codec_decode::<${rust}>`, name: decode, label: "canonical scalar Decode", rows: [],
    params: [{ jet: 'Handle{name: "Dyn"}', ctys: ["*mut crate::jet_std::Dyn"], expr: ([a]) => `0, unsafe { &*${a} }`, handle: tree }],
    result: { jet: `Result{ok: LowerRouteCarrier.${jet}, err: LowerRouteCarrier.List{elem: LowerRouteCarrier.Handle{name: "FieldError"}}}`, ret: "i64",
      outs: [{ name: "ok", cty }, { name: "err", cty: "*mut u64" }, { name: "err_len", cty: "i64" }], handles: [error],
      body: (call) => `match ${call} {\n            Ok(value) => { unsafe { ok.write(${write("value")}) }; 1 }\n            Err(errors) => { let n = list_out(errors.into_iter().map(|e| Box::into_raw(Box::new(e)) as u64).collect(), err); unsafe { err_len.write(n) }; 0 }\n        }` },
  });
}

// Parse-to-tree specialization retains the typed intrinsic's exact parser and
// FieldError envelope, then the native backend invokes the checked Decode impl.
for (const format of ["json", "toml", "yaml"]) {
  const symbol = `jet_rt_codec_parse_${runtimeAbiAtom(format)}`;
  const result = resultShape("Result<jet_std::Dyn,Vec<jet_std::FieldError>>", "", false);
  if (!result) throw new Error(`gen-c-abi-routes: missing ${format} typed tree carriers`);
  adapters.push({ symbol, target: `jet_enc_${format}_decode::<crate::jet_std::Dyn>`, name: symbol,
    label: `canonical typed ${format.toUpperCase()} parse`, rows: [],
    params: [paramCarrier("&String", "")], result });
}
{
  const symbol = "jet_rt_codec_under";
  const params = [paramCarrier("&str", ""), paramCarrier("Vec<jet_std::FieldError>", "")];
  const result = resultShape("Vec<jet_std::FieldError>", "", false);
  if (params.some((param) => !param) || !result) throw new Error("gen-c-abi-routes: missing FieldError prefix carriers");
  adapters.push({ symbol, target: "jet_std::FieldError::under_errors", name: symbol,
    label: "canonical FieldError path prefix", rows: [], params, result });
}

// Numeric lowering has a raw, source-owned CAbi export: the fixed-width value
// stays raw, while the operation/width exact-Int words are decoded by that ABI.
// Enumerate it without generating a second wrapper under its exported name.
adapters.push({
  symbol: "jet_numeric_bit_count", name: "jet_numeric_bit_count", external: true,
  rows: ["source numeric bit count"], params: Array.from({ length: 3 }, () => ({ jet: "Word" })),
  result: { jet: "Plain{carrier: LowerRouteCarrier.Word}" },
});
// Duration uses the existing saturating i64 kernel on raw nanoseconds, not
// exact-Int owner words. Its scalar ABI must not insert tag conversion.
{
  const symbol = "jet_i64_saturating_add";
  const row = { symbol, target: symbol, name: symbol, label: "raw duration nanoseconds", rows: ["source duration addition"],
    params: Array.from({ length: 2 }, () => ({ jet: "Word", ctys: ["i64"], expr: ([value]) => value })),
    result: { jet: "Plain{carrier: LowerRouteCarrier.Word}", ret: "i64", outs: [], handles: [], body: (call) => call },
  };
  const index = adapters.findIndex((adapter) => adapter.symbol === symbol);
  if (index < 0) adapters.push(row); else adapters[index] = row;
}

// The raw tagged-Int destructor is callable only after its Jet layer owns the
// export. Earlier E1/E4 runtime epochs must not acquire a dangling route.
if (jetRuntimeExports.has("jet_rt_int_drop") && !adapters.some((adapter) => adapter.name === "jet_rt_int_drop")) {
  const adapter = {
    symbol: "jet_rt_int_drop", name: "jet_rt_int_drop", external: true,
    params: [{ jet: "Word", ctys: ["i64"] }],
    result: { jet: "Plain{carrier: LowerRouteCarrier.Unit}", ret: null, outs: [] },
    rows: ["JetRuntime.Int.drop"],
  };
  validateRuntimeRoute(adapter, jetRuntimeSignatures.get(adapter.name));
  adapters.push(adapter);
}

// Native-only operations have no seed kernel to wrap. Their dispatch is typed
// in Routes.jet; their real Jet providers still validate against this ABI table.
// Weak signatures: Shared.jet@4f9428a3a, Word -> Word, Word/Word -> Flag,
// Word -> Unit. Enrollment does not admit a Rust Box carrier or seed body.
for (const [name, params, result, row] of [
  ["jet_rt_native_bits_new", [], { jet: 'Plain{carrier: LowerRouteCarrier.Handle{name: "JetBitSet"}}', ret: "i64", outs: [] }, "JetRuntime.BitSet.new"],
  ["jet_rt_native_bits_clear", [{ jet: 'HandleMut{name: "JetBitSet"}', ctys: ["i64"] }], { jet: "Plain{carrier: LowerRouteCarrier.Unit}", ret: null, outs: [] }, "JetRuntime.BitSet.clear"],
  ["jet_rt_native_shared_downgrade", [{ jet: "Word", ctys: ["i64"] }], { jet: "Plain{carrier: LowerRouteCarrier.Word}", ret: "i64", outs: [] }, "JetRuntime.Shared.downgrade"],
  ["jet_rt_native_shared_weak_upgrade", [{ jet: "Word", ctys: ["i64"] }, { jet: "Word", ctys: ["i64"] }], { jet: "Plain{carrier: LowerRouteCarrier.Flag}", ret: "bool", outs: [] }, "JetRuntime.Shared.weak_upgrade"],
  ["jet_rt_native_shared_weak_drop", [{ jet: "Word", ctys: ["i64"] }], { jet: "Plain{carrier: LowerRouteCarrier.Unit}", ret: null, outs: [] }, "JetRuntime.Shared.weak_drop"],
  // Physical Shared owner/alias/guard exports: accepted f4da51359+20314fd26.
  // Words are provider-owned addresses/status, not exact-Int conversions.
  ...[
    ["owner_strong_count", 1, "Word"], ["owner_pin", 2, "Flag"],
    ["pin_drop", 1, "Unit"], ["owner_alias", 2, "Flag"],
    ["alias_token", 1, "Word"], ["alias_drop", 1, "Unit"],
    ["guard_acquire", 2, "Word"], ["guard_clone", 1, "Word"],
    ["guard_held", 1, "Flag"], ["guard_root_ptr", 1, "Word"],
    ["guard_revision", 1, "Word"], ["guard_mark_dirty", 1, "Unit"],
    ["guard_publish_and_release", 1, "Unit"], ["guard_abort_release", 1, "Unit"],
    ["guard_reacquire_and_refresh", 2, "Flag"], ["guard_drop", 1, "Unit"],
    ["guard_wait_once", 2, "Word"],
  ].map(([operation, arity, carrier]) => [
    `jet_rt_shared_${runtimeAbiAtom(operation)}`,
    Array.from({ length: arity }, () => ({ jet: "Word", ctys: ["i64"] })),
    { jet: `Plain{carrier: LowerRouteCarrier.${carrier}}`,
      ret: carrier === "Unit" ? null : carrier === "Flag" ? "bool" : "i64", outs: [] },
    `JetRuntime.Shared.${operation}`,
  ]),
]) {
  if (!jetRuntimeExports.has(name)) continue;
  const adapter = { symbol: name, name, external: true, params, result, rows: [row] };
  validateRuntimeRoute(adapter, jetRuntimeSignatures.get(name));
  adapters.push(adapter);
}
// Core types the runtime holds natively: the Jet name -> native Rust type
// tables generated Rust spells its types with (Codegen/Context.rs and
// Syntax/predicates.rs, applied in MIRRust.rs `rust_type` order, with its
// canonical-Core probe `core_native_probe_name`). Every one whose native type
// is a plain runtime struct or enum is a handle type (drop, clone, render and
// equality adapters below), whether or not a route mentions it.
const syntaxConstants = new Map();
{
  const dir = `${repo}/crates/jet-foundation/src/Syntax`;
  for (const file of [`${repo}/crates/jet-foundation/src/Syntax.rs`, ...readdirSync(dir).filter((f) => f.endsWith(".rs")).map((f) => `${dir}/${f}`)]) {
    for (const m of readFileSync(file, "utf8").matchAll(/pub const ([A-Z][A-Z0-9_]*): &str = "([^"]*)";/g)) syntaxConstants.set(m[1], m[2]);
  }
}
// The `"A" | "B" => Some("T")` and `n if n == Syntax::A || n == "B" => Some("T")`
// arms of one name table (`Some(name)` maps a name to itself).
const nameTable = (source, name) => {
  const table = new Map();
  for (const m of fnBody(source, name).matchAll(/((?:"[^"]+"\s*\|\s*)*"[^"]+"|(?:Syntax::)?[A-Z][A-Z0-9_]*|n if [^=>{}]*==[^>{}]*?)\s*=>\s*Some\((?:"([^"]+)"|name)\)/g)) {
    const names = m[1].startsWith("n if")
      ? [...m[1].matchAll(/Syntax::([A-Z][A-Z0-9_]*)|"([^"]+)"/g)].map((k) => k[2] ?? syntaxConstants.get(k[1]))
      : m[1].startsWith('"') ? [...m[1].matchAll(/"([^"]+)"/g)].map((k) => k[1])
      : [syntaxConstants.get(m[1].replace(/^Syntax::/, ""))];
    for (const jetName of names) {
      if (!jetName) throw new Error(`gen-c-abi-routes: unresolved Syntax constant in ${name}: ${m[1]}`);
      if (!table.has(jetName)) table.set(jetName, m[2] ?? jetName);
    }
  }
  if (table.size === 0) throw new Error(`gen-c-abi-routes: empty name table ${name}`);
  return table;
};
const contextRs = readFileSync(`${repo}/crates/jet-codegen/src/Codegen/Context.rs`, "utf8");
const predicatesRs = readFileSync(`${repo}/crates/jet-foundation/src/Syntax/predicates.rs`, "utf8");
// MIRRust.rs `rust_apply_type`'s own argument-free spellings
// (`if name.name == X { return <root path> }`), checked before every table
// against the name as written (a canonical Core name never matches them).
const explicitTable = new Map();
for (const m of fnBody(mirRustRs, "rust_apply_type").matchAll(/if (?:args\.is_empty\(\) && )?name\.name == ([A-Za-z_:"]+)\s*\{\s*return (?:format!\(\s*"\{\}([A-Za-z_:]+)",\s*self\.config\.root_prefix\s*\)|self\.rust_root_prelude_type\("([A-Za-z_]+)",\s*args\))/g)) {
  const token = m[1];
  const constant = /([A-Z][A-Z0-9_]*)$/.exec(token)?.[1];
  const local = constant ? new RegExp(`\\bconst ${constant}: &str = "([^"]*)";`).exec(mirRustRs)?.[1] : null;
  const jetName = token.startsWith('"') ? token.slice(1, -1) : local ?? syntaxConstants.get(constant);
  if (!jetName) throw new Error(`gen-c-abi-routes: unresolved rust_apply_type name ${token}`);
  if (!explicitTable.has(jetName)) explicitTable.set(jetName, m[2] ?? m[3]);
}
if (explicitTable.size === 0) throw new Error("gen-c-abi-routes: no rust_apply_type spellings");
const rootPrelude = new Map([...nameTable(contextRs, "root_prelude_rust_type_name"), ...nameTable(contextRs, "job_queue_rust_type"), ...nameTable(contextRs, "net_handle_rust_type")].reverse());
const coreTable = nameTable(contextRs, "core_rust_type_name");
const fileTable = nameTable(contextRs, "file_handle_rust_type");
const emailTable = nameTable(contextRs, "core_email_rust_type_name");
const leafTables = [
  nameTable(predicatesRs, "reflect_handle_rust_type"),
  nameTable(predicatesRs, "binary_text_handle_rust_type"),
];
const lateLeafTables = [
  nameTable(contextRs, "compute_handle_rust_type"),
  nameTable(contextRs, "service_handle_rust_type"),
  nameTable(predicatesRs, "alloc_handle_rust_type"),
  nameTable(contextRs, "layout_handle_rust_type"),
];
// Canonical Core types whose own Jet shape generated Rust keeps: MIRRust.rs
// core_native_probe_name's explicit tests plus the generic source-owned switch
// (CoreModuleExports.rs CORE_SOURCE_OWNED_TYPES, `core_source_owns_type`).
const sourceOwned = [];
for (const m of fnBody(mirRustRs, "core_native_probe_name").matchAll(/starts_with\("([^"]+)"\)(?:\s*&&\s*(?:name\.ends_with\("([^"]+)"\)|matches!\(\s*[^,]+,\s*((?:"[^"]+"\s*\|?\s*)+)\)))?/g)) {
  if (m[1] === "<corelib>/") continue;
  sourceOwned.push({ prefix: m[1], suffix: m[2], leaves: m[3] ? [...m[3].matchAll(/"([^"]+)"/g)].map((k) => k[1]) : null });
}
const ownedTypes = new Set();
for (const m of readFileSync(`${repo}/crates/jet-foundation/src/CoreModuleExports.rs`, "utf8").matchAll(/CoreSourceOwnedType \{ path: "([^"]+)", name: "([^"]+)" \}/g)) ownedTypes.add(`${m[1]}::${m[2]}`);
if (ownedTypes.size === 0) throw new Error("gen-c-abi-routes: no CORE_SOURCE_OWNED_TYPES rows");
// A Core module identity is its module name under the Core root
// (`<corelib>/core.net`); its source path comes from the CoreSourceOwned table.
const coreModulePath = new Map();
const corePathModule = new Map();
for (const m of readFileSync(`${repo}/Compiler/JetFoundation/Source/Registry/CoreSourceOwned.jet`, "utf8").matchAll(/module_name: "([^"]+)", alias: "[^"]*", path: "([^"]+)"/g)) {
  coreModulePath.set(m[1], m[2]);
  corePathModule.set(m[2], m[1]);
}
const coreInModule = (name, module) => name === `<corelib>/${module}` || name.startsWith(`<corelib>/${module}::`) || name.startsWith(`<corelib>/${module}.`);
const coreLeaf = (name) => name.slice("<corelib>/".length).split("::").pop();
const ownsType = (name) => {
  const parts = name.startsWith("<corelib>/") ? name.slice("<corelib>/".length).split("::") : [];
  const path = parts.length >= 2 ? coreModulePath.get(parts[0]) : undefined;
  return path !== undefined && ownedTypes.has(`${path}::${parts[parts.length - 1]}`);
};
const isSourceOwned = (name) => ownsType(name) || sourceOwned.some((rule) => name.startsWith(rule.prefix) && (!rule.suffix || name.endsWith(rule.suffix)) && (!rule.leaves || rule.leaves.includes(coreLeaf(name))));
const sourceNominalReferences = new Set();
for (const definitions of fns.values()) {
  for (const definition of definitions) {
    for (const name of definition.sig.match(/\b__jet_[A-Za-z0-9_]+\b/g) ?? []) sourceNominalReferences.add(name);
    if (definition.selfType?.startsWith("__jet_")) sourceNominalReferences.add(definition.selfType);
  }
}
// The native Rust type path (from the crate root) of a Jet type name, as
// MIRRust.rs `rust_type` resolves an argument-free nominal; null if none.
const nativeType = (name) => {
  const canonical = name.startsWith("<corelib>/");
  // Actual source nominal declarations, not marker structs or retired Rust
  // carriers. Nongeneric ownership also needs the source rule or a typed
  // source use; generic concrete uses are discovered from signatures below.
  const generated = canonical ? generatedRecordPath(name) : null;
  const declaration = generated ? declarationOf(generated) : null;
  if (declaration?.decls.length === 1 && (declaration.decls[0].generic || isSourceOwned(name) || sourceNominalReferences.has(generated))) return declaration.key;
  const native = canonical && !isSourceOwned(name) ? coreLeaf(name) : name;
  if (explicitTable.has(name)) return explicitTable.get(name);
  const fileName = /^<corelib>\/core\.(files|term)::/.test(name) ? coreLeaf(name) : name;
  if (fileTable.has(fileName)) return fileTable.get(fileName);
  for (const table of leafTables) if (table.has(native)) return table.get(native);
  const emailName = name.replace(/^<corelib>\/core\.email::/, "");
  if (emailTable.has(emailName)) return `jet_email::${emailTable.get(emailName)}`;
  if (rootPrelude.has(native)) return rootPrelude.get(native);
  for (const table of lateLeafTables) if (table.has(native)) return table.get(native);
  if (canonical && coreInModule(name, "core.sync") && name.endsWith("::RowPolicy")) return null;
  const core = coreTable.get(canonical ? coreLeaf(name) : name);
  if (!core) return null;
  if (core === "JetRowPolicy" || core === "JetTaskFailure") return core;
  return rootPrelude.get(core) ?? `jet_std::${core}`;
};
// A native type path the adapters can hold by handle: a handle a route
// already holds at that path, or one visible, ungated runtime declaration
// with concrete arguments substituted at the boundary.
const handleOf = (rustPath) => {
  if (!rustPath || !opaqueName.test(rustPath)) return null;
  const application = recordApplication(rustPath);
  if (!application) return null;
  if (application.args.length > 0) return runtimeHandle(rustPath, "");
  const name = application.base.split("::").pop();
  if (handles.get(name) === `crate::${rustPath}`) return { name, full: `crate::${rustPath}` };
  const found = declarationOf(rustPath);
  if (!found || found.decls.length !== 1) return null;
  const [decl] = found.decls;
  // A bare name-table entry for a generic is not a concrete ABI request.
  if (!decl.visible || decl.generic || decl.gated) return null;
  return { name: found.key.split("::").pop(), full: `crate::${found.key}` };
};
// Every Jet name of a native handle type: the table names, plus the canonical
// name of each Core source type that resolves to a native type.
const jetNames = new Map(); // Jet name -> runtime handle name
{
  const candidates = new Set([...explicitTable.keys(), ...fileTable.keys(), ...emailTable.keys(), ...rootPrelude.keys(), ...coreTable.keys(), ...[...leafTables, ...lateLeafTables].flatMap((table) => [...table.keys()])]);
  const coreDir = `${repo}/Core`;
  for (const file of readdirSync(coreDir, { recursive: true }).filter((f) => f.endsWith(".jet")).sort()) {
    const rel = `Core/${file}`;
    for (const m of readFileSync(`${coreDir}/${file}`, "utf8").matchAll(/^\s*(?:struct|enum) ([A-Z][A-Za-z0-9_]*)\b/gm)) {
      const module = corePathModule.get(rel);
      if (module) candidates.add(`<corelib>/${module}::${m[1]}`);
    }
  }
  // Pure Core source functions can mention concrete generic records without
  // a C route. Discover those applications from their emitted signatures too.
  for (const jetName of [...candidates].sort()) {
    const target = nativeType(jetName);
    const declaration = target ? declarationOf(target) : null;
    if (!declaration || declaration.decls.length !== 1 || !declaration.decls[0].generic) continue;
    const base = target.split("::").pop();
    for (const defs of fns.values()) {
      for (const def of defs) {
        const signature = parseSignature(def.sig);
        if (!signature || signature.generics || def.gated || def.wasmOnly) continue;
        classificationSite = { phase: "source generic", signature: def.sig, module: def.path, offset: def.at };
        for (const application of recordApplicationsIn(def.sig, base)) {
          const handle = runtimeHandle(application, def.path);
          if (!handle) continue; // Classified unadmitted carriers remain in the inventory.
          const known = handles.get(handle.name);
          if (known && known !== handle.full) throw new Error(`generic record ${application}: handle key names two declarations`);
          handles.set(handle.name, handle.full);
        }
      }
    }
  }
  // Two spellings of one declaration (`crate::X` and, through a glob import,
  // `crate::jet_std::X`) are one handle type: the first spelling stays.
  const sameType = (a, b) => {
    const left = declarationOf(a.slice("crate::".length));
    return a === b || (!a.includes("<") && !b.includes("<") && left !== null && left.key === declarationOf(b.slice("crate::".length))?.key);
  };
  for (const jetName of [...candidates].sort()) {
    const handle = handleOf(nativeType(jetName));
    if (!handle) continue;
    const known = handles.get(handle.name);
    if (known && !sameType(known, handle.full)) { skipped.push(`handle type ${jetName} -> ${handle.full}: ${handle.name} names two types`); continue; }
    if (!known) handles.set(handle.name, handle.full);
    jetNames.set(jetName, handle.name);
  }
  for (const jetName of [...candidates].sort()) {
    for (const instance of concreteHandles.values()) {
      if (!handles.has(instance.name)) continue;
      const target = nativeType(jetName);
      if (!target || declarationOf(target)?.key !== declarationOf(instance.full.slice(7))?.key) continue;
      const labels = instance.args.map((arg) => {
        if (genericArgumentKinds.has(arg)) return genericArgumentKinds.get(arg);
        const key = declarationOf(arg.slice("crate::".length))?.key;
        const names = [...jetNames].filter(([, runtime]) => {
          const full = handles.get(runtime);
          return full === arg || key !== undefined && full !== undefined && declarationOf(full.slice("crate::".length))?.key === key;
        }).map(([name]) => name).sort((a, b) => a.localeCompare(b));
        return names.find((name) => name.startsWith("<corelib>/")) ?? names[0];
      });
      if (labels.some((label) => !label)) {
        classificationFailure(new Error(`generic record ${instance.full}: argument has no Jet runtime identity`), instance.full, "", { phase: "source identity", jetName });
        continue;
      }
      jetNames.set(`${jetName}<${labels.join(",")}>`, instance.name);
    }
  }
}

// Handle drops: Core types that cross the boundary by handle.
const handleRows = [...handles].sort((a, b) => a[0].localeCompare(b[0]));
for (const [name, full] of handleRows) {
  if (handleBridge(full) && (!jetRuntimeExports.has(`jet_rt_handle_drop_${runtimeAbiAtom(name)}`) || !jetRuntimeExports.has(`jet_rt_handle_clone_${runtimeAbiAtom(name)}`))) {
    throw new Error(`${name} carrier requires Jet clone/drop exports in sources.list before adapter cutover`);
  }
}
// One ownership gate for every generated handle helper. Carrier metadata
// remains unchanged; a Jet provider must implement this exact physical ABI.
const jetOwnsHandleGlue = (symbol, parameters, result) => {
  if (!jetRuntimeExports.has(symbol)) return false;
  validateRuntimeRoute({ symbol, name: symbol,
    params: parameters.map((type) => ({ ctys: [type] })),
    result: { ret: result, outs: [] } }, jetRuntimeSignatures.get(symbol));
  return true;
};

const rs = [];
rs.push("// BEGIN GENERATED C-ABI ROUTES (Tools/agent/gen-c-abi-routes.mjs; do not hand-edit)");
rs.push("// Rust adapters for Core routes with a C carrier (CAbi.rs carrier rules).");
rs.push("// Jet runtime #Export(c) definitions replace their adapters; carrier metadata");
rs.push("// remains shared. Routes without a C carrier are listed at the end.");
rs.push("#[cfg(not(target_arch = \"wasm32\"))]");
rs.push("#[allow(non_snake_case, clippy::all)]");
rs.push("mod jet_c_abi_routes {");
rs.push("    use super::jet_c_abi::{borrowed, guard, handle, jet_rt_alloc, native, view, JetCString};");
rs.push("");
rs.push("    #[cold]");
rs.push("    fn range_stop() -> ! {");
rs.push("        super::jet_arithmetic_stop(\"<core.prelude>\", 0, \"native Int argument exceeds host range\")");
rs.push("    }");
rs.push("");
rs.push("    /// `native_int_input` narrowed to a kernel's fixed-width parameter.");
rs.push("    fn fixed<T: TryFrom<i64>>(value: i64) -> T {");
rs.push("        T::try_from(native(value)).unwrap_or_else(|_| range_stop())");
rs.push("    }");
rs.push("");
rs.push("    /// A Char word: its Unicode scalar value (as jet_rt_char_to_string reads it).");
rs.push("    fn scalar(value: i64) -> char {");
rs.push("        u32::try_from(value).ok().and_then(char::from_u32).unwrap_or(char::REPLACEMENT_CHARACTER)");
rs.push("    }");
rs.push("");
rs.push("    /// A List argument uses its declared fixed-integer width or word carrier.");
rs.push("    fn list_in<S: Copy, T>(data: *const S, len: i64, item: impl Fn(S) -> T) -> Vec<T> {");
rs.push("        if len <= 0 || data.is_null() {");
rs.push("            return Vec::new();");
rs.push("        }");
rs.push("        // SAFETY: the caller passes its List's slot buffer and length.");
rs.push("        unsafe { std::slice::from_raw_parts(data, len as usize) }.iter().map(|w| item(*w)).collect()");
rs.push("    }");
rs.push("");
rs.push("    /// A List result: a slot buffer from the allocator `jet_rt_free` returns");
rs.push("    /// to (null when empty), written to `data`; returns the length.");
rs.push("    fn list_out<S: Copy>(words: Vec<S>, data: *mut *mut S) -> i64 {");
rs.push("        let len = words.len();");
rs.push("        let buffer = if len == 0 {");
rs.push("            std::ptr::null_mut()");
rs.push("        } else {");
rs.push("            let layout = std::alloc::Layout::from_size_align(len * std::mem::size_of::<u64>(), std::mem::align_of::<u64>()).unwrap_or_else(|_| range_stop());");
rs.push("            // SAFETY: `layout` has a nonzero size.");
rs.push("            let buffer = unsafe { jet_rt_alloc(layout.size(), layout.align()) } as *mut u64;");
rs.push("            if buffer.is_null() {");
rs.push("                std::alloc::handle_alloc_error(layout);");
rs.push("            }");
rs.push("            // SAFETY: `buffer` holds `len` slots.");
rs.push("            unsafe { std::ptr::copy_nonoverlapping(words.as_ptr(), buffer, len) };");
rs.push("            buffer");
rs.push("        };");
rs.push("        // SAFETY: the caller passes a writable out slot.");
rs.push("        unsafe { data.write(buffer) };");
rs.push("        len as i64");
rs.push("    }");
const cloneable = new Set(handleRows.filter(([, full]) => derivesClone(full)).map(([name]) => name));
for (const [name, full] of handleRows) {
  const pointer = `*mut ${handleType(full)}`;
  if (!jetOwnsHandleGlue(`jet_rt_handle_drop_${runtimeAbiAtom(name)}`, [pointer], null)) {
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_handle_drop_${runtimeAbiAtom(name)}(value: ${pointer}) {`);
    rs.push("        if !value.is_null() {");
    rs.push("            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.");
    rs.push(handleBridge(full) ? `            super::jet_c_abi_host::${handleBridge(full).stem}_drop(value);` : "            drop(unsafe { Box::from_raw(value) });");
    rs.push("        }");
    rs.push("    }");
  }
  const ownedClone = jetOwnsHandleGlue(`jet_rt_handle_clone_${runtimeAbiAtom(name)}`, [pointer], pointer);
  if (!cloneable.has(name) || ownedClone) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_handle_clone_${runtimeAbiAtom(name)}(value: *mut ${handleType(full)}) -> *mut ${handleType(full)} {`);
  rs.push("        // SAFETY: `value` is a live handle the caller owns for this call.");
  rs.push(`        guard(|| ${handleOut(full, handleCopy(full, "value"))})`);
  rs.push("    }");
}
// Display, show and debug text of a handle: the runtime type's own
// JetDisplay / JetShow / JetDebug implementation, as a new owned String handle.
const rendered = (name, full) => {
  const runtimeName = full.split("::").pop();
  return { display: renders.JetDisplay.has(runtimeName), show: renders.JetShow.has(runtimeName), debug: renders.JetDebug.has(runtimeName) };
};
for (const [name, full] of handleRows) {
  for (const [kind, trait, method] of [["display", "JetDisplay", "jet_display"], ["show", "JetShow", "jet_show"], ["debug", "JetDebug", "jet_debug"]]) {
    const owned = jetOwnsHandleGlue(`jet_rt_handle_${runtimeAbiAtom(kind)}_${runtimeAbiAtom(name)}`, [`*mut ${handleType(full)}`], "JetCString");
    if (!rendered(name, full)[kind] || owned) continue;
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_handle_${runtimeAbiAtom(kind)}_${runtimeAbiAtom(name)}(value: *mut ${handleType(full)}) -> JetCString {`);
    rs.push("        // SAFETY: `value` is a live handle the caller lends for this call.");
    rs.push(`        guard(|| handle(<${full} as crate::${trait}>::${method}(${handleBorrow(full, "value")})))`);
    rs.push("    }");
  }
}
const comparable = new Set(handleRows.filter(([, full]) => derives(full, "PartialEq")).map(([name]) => name));
for (const [name, full] of handleRows) {
  const pointer = `*mut ${handleType(full)}`;
  const owned = jetOwnsHandleGlue(`jet_rt_handle_eq_${runtimeAbiAtom(name)}`, [pointer, pointer], "bool");
  if (!comparable.has(name) || owned) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_handle_eq_${runtimeAbiAtom(name)}(left: *mut ${handleType(full)}, right: *mut ${handleType(full)}) -> bool {`);
  rs.push("        // SAFETY: both are live handles the caller lends for this call.");
  rs.push(`        guard(|| ${handleBorrow(full, "left")} == ${handleBorrow(full, "right")})`);
  rs.push("    }");
}

// Core records (Compiler/JetBackend/Source/Lower/Records.jet): a MIR-defined
// Core type whose generated-Rust type is one of these runtime structs or enums
// crosses a route as that value's handle, converted member by member:
//   jet_rt_record_new_<R>(fields...) / jet_rt_record_get_<R>_<field>(h)   struct
//   jet_rt_record_tag_<R>(h) / jet_rt_record_new_<R>_<V>(payload?) /
//   jet_rt_record_get_<R>_<V>(h)                                          enum
// A member is a String, an Int (i64 or exact), a Bool, a Float, a fieldless
// runtime enum (its index), a nested record, or an Option / JetOutcome of a
// String, an Int or a record. Each schema row names its members' kinds, so the
// lowering crosses a Jet type only when every member means the same carrier.
// Records CAbi.rs writes by hand (their exports there) are not generated.
const recordable = new Map(); // runtime name -> { full, kind, path, members }
for (const [name, full] of handleRows) {
  const decl = recordDecl(full);
  if (!decl) {
    if (concreteHandles.has(name)) classificationFailure(new Error(`generic record ${full}: unsupported record declaration`), full, "", { phase: "record declaration" });
    continue;
  }
  // Source nominal members use the emitters' plain __jet_ prefix; the schema
  // uses Jet declaration names, while Rust projections keep emitted names.
  const memberName = (rust) => full.startsWith("crate::__jet_") ? rust.replace(/^__jet_/, "") : rust;
  const members = [];
  let ok = true;
  for (const item of decl.items) {
    if (decl.kind === "struct") {
      const field = /^(pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*([\s\S]+)$/.exec(item);
      // A struct in a module exposes only its `pub` fields to the adapters.
      if (!field || (decl.path && !field[1])) { ok = false; break; }
      members.push({ name: memberName(field[2]), rustName: field[2], ty: field[3].replace(/\s+/g, "") });
    } else {
      const variant = /^([A-Z][A-Za-z0-9_]*|__jet_[A-Za-z0-9_]+)\s*(?:\(([\s\S]*)\)|=\s*[-0-9]+)?$/.exec(item);
      if (!variant || (variant[2] !== undefined && splitTop(variant[2]).length !== 1)) { ok = false; break; }
      members.push({ name: memberName(variant[1]), rustName: variant[1], ty: variant[2] === undefined ? null : variant[2].replace(/\s+/g, "") });
    }
  }
  if (ok) recordable.set(name, { full, kind: decl.kind, path: decl.path, members });
  else if (concreteHandles.has(name)) classificationFailure(new Error(`generic record ${full}: inaccessible or unsupported stored members`), full, decl.path, { phase: "record stored members" });
}
// A member's carrier: { kind, ret, outCty, get(place), cty, make(param) }, or
// null when no export carries it. `place` is a Rust place of the member.
// Recursive Dyn collections cross as temporary owned Vec holders. These
// exports only marshal storage; ordering/duplicate policy remains in Jet.
const recordSequences = new Map();
const recordMember = (ty, path, site) => {
  const exact = /^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty);
  if (ty === "String") return { kind: "Text", ret: "JetCString", get: (v) => `handle(${v}.clone())`, cty: "JetCString", make: (p) => `view(${p}).to_owned()` };
  if (ty === "i64") return { kind: "Int", ret: "i64", get: (v) => `crate::jet_std::jet_int_from_i64(${v})`, cty: "i64", make: (p) => `native(${p})` };
  if (exact) return { kind: "Int", ret: "i64", get: (v) => `${v}.clone().into_raw()`, cty: "i64", make: (p) => `unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${p}) }` };
  if (ty === "bool") return { kind: "Flag", ret: "bool", get: (v) => `${v}`, cty: "u8", make: (p) => `${p} != 0` };
  if (ty === "f64") return { kind: "Float", ret: "f64", get: (v) => `${v}`, cty: "f64", make: (p) => p };
  const list = /^Vec<(.*)>$/.exec(ty);
  if (list) {
    const entries = /^\(String,([A-Z][A-Za-z0-9_:]*)\)$/.exec(list[1]);
    const elementTy = entries ? entries[1] : list[1];
    const element = elementTy === "u8" ? null : recordMember(elementTy, path, site);
    if (elementTy !== "u8" && !element?.kind.startsWith("Rec.")) return null;
    if (element?.bridge) throw new Error(`record ${site}: borrowed C-carrier sequence projections are unsupported`);
    const kind = `${entries ? "Entries" : "List"}.${element ? element.kind : "Byte"}`;
    const elementFull = element ? element.cty.replace(/^\*mut /, "") : "u8";
    const full = `Vec<${entries ? `(String, ${elementFull})` : elementFull}>`;
    const prefix = `jet_rt_record_seq_${runtimeAbiAtom(kind.replaceAll(".", "_"))}`;
    recordSequences.set(kind, { kind, prefix, full, entries: !!entries, element, elementFull });
    return { kind, ret: `*mut ${full}`, get: (v) => `&${v} as *const ${full} as *mut ${full}`,
      cty: `*mut ${full}`, make: (p) => `*unsafe { Box::from_raw(${p}) }` };
  }
  const tag = tagCarrier(ty, path);
  if (tag) return { kind: `Tag.${opaqueName.exec(ty)[1]}`, ret: "i64", get: (v) => tag.write(`&${v}`), cty: "i64", make: tag.read };
  const option = /^(Option|JetOutcome)<(.+?)(?:,JetAbsent|,(?:crate::|super::)*JetAbsent)?>$/.exec(ty);
  if (option) {
    if (option[1] === "JetOutcome" && !/,(?:(?:crate|super)::)*JetAbsent>$/.test(ty)) return null;
    const inner = recordMember(option[2], path, site);
    if (!inner || !(inner.kind === "Text" || inner.kind === "Int" || inner.kind.startsWith("Rec."))) return null;
    const [some, none] = option[1] === "Option" ? ["Some", "None"] : ["Ok", "Err(crate::JetAbsent)"];
    const boxed = inner.kind.startsWith("Rec.") ? inner.cty : `*const ${inner.cty}`;
    return {
      kind: `Opt${inner.kind}`, ret: "i64", outCty: inner.ret,
      get: (v) => `match &${v} { ${some}(item) => { unsafe { some.write(${inner.get("(*item)")}) }; 1 } _ => 0 }`,
      cty: boxed,
      make: (p) => `if ${p}.is_null() { ${none} } else { ${some}(${inner.kind.startsWith("Rec.") ? inner.make(p) : inner.make(`unsafe { *${p} }`)}) }`,
    };
  }
  if (opaqueName.test(ty)) {
    const handle = runtimeHandle(ty, path);
    if (!handle) return null;
    const { name } = handle;
    const nested = handles.get(name);
    if (!nested || nested !== qualify(ty, path) && (!ty.includes("<") && declarationOf(nested.replace(/^crate::/, ""))?.key !== declarationOf(qualify(ty, path).replace(/^crate::/, ""))?.key)) return null;
    if (!cloneable.has(name)) return null;
    return { kind: `Rec.${name}`, ret: `*mut ${handleType(nested)}`, get: (v) => handleOut(nested, `${v}.clone()`), cty: `*mut ${handleType(nested)}`, make: (p) => handleTake(nested, p), bridge: handleBridge(nested) !== undefined };
  }
  return null;
};
// A record whose members all cross; a nested record must itself cross.
const recordMembers = new Map(); // runtime name -> [{ name, carrier | null }]
for (const [name, record] of recordable) {
  classificationSite = { phase: "record members", record: record.full };
  const carried = record.members.map((member) => ({ name: member.name, rustName: member.rustName, carrier: member.ty === null ? null : recordMember(member.ty, record.path, `${name}.${member.name}`) }));
  if (carried.every((member, k) => record.members[k].ty === null || member.carrier)) recordMembers.set(name, carried);
  else if (concreteHandles.has(name)) classificationFailure(new Error(`generic record ${record.full}: a substituted member has an unsupported carrier`), record.full, record.path);
}
for (let changed = true; changed;) {
  changed = false;
  for (const [name, members] of recordMembers) {
    const nestedKinds = members.map((member) => member.carrier?.kind.replace(/^(?:Opt|List\.|Entries\.)/, "") ?? "").filter((kind) => kind.startsWith("Rec."));
    if (nestedKinds.every((kind) => recordMembers.has(kind.slice(4)) || handExports.has(`jet_rt_record_new_${runtimeAbiAtom(kind.slice(4))}`))) continue;
    if (concreteHandles.has(name)) classificationFailure(new Error(`generic record ${recordable.get(name).full}: nested record schema is unsupported`), recordable.get(name).full, "", { phase: "nested record schema" });
    recordMembers.delete(name);
    changed = true;
  }
}
for (const { prefix, full, entries, element, elementFull } of recordSequences.values()) {
  rs.push("", "    #[no_mangle]");
  rs.push(`    pub extern "C" fn ${prefix}_new(capacity: i64) -> *mut ${full} { guard(|| Box::into_raw(Box::new(Vec::with_capacity(capacity as usize)))) }`);
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn ${prefix}_len(value: *mut ${full}) -> i64 { guard(|| unsafe { &*value }.len() as i64) }`);
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn ${prefix}_get(value: *mut ${full}, index: i64${entries ? ", key: *mut JetCString" : ""}) -> ${element ? element.cty : "u64"} {`);
  const place = entries ? "item.1" : "(*item)";
  rs.push(`        guard(|| { let item = &unsafe { &*value }[index as usize]; ${entries ? "unsafe { key.write(handle(item.0.clone())) }; " : ""}${element ? `&${place} as *const ${elementFull} as *mut ${elementFull}` : "*item as u64"} })`);
  rs.push("    }", "    #[no_mangle]");
  rs.push(`    pub extern "C" fn ${prefix}_push(value: *mut ${full}${entries ? ", key: JetCString" : ""}, item: ${element ? element.cty : "u64"}) {`);
  const item = element ? element.make("item") : "u8::try_from(item).unwrap_or_else(|_| range_stop())";
  rs.push(`        guard(|| unsafe { &mut *value }.push(${entries ? `(view(key).to_owned(), ${item})` : item}));`);
  rs.push("    }");
}
const recordSchemas = [];
for (const [name, members] of [...recordMembers].sort((a, b) => a[0].localeCompare(b[0]))) {
  const { full, kind } = recordable.get(name);
  const described = members.map((member) => (member.carrier ? `${member.name}=${member.carrier.kind}` : member.name)).join(",");
  recordSchemas.push(kind === "struct" ? { name, variants: "", fields: described } : { name, variants: described, fields: "" });
  // Hand-written carrier helpers still need the real field schema. Their
  // ownership changes where the definition lives, not the record's shape.
  if (handExports.has(`jet_rt_record_tag_${runtimeAbiAtom(name)}`) || handExports.has(`jet_rt_record_new_${runtimeAbiAtom(name)}`)) {
    const validate = (export_, ctys, ret) => {
      if (!jetRuntimeExports.has(export_)) return;
      validateRuntimeRoute({ symbol: export_, name: export_, params: ctys.map((cty) => ({ ctys: [cty] })), result: { ret, outs: [] } }, jetRuntimeSignatures.get(export_));
    };
    const pointer = `*mut ${handleType(full)}`;
    if (kind === "struct") {
      validate(`jet_rt_record_new_${runtimeAbiAtom(name)}`, members.map((member) => member.carrier.cty), pointer);
      for (const { name: field, carrier } of members) {
        validate(`jet_rt_record_get_${runtimeAbiAtom(name)}_${runtimeAbiAtom(field)}`, [pointer, ...(carrier.outCty ? [`*mut ${carrier.outCty}`] : [])], carrier.ret);
      }
    } else {
      validate(`jet_rt_record_tag_${runtimeAbiAtom(name)}`, [pointer], "i64");
      for (const { name: variant, carrier } of members) {
        validate(`jet_rt_record_new_${runtimeAbiAtom(name)}_${runtimeAbiAtom(variant)}`, carrier ? [carrier.cty] : [], pointer);
        if (carrier) validate(`jet_rt_record_get_${runtimeAbiAtom(name)}_${runtimeAbiAtom(variant)}`, [pointer, ...(carrier.outCty ? [`*mut ${carrier.outCty}`] : [])], carrier.ret);
      }
    }
    continue;
  }
  const getter = (export_, param, carrier, place) => {
    const out = carrier.outCty ? `, some: *mut ${carrier.outCty}` : "";
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn ${export_}(value: *mut ${handleType(full)}${out}) -> ${carrier.ret} {`);
    rs.push(`        // SAFETY: \`value\` is a live handle the caller lends${carrier.outCty ? "; `some` is its zeroed out slot" : " for this call"}.`);
    rs.push(`        guard(|| ${param} ${place})`);
    rs.push("    }");
  };
  if (kind === "struct") {
    // A fieldless struct carries nothing a schema row can check.
    if (members.length === 0) { recordSchemas.pop(); continue; }
    for (const member of members) getter(`jet_rt_record_get_${runtimeAbiAtom(name)}_${runtimeAbiAtom(member.name)}`, `{ let value = ${handleBorrow(full, "value")};`, member.carrier, `${member.carrier.get(`value.${member.rustName}`)} }`);
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_record_new_${runtimeAbiAtom(name)}(${members.map((member, k) => `m${k}: ${member.carrier.cty}`).join(", ")}) -> *mut ${handleType(full)} {`);
    rs.push(`        guard(|| ${handleOut(full, `${expressionType(full)} { ${members.map((member, k) => `${member.rustName}: ${member.carrier.make(`m${k}`)}`).join(", ")} }`)})`);
    rs.push("    }");
    continue;
  }
  // A fieldless enum crosses as its index: its schema row alone.
  if (members.every((member) => !member.carrier)) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_record_tag_${runtimeAbiAtom(name)}(value: *mut ${handleType(full)}) -> i64 {`);
  rs.push("        // SAFETY: `value` is a live handle the caller lends for this call.");
  rs.push(`        guard(|| match ${handleBorrow(full, "value")} { ${members.map((member, k) => `${expressionType(full)}::${member.rustName}${member.carrier ? "(_)" : ""} => ${k}`).join(", ")} })`);
  rs.push("    }");
  for (const member of members) {
    if (member.carrier) {
      getter(`jet_rt_record_get_${runtimeAbiAtom(name)}_${runtimeAbiAtom(member.name)}`, `match ${handleBorrow(full, "value")} { ${expressionType(full)}::${member.rustName}(item) =>`, member.carrier, `${member.carrier.get("(*item)")}, _ => crate::jet_panic("<core.prelude>", 0, "record variant index out of range") }`);
    }
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_record_new_${runtimeAbiAtom(name)}_${runtimeAbiAtom(member.name)}(${member.carrier ? `payload: ${member.carrier.cty}` : ""}) -> *mut ${handleType(full)} {`);
    rs.push(`        guard(|| ${handleOut(full, `${expressionType(full)}::${member.rustName}${member.carrier ? `(${member.carrier.make("payload")})` : ""}`)})`);
    rs.push("    }");
  }
}
// Field places of runtime handle structs lend addresses except String and
// canonical IP/socket carriers, which write a borrowed header to caller stack
// storage. The storage is read/clone-only: never freed, grown, moved or taken.
const handleFields = [];
for (const [name, full] of handleRows) {
  if (recordMembers.has(name)) continue;
  const decl = recordDecl(full);
  if (!decl || decl.kind !== "struct") continue;
  for (const item of decl.items) {
    const field = /^(pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*([\s\S]+)$/.exec(item);
    if (!field || (decl.path && !field[1])) continue;
    const ty = field[3].replace(/\s+/g, "");
    let kind = null;
    let pointee = null;
    let borrowedBridge = null;
    if (ty === "String") { kind = "Text"; pointee = "super::jet_c_abi::JetCText"; }
    else if (ty === "f64") { kind = "Float"; pointee = "f64"; }
    else if (ty === "bool") { kind = "Flag"; pointee = "bool"; }
    else if (opaqueName.test(ty) && !tagCarrier(ty, decl.path)) {
      const nested = opaqueName.exec(ty)[1];
      const held = handles.get(nested);
      if (held && declarationOf(held.replace(/^crate::/, ""))?.key === declarationOf(qualify(ty, decl.path).replace(/^crate::/, ""))?.key) {
        const bridge = handleBridge(held);
        if (bridge && (!bridge.borrowedBytes || full.startsWith("crate::__jet_"))) {
          classificationFailure(new Error(`field ${name}.${field[2]}: borrowed C-carrier projection requires an exact source-owned provider`),
            held, decl.path, { phase: "field place", handle: name, field: field[2] });
          continue;
        }
        borrowedBridge = bridge ?? null;
        kind = `Handle.${nested}`; pointee = handleType(held);
      }
    }
    if (!kind) continue;
    handleFields.push({ name, field: field[2], kind, headerBytes: borrowedBridge?.borrowedBytes ?? 0 });
    rs.push("");
    rs.push("    #[no_mangle]");
    if (kind === "Text") {
      rs.push(`    pub extern "C" fn jet_rt_handle_field_${runtimeAbiAtom(name)}_${runtimeAbiAtom(field[2])}(value: *mut ${full}, out: *mut ${pointee}) {`);
      rs.push("        // SAFETY: value lends an immutable field; out is a read/clone-only stack header, never freed/grown/taken.");
      rs.push(`        unsafe { let text = &(*value).${field[2]}; out.write(super::jet_c_abi::JetCText { ptr: text.as_ptr() as *mut u8, len: text.len(), cap: text.capacity() }); }`);
    } else if (borrowedBridge) {
      rs.push(`    pub extern "C" fn jet_rt_handle_field_${runtimeAbiAtom(name)}_${runtimeAbiAtom(field[2])}(value: *mut ${full}, out: *mut ${pointee}) {`);
      rs.push("        // SAFETY: caller owns read-only stack carrier storage; no owner clone or heap allocation.");
      rs.push(`        unsafe { out.write(super::jet_c_abi_host::${borrowedBridge.stem}_borrow_carrier(&(*value).${field[2]})); }`);
    } else {
      rs.push(`    pub extern "C" fn jet_rt_handle_field_${runtimeAbiAtom(name)}_${runtimeAbiAtom(field[2])}(value: *mut ${full}) -> *const ${pointee} {`);
      rs.push("        // SAFETY: value is a live handle the caller lends; the field outlives this read.");
      rs.push(`        unsafe { std::ptr::addr_of!((*value).${field[2]}) }`);
    }
    rs.push("    }");
  }
}
// Temporary representation seam (#19): removed when the checked Jet-owned CSV
// intrinsic/native Dyn+FieldError path replaces this canonical row kernel.
// No CSV, user-shape or row-error policy is reproduced here.
rs.push(`
    extern "C" {
        #[link_name = "jet_rt_alloc"]
        fn codec_alloc(size: usize, align: usize) -> *mut u8;
        #[link_name = "jet_rt_free"]
        fn codec_free(value: *mut u8, size: usize, align: usize);
    }
    /// Canonical CSV rows with a checked native Decode callback.
    /// invoke(borrowed_tree, value_out, tag_out, errors_out, length_out) -> OCB1
    /// consumed mask zero. Outs remain unwritten on delivered native Stop.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_codec_csv(
        text: JetCString, invoke: usize,
        meta: *const crate::jet_c_abi_iter::NativeMeta,
        values: *mut crate::jet_c_abi_iter::Header,
        errors: *mut *mut u64, errors_len: *mut i64,
    ) -> i64 {
        guard(|| {
            use crate::jet_c_abi_iter::{Meta, NativeValue, Header};
            let meta = Meta::copy(meta);
            let invoke: unsafe extern "C" fn(*mut crate::jet_std::Dyn, *mut u8, *mut u64, *mut *mut u64, *mut i64) -> u64 = std::mem::transmute(invoke);
            let rows = crate::jet_ring_csv_parse(${paramCarrier("&String", "").expr(["text"])}, &",".to_string(), false, false)
                .map_err(crate::jet_std::FieldError::one);
            let decoded = rows.and_then(|rows| crate::jet_std::jet_enc_csv_decode_rows(
                rows,
                |mut tree| {
                    let mut value = NativeValue::uninitialized(meta.clone());
                    let mut tag = std::mem::MaybeUninit::<u64>::uninit();
                    let mut errors = std::ptr::null_mut();
                    let mut count = 0i64;
                    let consumed = crate::jet_native_callback_body(|| invoke(&mut tree, value.slot_mut(), tag.as_mut_ptr(), &mut errors, &mut count));
                    assert_eq!(consumed, 0, "typed CSV lends its Dyn input");
                    if tag.assume_init() != 0 {
                        Ok(value.initialized())
                    } else {
                        let failures = if count == 0 {
                            assert!(errors.is_null(), "empty native FieldError handle array is null");
                            Vec::new()
                        } else {
                            let failures = std::slice::from_raw_parts(errors.cast::<*mut crate::jet_std::FieldError>(), count as usize)
                                .iter().map(|error| *Box::from_raw(*error)).collect();
                            codec_free(errors.cast(), count as usize * std::mem::size_of::<u64>(), 8);
                            failures
                        };
                        Err(failures)
                    }
                },
                crate::jet_std::FieldError::under_errors,
                crate::jet_std::Dyn::Text,
                crate::jet_std::Dyn::Object,
            ));
            match decoded {
                Ok(rows) => {
                    let length = rows.len();
                    let stride = meta.stride();
                    let data = if length == 0 { std::ptr::null_mut() } else { codec_alloc(length * stride, 8) };
                    for (index, row) in rows.into_iter().enumerate() { row.put(data.add(index * stride)); }
                    values.write(Header { len: length, cap: length, data });
                    1
                }
                Err(failures) => {
                    let length = failures.len();
                    let data = if length == 0 { std::ptr::null_mut() } else { codec_alloc(length * std::mem::size_of::<u64>(), 8).cast::<u64>() };
                    for (index, failure) in failures.into_iter().enumerate() { data.add(index).write(Box::into_raw(Box::new(failure)) as u64); }
                    errors.write(data);
                    errors_len.write(length as i64);
                    0
                }
            }
        })
    }
`);

// Source declarations are candidates, not a layout claim. Lower resolves the
// exact declaration + arguments in the actual MIR and refuses opaque runtime
// handles; this table merely supplies stable C names for that typed lookup.
const nativeTypes = [];
for (const [runtime] of handleRows) {
  if (!handles.get(runtime).startsWith("crate::__jet_")) continue;
  const names = [...jetNames].filter(([, target]) => target === runtime).map(([name]) => name).sort();
  for (const key of names) {
    const match = /^<corelib>\/([a-z0-9_.]+)::([A-Za-z_]\w*)(?:<(.*)>)?$/.exec(key);
    if (!match) continue;
    const [, module, name, argumentsText] = match;
    const path = coreModulePath.get(module);
    if (!path) continue;
    const source = readFileSync(`${repo}/${path}`, "utf8");
    if (!new RegExp(`\\b(?:struct|enum)\\s+${name}\\b`).test(source)) continue;
    const base = argumentsText === undefined ? key : key.slice(0, key.length - argumentsText.length - 2);
    const render = rendered(runtime, handles.get(runtime));
    const members = recordMembers.get(runtime);
    const record = members && recordSchemas.some((schema) => schema.name === runtime)
      ? { kind: recordable.get(runtime).kind, members: members.map((member) => ({ name: member.name, kind: member.carrier?.kind ?? null })) } : undefined;
    const fields = handleFields.filter((field) => field.name === runtime).map((field) => ({ name: field.field, kind: field.kind, headerBytes: field.headerBytes }));
    nativeTypes.push({ runtime, jetNames: names, jetName: key, source: { path, key: base },
      arguments: argumentsText ?? "", clone: cloneable.has(runtime), eq: comparable.has(runtime), ...render, record, fields });
    break;
  }
}
const seedHelperNames = new Set([...rs.join("\n").matchAll(/pub extern "C" fn ([A-Za-z_]\w*)\(/g)].map((match) => match[1]));
const nativeExports = [
  ...nativeHandleExports(nativeTypes).map((row) => ({ ...row, op: `handle.${row.op}`, member: "" })),
  ...nativeRecordExports(nativeTypes),
].filter((descriptor) => seedHelperNames.has(descriptor.name) && !jetRuntimeExports.has(descriptor.name));
const nativeSourceExports = new Set(nativeExports.map((row) => row.name));
// b5a331841's invariant is that no retained native-pack export touches a
// replaced Rust Box layout. Ownership is per symbol: a shared carrier may have
// native Jet providers and unavailable members. Every seed-only member is
// excluded from native-pack emission, never promoted to a Jet provider; its
// logical route remains so a native use reaches the existing E2104 linker
// refusal. The Rust-free pack producer independently rejects Rust symbols.
const seedOnlyExports = nativeSeedOnlyExports(seedHelperNames, adapters, jetRuntimeExports);
const sourceAdapterProofs = sourceOwnedAdapterProofs(adapters,
  readFileSync(`${repo}/crates/jet-foundation/src/CoreModuleExports.rs`, "utf8"),
  readFileSync(`${repo}/crates/jet-codegen/src/Codegen/TIR/routes.rs`, "utf8"),
  (path) => readFileSync(`${repo}/${path}`, "utf8"));
// Inspect every retained emission before either output mode. Only the
// explicitly partitioned seed bodies may be absent from native-pack output;
// every remaining use of a replaced Rust layout is a hard classification error.
const ownedBoxTypes = handleRows.filter(([name, full]) => !handleBridge(full) &&
  [...jetRuntimeExports].some((symbol) =>
    ["drop", "clone", "eq", "display", "show", "debug"].some((op) => symbol === `jet_rt_handle_${runtimeAbiAtom(op)}_${runtimeAbiAtom(name)}`) ||
    symbol.startsWith(`jet_rt_handle_field_${runtimeAbiAtom(name)}_`) ||
    symbol === `jet_rt_record_new_${runtimeAbiAtom(name)}` || symbol === `jet_rt_record_tag_${runtimeAbiAtom(name)}` ||
    symbol.startsWith(`jet_rt_record_new_${runtimeAbiAtom(name)}_`) || symbol.startsWith(`jet_rt_record_get_${runtimeAbiAtom(name)}_`)));
const requireUnownedBox = (name, symbol) => {
  classificationFailure(new Error(`all-or-none ownership for ${name}: retained Rust Box-layout emission ${symbol} requires a complete Jet-owned type family`),
    handles.get(name) ?? name, "", { phase: "type-family ownership", handle: name, symbol });
};
for (const [name, full] of ownedBoxTypes) {
  const escaped = full.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pointer = new RegExp(`\\*(?:mut|const)\\s+[^;{}\\n]*?${escaped}(?![A-Za-z0-9_:<])`);
  for (const match of rs.join("\n").matchAll(/pub extern "C" fn ([A-Za-z_]\w*)\(([^{}]*?)\{/g)) {
    if (!jetRuntimeExports.has(match[1]) && !nativeSourceExports.has(match[1]) && !seedOnlyExports.has(match[1]) && pointer.test(match[2])) requireUnownedBox(name, match[1]);
  }
  for (const adapter of adapters) {
    if (adapter.external || jetRuntimeExports.has(adapter.name) || seedOnlyExports.has(adapter.name)) continue;
    const carried = [...adapter.params.map((param) => param.handle).filter(Boolean), ...adapter.result.handles];
    if (carried.some((handle) => handle.full === full)) requireUnownedBox(name, adapter.name);
  }
}
// Inventory uses the SAME descriptor pass as native-pack emission. It never
// writes generated tables or treats this list as actual MIR layout admission.
if (nativeListOnly) {
  const digest = (body) => createHash("sha256").update(body).digest("hex");
  const declarations = [...new Set([...nativeTypes.map((type) => type.source.path),
    ...[...sourceAdapterProofs.values()].flatMap((proof) => proof.declarations.map((declaration) => declaration.path))])].sort();
  const inventory = {
    mode: "native-pack-descriptors",
    provenance: {
      runtime: resolve(runtimePath), runtimeSha256: digest(text),
      generatorSha256: digest(readFileSync(fileURLToPath(import.meta.url))),
      providerManifest: "Compiler/JetRuntime/sources.list",
      providerManifestSha256: digest(readFileSync(`${repo}/Compiler/JetRuntime/sources.list`)),
      sourceDeclarations: declarations.map((path) => ({ path, sha256: digest(readFileSync(`${repo}/${path}`)) })),
      borrowedCarriers: [...borrowedHandles.values()].sort((a, b) => a.runtime.localeCompare(b.runtime)),
    },
    nativeTypes, nativeExports, handleRows,
    wordRecords: [...wordRecords.values()],
    seedHelperNames: [...seedHelperNames].sort(),
    jetRuntimeExports: [...jetRuntimeExports].sort(),
    seedOnlyExports: [...seedOnlyExports].sort(),
    sourceOwnedAdapters: [...sourceAdapterProofs].map(([name, proof]) => ({ name, ...proof })),
    adapterOwnership: adapters.map((adapter) => ({ symbol: adapter.symbol, name: adapter.name, rows: adapter.rows,
      ownership: jetRuntimeExports.has(adapter.name) ? "jet-provider" : sourceAdapterProofs.has(adapter.name) ? "core-source" : adapter.external ? "external" : "seed-only" })),
    unsupportedSumCarriers: [...unsupportedSumCarriers.values()],
    unsupportedRustCarriers: [...unsupportedRustCarriers.values()],
    unsupportedGenericCarriers: [...unsupportedGenericCarriers.values()],
    skipped,
  };
  const packet = classificationErrors.length ? { mode: "native-pack-classification-errors",
    provenance: inventory.provenance, classificationErrors,
    unsupportedGenericCarriers: inventory.unsupportedGenericCarriers,
    unsupportedRustCarriers: inventory.unsupportedRustCarriers,
    unsupportedSumCarriers: inventory.unsupportedSumCarriers } : inventory;
  // Wait for the pipe to drain; a direct process.exit can truncate large JSON.
  await new Promise((done) => process.stdout.write(JSON.stringify(packet) + "\n", done));
  process.exit(classificationErrors.length ? 1 : 0);
}
for (const adapter of adapters) {
  if (adapter.external) continue;
  const params = [];
  const names = adapter.params.map((param, index) => param.ctys.map((cty, k) => {
    const name = param.ctys.length === 1 ? `a${index}` : `a${index}_${k}`;
    params.push(`${name}: ${cty}`);
    return name;
  }));
  for (const out of adapter.result.outs) params.push(`${out.name}: *mut ${out.cty}`);
  const ret = adapter.result.ret ? ` -> ${adapter.result.ret}` : "";
  const call = `crate::${adapter.target}(${adapter.params.map((param, index) => param.expr(names[index])).join(", ")})`;
  rs.push("");
  rs.push(`    /// ${adapter.rows.join(", ")}`.trimEnd());
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn ${adapter.name}(${params.join(", ")})${ret} {`);
  rs.push(`        guard(|| ${adapter.result.body(call)})`);
  rs.push("    }");
}
rs.push("}");
rs.push("// Skipped routes (no C carrier yet):");
for (const line of skipped) rs.push(`//   ${line}`);
rs.push("// END GENERATED C-ABI ROUTES");

const jet = [];
jet.push("// BEGIN GENERATED C-ABI ROUTES (Tools/agent/gen-c-abi-routes.mjs; do not hand-edit)");
jet.push("// Shared carriers of runtime C-ABI routes, whether their definition lives");
jet.push("// in CAbiRoutes.rs or a Jet runtime #Export(c), keyed by the route's");
jet.push("// Prelude symbol; `export` is the adapter's C name (its last segment).");
jet.push("");
jet.push(emitRuntimeAbiAtom());
jet.push("// `HandleMut` lends the handle of the caller's Write place: the runtime");
jet.push("// mutates the value in that box. A Tuple writes its flattened carrier");
jet.push("// slots through out pointers (zeroed eight-byte slots); an Option item");
jet.push("// writes a 0/1 tag followed by its payload slots and returns nothing.");
jet.push("enum LowerRouteCarrier { Word Flag Float Text Unit Handle(name: String) HandleMut(name: String) List(elem: LowerRouteCarrier) Option(some: LowerRouteCarrier) Tag(name: String, variants: String) }");
jet.push("");
jet.push("enum LowerRouteResult {");
jet.push("    Plain(carrier: LowerRouteCarrier)");
jet.push("    Option(some: LowerRouteCarrier)");
jet.push("    Result(ok: LowerRouteCarrier, err: LowerRouteCarrier)");
jet.push("    Tuple(items: [LowerRouteCarrier])");
jet.push("}");
jet.push("");
jet.push("struct LowerRoute { symbol: String, export: String, native_lists: Bool, params: [LowerRouteCarrier], result: LowerRouteResult }");
jet.push("");
// #5001: a table's keyed index, frozen at build time: its distinct keys in
// ascending byte order and, beside each, the key's first row (the row a scan
// of the table returns), searched by lower_key_row.
function jetKeyIndex(name, keys) {
  const first = new Map();
  keys.forEach((key, position) => { if (!first.has(key)) first.set(key, position); });
  const sorted = [...first].sort(([a], [b]) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  return [
    `${name}_KEYS :: prep { [String]{`, ...sorted.map(([key]) => `    "${key}",`), "} }",
    `${name}_KEY_ROWS :: prep { [Int]{`, ...sorted.map(([, position]) => `    ${position},`), "} }",
  ];
}
jet.push("LOWER_RUNTIME_ROUTES :: prep { [LowerRoute]{");
for (const adapter of adapters) {
  const params = adapter.params.map((param) => `LowerRouteCarrier.${param.jet}`).join(", ");
  jet.push(`    LowerRoute{symbol: "${adapter.symbol}", export: "${adapter.name}", native_lists: ${jetRuntimeExports.has(adapter.name)}, params: [LowerRouteCarrier]{${params}}, result: LowerRouteResult.${adapter.result.jet}},`);
}
jet.push("} }");
jet.push("");
jet.push("// #5001: each table below is read in place, and its lookup is keyed: the");
jet.push("// table's distinct keys in ascending byte order with each key's first row,");
jet.push("// frozen at build time, searched by halving instead of a scan of every row.");
jet.push("fn lower_key_row(keys: [String], rows: [Int], key: String) -> Int? Never! {");
jet.push("    low := 0");
jet.push("    high := keys.len()");
jet.push("    loop low < high {");
jet.push("        middle :: low + (high - low) /% 2");
jet.push("        if {");
jet.push("            keys[middle] == key -> return Val(rows[middle])");
jet.push("            keys[middle] < key -> low = middle + 1");
jet.push("            else -> high = middle");
jet.push("        }");
jet.push("    }");
jet.push("    None");
jet.push("}");
jet.push("");
jet.push(...jetKeyIndex("LOWER_RUNTIME_ROUTE", adapters.map((adapter) => adapter.symbol)));
jet.push("");
jet.push("fn lower_runtime_route(symbol: String) -> LowerRoute? {");
jet.push("    position :: lower_key_row(LOWER_RUNTIME_ROUTE_KEYS, LOWER_RUNTIME_ROUTE_KEY_ROWS, symbol) ?? return None");
jet.push("    Val(LOWER_RUNTIME_ROUTES[position].copy())");
jet.push("}");
jet.push("");
jet.push("// The runtime handle type a Jet type generated Rust holds natively crosses the");
jet.push("// boundary as, keyed by the Jet name MIR spells it with: a Core table name");
jet.push("// (`LocalDate`), or a canonical Core source identity");
jet.push("// (`<corelib>/core.net::TCPStream`) whose own Jet shape");
jet.push("// generated Rust replaces. It is dropped by `jet_rt_handle_drop_<runtime>`;");
jet.push("// copied by `jet_rt_handle_clone_<runtime>` when `clone`; rendered by");
jet.push("// `jet_rt_handle_display_<runtime>` / `jet_rt_handle_show_<runtime>` /");
jet.push("// `jet_rt_handle_debug_<runtime>` when `display` / `show` / `debug`; compared");
jet.push("// by `jet_rt_handle_eq_<runtime>` when `eq`.");
jet.push("struct LowerRuntimeHandle { jet_name: String, runtime: String, clone: Bool, display: Bool, show: Bool, debug: Bool, eq: Bool }");
jet.push("");
jet.push("LOWER_RUNTIME_HANDLES :: prep { [LowerRuntimeHandle]{");
const fullOf = new Map(handleRows);
const handleOrder = [...jetNames].sort((a, b) => a[0].localeCompare(b[0]));
for (const [jetName, name] of handleOrder) {
  const render = rendered(name, fullOf.get(name));
  jet.push(`    LowerRuntimeHandle{jet_name: "${jetName}", runtime: "${name}", clone: ${cloneable.has(name)}, display: ${render.display}, show: ${render.show}, debug: ${render.debug}, eq: ${comparable.has(name)}},`);
}
jet.push("} }");
jet.push("");
jet.push("// Opaque native providers: physical ownership ABI only, no Rust schema.");
jet.push("LOWER_NATIVE_PROVIDER_HANDLES :: prep { [LowerRuntimeHandle]{");
for (const row of nativeProviders) jet.push(`    LowerRuntimeHandle{jet_name: "${row.jetName}", runtime: "${row.runtime}", clone: true, display: false, show: false, debug: false, eq: false},`);
jet.push("} }");
jet.push("");
jet.push(...jetKeyIndex("LOWER_RUNTIME_HANDLE", handleOrder.map(([jetName]) => jetName)));
jet.push(...jetKeyIndex("LOWER_NATIVE_PROVIDER_HANDLE", nativeProviders.map((row) => row.jetName)));
jet.push("");
jet.push("fn lower_runtime_handle(name: String) -> LowerRuntimeHandle? {");
jet.push("    if lower_key_row(LOWER_RUNTIME_HANDLE_KEYS, LOWER_RUNTIME_HANDLE_KEY_ROWS, name) == .Val(position) {");
jet.push("        return Val(LOWER_RUNTIME_HANDLES[position].copy())");
jet.push("    }");
jet.push("    // Existing Rust rows win in seed mode; absent nominals may use a provider.");
jet.push("    position :: lower_key_row(LOWER_NATIVE_PROVIDER_HANDLE_KEYS, LOWER_NATIVE_PROVIDER_HANDLE_KEY_ROWS, name) ?? return None");
jet.push("    Val(LOWER_NATIVE_PROVIDER_HANDLES[position].copy())");
jet.push("}");
jet.push("");
jet.push("// Core records the runtime's generated record exports carry (Records.jet),");
jet.push("// keyed by runtime type: a struct's fields in constructor order or an enum's");
jet.push("// variants in index order, each `name=kind` when it carries a member (Text,");
jet.push("// Int, Flag, Float, Tag.<enum>, Rec.<record>, Opt<kind>).");
jet.push("LOWER_GENERATED_RECORDS :: prep { [LowerRecordSchema]{");
for (const schema of recordSchemas) jet.push(`    LowerRecordSchema{jet_name: "${schema.name}", variants: "${schema.variants}", fields: "${schema.fields}"},`);
jet.push("} }");
jet.push("// Declaration-proven scalar records; Lower still proves the actual native field.");
jet.push("struct LowerWordRecord { runtime: String, field: String }");
jet.push("LOWER_WORD_RECORDS :: prep { [LowerWordRecord]{");
for (const [runtime, full] of handleRows) {
  const record = wordRecord(full, "");
  if (record) jet.push(`    LowerWordRecord{runtime: "${runtime}", field: "${record.member}"},`);
}
jet.push("} }");
jet.push("");
jet.push("// Fields of runtime handle structs readable in place (Places.jet):");
jet.push("// Scalar/native handle fields lend storage; Text and header_bytes carriers");
jet.push("// write read-only caller stack headers, never owned/freeable field values.");
jet.push("struct LowerHandleField { runtime: String, field: String, kind: String, header_bytes: Int }");
jet.push("");
jet.push("LOWER_HANDLE_FIELDS :: prep { [LowerHandleField]{");
for (const row of handleFields) jet.push(`    LowerHandleField{runtime: "${row.name}", field: "${row.field}", kind: "${row.kind}", header_bytes: ${row.headerBytes}},`);
jet.push("} }");
jet.push("");
jet.push(...jetKeyIndex("LOWER_HANDLE_FIELD", handleFields.map((row) => `${row.name} ${row.field}`)));
jet.push("");
jet.push("fn lower_handle_field(runtime: String, field: String) -> LowerHandleField? {");
jet.push("    position :: lower_key_row(LOWER_HANDLE_FIELD_KEYS, LOWER_HANDLE_FIELD_KEY_ROWS, \"{runtime} {field}\") ?? return None");
jet.push("    Val(LOWER_HANDLE_FIELDS[position].copy())");
jet.push("}");
jet.push(...emitNativeEnvironmentRoutes(nativeEnvironmentRoutes(routes, jetRuntimeExports, jetRuntimeSignatures)));

jet.push("");
jet.push("// Candidate source identities; Lower validates against actual MIR/native layout.");
jet.push("struct LowerNativeType { jet_name: String, declaration: String, arguments: String }");
jet.push("LOWER_NATIVE_TYPES :: prep { [LowerNativeType]{");
for (const type of nativeTypes) jet.push(`    LowerNativeType{jet_name: "${type.jetName}", declaration: "${type.source.key}", arguments: "${type.arguments}"},`);
jet.push("} }");
jet.push(...jetKeyIndex("LOWER_NATIVE_TYPE", nativeTypes.map((type) => type.jetName)));
jet.push("fn lower_native_type(name: String) -> LowerNativeType? {");
jet.push("    position :: lower_key_row(LOWER_NATIVE_TYPE_KEYS, LOWER_NATIVE_TYPE_KEY_ROWS, name) ?? return None");
jet.push("    Val(LOWER_NATIVE_TYPES[position].copy())");
jet.push("}");
jet.push("struct LowerNativeExport { symbol: String, jet_name: String, op: String, member: String }");
jet.push("LOWER_NATIVE_EXPORTS :: prep { [LowerNativeExport]{");
for (const row of nativeExports) jet.push(`    LowerNativeExport{symbol: "${row.name}", jet_name: "${row.jetName}", op: "${row.op}", member: "${row.member}"},`);
jet.push("} }");
jet.push(...jetKeyIndex("LOWER_NATIVE_EXPORT", nativeExports.map((row) => row.name)));
jet.push("fn lower_native_export(symbol: String) -> LowerNativeExport? {");
jet.push("    position :: lower_key_row(LOWER_NATIVE_EXPORT_KEYS, LOWER_NATIVE_EXPORT_KEY_ROWS, symbol) ?? return None");
jet.push("    Val(LOWER_NATIVE_EXPORTS[position].copy())");
jet.push("}");
jet.push("// All possible external C imports, mechanically derived from the ABI tables.");
jet.push("LOWER_NATIVE_IMPORTS :: prep { [String]{");
const nativeImportNames = new Set([
  ...adapters.map((row) => row.name),
  ...jetRuntimeExports,
  ...[...rs.join("\n").matchAll(/pub extern "C" fn ([A-Za-z_]\w*)\(/g)].map((match) => match[1]),
  ...nativeExports.map((row) => row.name),
]);
for (const name of [...nativeImportNames].sort()) jet.push(`    "${name}",`);
jet.push("} }");
jet.push("// END GENERATED C-ABI ROUTES");

// Lifecycle and record/field helpers obey the same ownership partition as
// routes. Validate every replaced C signature before excluding its seed body
// from the native pack. sources.list is the canonical provider ownership input.
const rustBody = rs.join("\n") + "\n";
const partitionedRust = excludeNativeSeedBodies(rustBody, jetRuntimeExports, seedOnlyExports, (name, raw) => {
  const signature = parseSignature(`fn ${name}${raw}`);
  if (!signature || signature.generics) throw new Error(`Jet-owned generated export ${name}: unparsed C ABI`);
  const params = splitTop(signature.params).map((param) => ({ ctys: [param.slice(param.indexOf(":") + 1).trim()] }));
  validateRuntimeRoute({ symbol: name, name, params, result: { outs: [], ret: signature.result === "()" ? null : signature.result } }, jetRuntimeSignatures.get(name));
});
const outputs = [[rsOut, partitionedRust], [jetOut, jet.join("\n") + "\n"]];
let stale = false;
for (const [path, body] of outputs) {
  let current = "";
  try { current = readFileSync(`${repo}/${path}`, "utf8"); } catch {}
  if (current === body) continue;
  if (check) { console.error(`${path} is stale; rerun Tools/agent/gen-c-abi-routes.mjs`); stale = true; }
  else writeFileSync(`${repo}/${path}`, body);
}
console.log(`${routes.size} registry routes, ${adapters.length} adapters, ${handleRows.length} handle types, ${skipped.length} skipped`);
if (stale) process.exit(1);
