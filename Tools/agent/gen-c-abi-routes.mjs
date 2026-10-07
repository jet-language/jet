#!/usr/bin/env node
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
// `Box<String>` handle (a result is a new owned handle); a Jet type generated
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

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const runtimePath = process.argv[2];
const check = process.argv.includes("--check");
if (!runtimePath) {
  console.error("usage: gen-c-abi-routes.mjs <runtime.rs> [--check]");
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
const addRoute = (symbol, nativeInt, row) => {
  const entry = routes.get(symbol) ?? { nativeInt: false, rows: [] };
  entry.nativeInt ||= nativeInt;
  entry.rows.push(row);
  routes.set(symbol, entry);
};
// Every Core call row: generated Rust calls a direct row's symbol, and a
// non-direct row's symbol too unless `core_call` gives it its own form.
const coreCalls = readFileSync(`${repo}/crates/jet-foundation/src/Syntax/core_calls.rs`, "utf8");
for (const line of coreCalls.split("\n")) {
  const record = /^\s*CoreCallRecord::new\(\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]*)",\s*(?:true|false)/.exec(line);
  if (!record) continue;
  const [, module, member, symbol] = record;
  if (!symbol) continue;
  if (line.includes(".without_direct_aot()")) {
    if (ownForm.has(`${module}.${member}`)) continue;
    if (module === "core.math" && sourceMathMembers.has(member)) continue;
  }
  addRoute(symbol, line.includes(".with_native_int_result()"), `${module}.${member}`);
}
const tirRoutes = readFileSync(`${repo}/crates/jet-codegen/src/Codegen/TIR/routes.rs`, "utf8");
for (const match of tirRoutes.matchAll(/prelude_route_row\(\s*MirPreludeFamily::StaticPrelude,\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]*)"/g)) {
  addRoute(match[3], false, `${match[1]}.${match[2]}`);
}
// Builtin method routes (`b(member, symbol, ...)` in the TBuiltinOp route
// table). Generated Rust widens every builtin Int result from the native
// kernel word (MIRRust.rs `native_int_result`), so these rows do too.
for (const match of tirRoutes.matchAll(/\bb\(\s*"([^"]*)",\s*"([^"]*)",/g)) {
  addRoute(match[2], true, `core.builtin.${match[1]}`);
}
// Handle method routes (`h(member, symbol, ...)` rows and
// `prelude(MirPreludeFamily::HandleMethod, module, member, symbol, ...)`).
// Generated Rust widens every handle method's Int result from the native
// kernel word (MIRRust.rs HandleMethod -> native_int_result).
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
      } else if (word === "use" && stack.every((frame) => frame.mod)) {
        const here = stack.map((frame) => frame.mod);
        // A `use` path (relative to this module unless `crate::`), as a
        // module path from the crate root.
        const resolve = (prefix) => {
          const target = prefix.split("::").filter(Boolean);
          const base = target[0] === "crate" ? [] : [...here];
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
const declarationOf = (rustPath, seen = new Set()) => {
  if (types.has(rustPath)) return { key: rustPath, decls: types.get(rustPath) };
  if (renames.has(rustPath)) return declarationOf(renames.get(rustPath), seen);
  const segments = rustPath.split("::");
  const name = segments.pop();
  const module = segments.join("::");
  if (seen.has(module)) return null;
  seen.add(module);
  for (const target of globs.get(module) ?? []) {
    const found = declarationOf(target ? `${target}::${name}` : name, seen);
    if (found) return found;
  }
  return null;
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
// (handle-method, DataTree, HTTP request and Core-type associated rows written
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

const handTable = readFileSync(`${repo}/crates/jet-codegen/src/Prelude/Core/CAbi.rs`, "utf8");
const handExports = new Set([...handTable.matchAll(/pub extern "C" fn ([A-Za-z0-9_]+)/g)].map((m) => m[1]));
for (const m of handTable.matchAll(/exact_int_(?:binary|located)!\s*\{?\(?([^)}]*)/g)) {
  for (const name of m[1].split(",").map((s) => s.trim()).filter(Boolean)) handExports.add(name);
}
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
  const name = full.split("::").pop();
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
  if (ty.startsWith("crate::") || ty.startsWith("std::")) return ty;
  if (ty.startsWith("super::")) return `crate::${path.split("::").slice(0, -1).filter(Boolean).concat(ty.slice(7)).join("::")}`;
  return path ? `crate::${path}::${ty}` : `crate::${ty}`;
};
const opaqueName = /^(?:[A-Za-z_][A-Za-z0-9_]*::)*([A-Z][A-Za-z0-9_]*)$/;

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
  if (!opaqueName.test(ty)) return null;
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
  if (ty === "i64") return { jet: "Word", read: "w as i64", write: "e as u64" };
  if (fixedInts.has(ty)) return { jet: "Word", slot: ty, read: `w as ${ty}`, write: "e" };
  if (ty === "bool") return { jet: "Flag", read: "w != 0", write: "e as u64" };
  if (ty === "f64") return { jet: "Float", read: "f64::from_bits(w)", write: "e.to_bits()" };
  if (ty === "char") return { jet: "Word", read: "scalar(w as i64)", write: "e as u64" };
  return null;
}

// A List parameter element of a cloneable runtime type: each slot word is a
// borrowed handle, cloned into the Rust Vec (the lowering lends a handle per
// element: a record element converts through Records.jet for the call).
function handleElement(ty, path) {
  if (!opaqueName.test(ty) || tagCarrier(ty, path)) return null;
  const name = opaqueName.exec(ty)[1];
  const full = qualify(ty, path);
  if (name === "String" || name === "Self" || !derivesClone(full)) return null;
  return { jet: `Handle{name: "${name}"}`, read: `unsafe { &*(w as *const ${full}) }.clone()`, handle: { name, full } };
}

// Parameter carrier: { jet, ctys: [C types], expr(names) } or null. A List
// parameter is two words: its slot data pointer and its length.
function paramCarrier(ty, path) {
  ty = ty.replace(/\s+/g, "");
  const one = (jet, cty, expr) => ({ jet, ctys: [cty], expr: ([a]) => expr(a) });
  if (ty === "&str") return one("Text", "JetCString", (a) => `view(${a})`);
  if (ty === "&String") return one("Text", "JetCString", (a) => `unsafe { &*${a} }`);
  if (ty === "Option<&String>") return one("Text", "JetCString", (a) => `(!${a}.is_null()).then(|| unsafe { &*${a} })`);
  if (ty === "String") return one("Text", "JetCString", (a) => `view(${a}).to_owned()`);
  if (ty === "&i64") return one("Word", "i64", (a) => `&${a}`);
  // The prelude Ordering (by value or borrowed): the Jet enum's variant index.
  const ordering = /^(&?)__jet_Ordering$/.exec(ty);
  if (ordering) return one(`Tag{name: "Ordering", variants: "Less,Equal,Greater"}`, "i64", (a) => `${ordering[1]}match ${a} { 0 => crate::__jet_Ordering::__jet_Less, 1 => crate::__jet_Ordering::__jet_Equal, 2 => crate::__jet_Ordering::__jet_Greater, _ => range_stop() }`);
  if (ty === "i64") return one("Word", "i64", (a) => a);
  if (fixedInts.has(ty)) return one("Word", "i64", (a) => `fixed::<${ty}>(${a})`);
  if (ty === "bool") return one("Flag", "u8", (a) => `${a} != 0`);
  if (ty === "f64") return one("Float", "f64", (a) => a);
  if (ty === "char") return one("Word", "i64", (a) => `scalar(${a})`);
  // A fieldless enum by value: its variant index (results keep their carrier).
  const tag = tagCarrier(ty, path);
  if (tag) return one(tag.jet, "i64", tag.read);
  // An exact Int by value: the borrowed carrier word, cloned into an owner.
  // The exact Int is spelled from the crate root wherever its kernel names it.
  if (/^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return one("Word", "i64", (a) => `unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }`);
  // A borrowed exact Int (an exact_prelude_adapter kernel): an owner of the
  // same carrier word, lent for the call.
  if (/^&(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return one("Word", "i64", (a) => `&unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(${a}) }`);
  const list = /^(&?)(?:\[(.+)\]|Vec<(.+)>)$/.exec(ty);
  if (list && (list[1] === "&" || list[3])) {
    const element = elementCarrier(list[2] ?? list[3]) ?? handleElement(list[2] ?? list[3], path);
    if (!element) return null;
    const items = (d, n) => `list_in(${d}, ${n}, |w| ${element.read})`;
    return { jet: `List{elem: LowerRouteCarrier.${element.jet}}`, ctys: [`*const ${element.slot ?? "u64"}`, "i64"], expr: ([d, n]) => (list[1] === "&" ? `&${items(d, n)}` : items(d, n)), handle: element.handle };
  }
  // A mutably borrowed handle: the adapter mutates the runtime value in the
  // caller's own box (the lowering lends the handle of its Write place).
  const borrowedMut = /^&mut(.+)$/.exec(ty);
  if (borrowedMut && opaqueName.test(borrowedMut[1]) && !tagCarrier(borrowedMut[1], path)) {
    const name = opaqueName.exec(borrowedMut[1])[1];
    if (name === "String" || name === "Self") return null;
    const full = qualify(borrowedMut[1], path);
    return { ...one(`HandleMut{name: "${name}"}`, `*mut ${full}`, (a) => `unsafe { &mut *${a} }`), handle: { name, full } };
  }
  const borrowed = /^&([^&].*)$/.exec(ty);
  // A borrowed fieldless enum: its variant index too.
  const borrowedTag = borrowed ? tagCarrier(borrowed[1], path) : null;
  if (borrowedTag) return one(borrowedTag.jet, "i64", (a) => `&${borrowedTag.read(a)}`);
  if (borrowed && !borrowed[1].startsWith("mut") && opaqueName.test(borrowed[1])) {
    const name = opaqueName.exec(borrowed[1])[1];
    if (name === "String" || name === "Self") return null;
    const full = qualify(borrowed[1], path);
    return { ...one(`Handle{name: "${name}"}`, `*mut ${full}`, (a) => `unsafe { &*${a} }`), handle: { name, full } };
  }
  // A cloneable handle by value: the adapter copies the borrowed handle.
  if (!borrowed && opaqueName.test(ty)) {
    const name = opaqueName.exec(ty)[1];
    const full = qualify(ty, path);
    if (name === "String" || name === "Self" || !derivesClone(full)) return null;
    return { ...one(`Handle{name: "${name}"}`, `*mut ${full}`, (a) => `unsafe { &*${a} }.clone()`), handle: { name, full } };
  }
  return null;
}

// A value carrier for a result side: { jet, cty, wrap(v) } or null; "()" is Unit.
function valueCarrier(ty, path, nativeInt) {
  ty = ty.replace(/\s+/g, "");
  if (ty === "()") return { jet: "Unit", cty: null, wrap: () => "()" };
  if (ty === "String") return { jet: "Text", cty: "JetCString", wrap: (v) => `handle(${v})` };
  if (ty === "i64") return { jet: "Word", cty: "i64", wrap: (v) => (nativeInt ? `crate::jet_std::jet_int_from_i64(${v})` : v) };
  if (fixedInts.has(ty)) return { jet: "Word", cty: "i64", wrap: (v) => nativeInt ? `crate::jet_std::jet_int_from_i64(i64::try_from(${v}).unwrap_or_else(|_| range_stop()))` : `(${v}) as i64` };
  if (ty === "bool") return { jet: "Flag", cty: "bool", wrap: (v) => v };
  if (ty === "f64") return { jet: "Float", cty: "f64", wrap: (v) => v };
  if (ty === "char") return { jet: "Word", cty: "i64", wrap: (v) => `(${v}) as i64` };
  // The prelude Ordering: the Jet enum's variant index (Less, Equal, Greater).
  if (ty === "__jet_Ordering") return { jet: `Tag{name: "Ordering", variants: "Less,Equal,Greater"}`, cty: "i64", wrap: (v) => `match ${v} { crate::__jet_Ordering::__jet_Less => 0, crate::__jet_Ordering::__jet_Equal => 1, crate::__jet_Ordering::__jet_Greater => 2 }` };
  // An owned exact Int: its carrier word, ownership passing to the caller.
  if (/^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return { jet: "Word", cty: "i64", wrap: (v) => `(${v}).into_raw()` };
  if (opaqueName.test(ty)) {
    const name = opaqueName.exec(ty)[1];
    if (name === "Self" || name === "String") return null;
    const full = qualify(ty, path);
    return { jet: `Handle{name: "${name}"}`, cty: `*mut ${full}`, wrap: (v) => `Box::into_raw(Box::new(${v}))`, handle: { name, full } };
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
function resultShape(ty, path, nativeInt) {
  ty = (ty ?? "()").replace(/\s+/g, "");
  if (ty === "!") ty = "()";
  // `JetOutcome<T, JetAbsent>` (= `Result<T, JetAbsent>`) is generated Rust's
  // `T?`: it crosses as an Option.
  const outcome = /^(?:(?:[a-z_][a-z0-9_]*::)*JetOutcome|Result)<(.*),JetAbsent>$/.exec(ty);
  const generic = outcome ? [ty, "Option", outcome[1]] : /^(Option|Result)<(.*)>$/.exec(ty);
  const [somePattern, nonePattern] = outcome ? ["Ok", "Err(_)"] : ["Some", "None"];
  // A List result element: a plain slot word, or a new owned handle of a
  // runtime value (the lowering's List keeps the handles as its elements).
  const resultElement = (t) => {
    const plain = elementCarrier(t);
    if (plain) return plain;
    if (!opaqueName.test(t) || tagCarrier(t, path)) return null;
    const name = opaqueName.exec(t)[1];
    if (name === "String" || name === "Self") return null;
    const full = qualify(t, path);
    return { jet: `Handle{name: "${name}"}`, write: "Box::into_raw(Box::new(e)) as u64", handle: { name, full } };
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
      jet: `Plain{carrier: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "data", cty: `*mut ${element.slot ?? "u64"}` }], handles: element.handle ? [element.handle] : [],
      body: (call) => `list_out(${call}.into_iter().map(|e| ${element.write}).collect(), data)`,
    };
  }
  const tuple = /^\((.*)\)$/.exec(ty);
  if (tuple && ty !== "()") {
    const items = splitTop(tuple[1]).map((item) => valueCarrier(item, path, nativeInt));
    if (items.length < 2 || items.some((item) => !item || !item.cty)) return null;
    const names = items.map((_, i) => `e${i}`);
    const writes = items.map((item, i) => `            unsafe { item${i}.write(${item.wrap(names[i])}) };`);
    return {
      jet: `Tuple{items: [LowerRouteCarrier]{${items.map((item) => `LowerRouteCarrier.${item.jet}`).join(", ")}}}`, ret: null,
      outs: items.map((item, i) => ({ name: `item${i}`, cty: item.cty })), handles: items.map((item) => item.handle).filter(Boolean),
      body: (call) => `{\n            let (${names.join(", ")}) = ${call};\n${writes.join("\n")}\n        }`,
    };
  }
  if (!generic) {
    const v = valueCarrier(ty, path, nativeInt);
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
        jet: `Option{some: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "some", cty: `*mut ${element.slot ?? "u64"}` }, { name: "some_len", cty: "i64" }], handles: element.handle ? [element.handle] : [],
        body: (call) => `match ${call} {\n            ${listArm("some", element, somePattern, 1)}\n            ${nonePattern} => 0,\n        }`,
      };
    }
    const v = valueCarrier(args[0], path, nativeInt);
    if (!v || !v.cty) return null;
    return {
      jet: `Option{some: LowerRouteCarrier.${v.jet}}`, ret: "i64", outs: [{ name: "some", cty: v.cty }], handles: v.handle ? [v.handle] : [],
      body: (call) => `match ${call} {\n            ${somePattern}(value) => { unsafe { some.write(${v.wrap("value")}) }; 1 }\n            ${nonePattern} => 0,\n        }`,
    };
  }
  if (args.length !== 2) return null;
  const okList = listOf(args[0]);
  const ok = okList ? null : valueCarrier(args[0], path, nativeInt);
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
    body: (call) => `match ${call} {\n            ${okArm}\n            ${errArm}\n        }`,
  };
}

// ---------------------------------------------------------------------------
// 4. Adapters.

const adapters = [];
const skipped = [];
const exported = new Set(handExports);
const handles = new Map();
for (const [symbol, route] of [...routes].sort((a, b) => a[0].localeCompare(b[0]))) {
  // An exact-Int adapted symbol calls its owned kernel (MIRRust.rs
  // call_symbol_for_function); the route keeps the symbol.
  const target = exactAdapters.get(symbol) ?? symbol;
  const segments = symbol.split("::");
  // A method's export names its type too (`JetDateTime::date` -> `JetDateTime_date`).
  const name = segments.length > 1 && /^[A-Z]/.test(segments[segments.length - 2]) ? segments.slice(-2).join("_") : segments[segments.length - 1];
  const label = route.rows[0];
  if (handExports.has(name)) continue;
  if (exported.has(name)) { skipped.push(`${symbol} (${label}): export name taken`); continue; }
  const all = (fns.get(target) ?? []).filter((def) => !def.wasmOnly);
  if (all.length === 0) { skipped.push(`${symbol} (${label}): no runtime function`); continue; }
  const defs = all.filter((def) => !def.gated);
  if (defs.length === 0) { skipped.push(`${symbol} (${label}): platform-gated`); continue; }
  if (new Set(defs.map((def) => def.sig)).size !== 1) { skipped.push(`${symbol} (${label}): cfg-dependent signature`); continue; }
  const def = defs[0];
  const path = def.path;
  if (path && !def.pub) { skipped.push(`${symbol} (${label}): not public in ${path}`); continue; }
  // A method's receiver is its first parameter; `Self` is the impl's type.
  const selfSig = def.selfType
    ? def.sig.replace(/\((\s*)(&\s*mut\s+|&\s*|mut\s+)?self\b/, (_, space, ref) => `(${space}self: ${(ref ?? "").replace(/^mut\s+$/, "")}Self`).replace(/\bSelf\b/g, def.selfType)
    : def.sig;
  const sig = parseSignature(selfSig);
  if (!sig) { skipped.push(`${symbol} (${label}): unparsed signature ${def.sig}`); continue; }
  if (sig.generics) { skipped.push(`${symbol} (${label}): generic`); continue; }
  const params = splitTop(sig.params).map((param) => {
    const colon = param.indexOf(":");
    return colon < 0 ? null : paramCarrier(param.slice(colon + 1), path);
  });
  if (params.some((param) => param === null)) { skipped.push(`${symbol} (${label}): parameter types ${sig.params}`); continue; }
  const result = resultShape(sig.result, path, route.nativeInt);
  if (!result) { skipped.push(`${symbol} (${label}): result type ${sig.result}`); continue; }
  exported.add(name);
  for (const handle of [...params.map((param) => param.handle).filter(Boolean), ...result.handles]) {
    const known = handles.get(handle.name);
    if (known && known !== handle.full) { skipped.push(`${symbol} (${label}): handle ${handle.name} names two types`); exported.delete(name); }
    else handles.set(handle.name, handle.full);
  }
  if (!exported.has(name)) continue;
  adapters.push({ symbol, target, name, label, params, result, rows: route.rows });
}

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
  const tree = { name: "DataTree", full: "crate::jet_std::DataTree" };
  const error = { name: "FieldError", full: "crate::jet_std::FieldError" };
  handles.set(tree.name, tree.full);
  handles.set(error.name, error.full);
  const encode = `jet_rt_codec_encode_${name}`;
  const decode = `jet_rt_codec_decode_${name}`;
  adapters.push({
    symbol: encode, target: "jet_codec_encode", name: encode, label: "canonical scalar Encode", rows: [],
    params: [{ jet, ctys: [cty], expr: ([a]) => name === "String" ? `0, ${paramCarrier("&String", "").expr([a])}` : `0, &(${read(a)})` }],
    result: { jet: 'Plain{carrier: LowerRouteCarrier.Handle{name: "DataTree"}}', ret: "*mut crate::jet_std::DataTree", outs: [], handles: [tree],
      body: (call) => `Box::into_raw(Box::new(${call}))` },
  });
  adapters.push({
    symbol: decode, target: `jet_codec_decode::<${rust}>`, name: decode, label: "canonical scalar Decode", rows: [],
    params: [{ jet: 'Handle{name: "DataTree"}', ctys: ["*mut crate::jet_std::DataTree"], expr: ([a]) => `0, unsafe { &*${a} }`, handle: tree }],
    result: { jet: `Result{ok: LowerRouteCarrier.${jet}, err: LowerRouteCarrier.List{elem: LowerRouteCarrier.Handle{name: "FieldError"}}}`, ret: "i64",
      outs: [{ name: "ok", cty }, { name: "err", cty: "*mut u64" }, { name: "err_len", cty: "i64" }], handles: [error],
      body: (call) => `match ${call} {\n            Ok(value) => { unsafe { ok.write(${write("value")}) }; 1 }\n            Err(errors) => { let n = list_out(errors.into_iter().map(|e| Box::into_raw(Box::new(e)) as u64).collect(), err); unsafe { err_len.write(n) }; 0 }\n        }` },
  });
}

// Parse-to-tree specialization retains the typed intrinsic's exact parser and
// FieldError envelope, then the native backend invokes the checked Decode impl.
for (const format of ["json", "toml", "yaml"]) {
  const symbol = `jet_rt_codec_parse_${format}`;
  const result = resultShape("Result<jet_std::DataTree,Vec<jet_std::FieldError>>", "", false);
  if (!result) throw new Error(`gen-c-abi-routes: missing ${format} typed tree carriers`);
  adapters.push({ symbol, target: `jet_enc_${format}_decode::<crate::jet_std::DataTree>`, name: symbol,
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
// Canonical Core types whose own Jet shape generated Rust keeps
// (MIRRust.rs core_native_probe_name and its is_source_owned_core_* tests).
const sourceOwned = [];
for (const fn of ["core_native_probe_name", "is_source_owned_core_compute_shape", "is_source_owned_core_net_shape", "is_source_owned_core_web_query_shape"]) {
  for (const m of fnBody(mirRustRs, fn).matchAll(/starts_with\("([^"]+)"\)(?:\s*&&\s*(?:name\.ends_with\("([^"]+)"\)|matches!\(\s*[^,]+,\s*((?:"[^"]+"\s*\|?\s*)+)\)))?/g)) {
    if (m[1] === "<corelib>/") continue;
    sourceOwned.push({ prefix: m[1], suffix: m[2], leaves: m[3] ? [...m[3].matchAll(/"([^"]+)"/g)].map((k) => k[1]) : null });
  }
}
const coreLeaf = (name) => name.slice("<corelib>/".length).split("::").pop();
const isSourceOwned = (name) => sourceOwned.some((rule) => name.startsWith(rule.prefix) && (!rule.suffix || name.endsWith(rule.suffix)) && (!rule.leaves || rule.leaves.includes(coreLeaf(name))));
// The native Rust type path (from the crate root) of a Jet type name, as
// MIRRust.rs `rust_type` resolves an argument-free nominal; null if none.
const nativeType = (name) => {
  const canonical = name.startsWith("<corelib>/");
  const native = canonical && !isSourceOwned(name) ? coreLeaf(name) : name;
  if (explicitTable.has(name)) return explicitTable.get(name);
  const fileName = /^<corelib>\/Core\/(files|term)::Core\/\1\/\1\.jet::/.test(name) ? coreLeaf(name) : name;
  if (fileTable.has(fileName)) return fileTable.get(fileName);
  for (const table of leafTables) if (table.has(native)) return table.get(native);
  const emailName = name.replace(/^<corelib>\/Core\/email::Core\/email\/email\.jet::/, "");
  if (emailTable.has(emailName)) return `jet_email::${emailTable.get(emailName)}`;
  if (rootPrelude.has(native)) return rootPrelude.get(native);
  for (const table of lateLeafTables) if (table.has(native)) return table.get(native);
  if (canonical && name.startsWith("<corelib>/Core/sync::") && name.endsWith("::RowPolicy")) return null;
  const core = coreTable.get(canonical ? coreLeaf(name) : name);
  if (!core) return null;
  if (core === "JetRowPolicy" || core === "JetTaskFailure") return core;
  return rootPrelude.get(core) ?? `jet_std::${core}`;
};
// A native type path the adapters can hold by handle: a handle a route
// already holds at that path, or one visible, ungated, non-generic runtime
// struct or enum, spelled at its declaration (a private glob import does not
// re-export it).
const handleOf = (rustPath) => {
  if (!rustPath || !/^(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Z][A-Za-z0-9_]*$/.test(rustPath)) return null;
  const name = rustPath.split("::").pop();
  if (handles.get(name) === `crate::${rustPath}`) return { name, full: `crate::${rustPath}` };
  const found = declarationOf(rustPath);
  if (!found || found.decls.length !== 1) return null;
  const [decl] = found.decls;
  if (!decl.visible || decl.generic || decl.gated) return null;
  // A renamed import names the declaration's own type.
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
    for (const m of readFileSync(`${coreDir}/${file}`, "utf8").matchAll(/^\s*(?:pub )?(?:struct|enum) ([A-Z][A-Za-z0-9_]*)\b/gm)) {
      candidates.add(`<corelib>/${dirname(rel)}::${rel}::${m[1]}`);
    }
  }
  // Two spellings of one declaration (`crate::X` and, through a glob import,
  // `crate::jet_std::X`) are one handle type: the first spelling stays.
  const sameType = (a, b) => {
    const left = declarationOf(a.slice("crate::".length));
    return a === b || (left !== null && left.key === declarationOf(b.slice("crate::".length))?.key);
  };
  for (const jetName of [...candidates].sort()) {
    const handle = handleOf(nativeType(jetName));
    if (!handle) continue;
    const known = handles.get(handle.name);
    if (known && !sameType(known, handle.full)) { skipped.push(`handle type ${jetName} -> ${handle.full}: ${handle.name} names two types`); continue; }
    if (!known) handles.set(handle.name, handle.full);
    jetNames.set(jetName, handle.name);
  }
}

// Handle drops: Core types that cross the boundary by handle.
const handleRows = [...handles].sort((a, b) => a[0].localeCompare(b[0]));

const rs = [];
rs.push("// BEGIN GENERATED C-ABI ROUTES (Tools/agent/gen-c-abi-routes.mjs; do not hand-edit)");
rs.push("// One adapter per Core call registry route whose kernel signature has a C");
rs.push("// carrier (CAbi.rs carrier rules). Skipped routes are listed at the end.");
rs.push("#[cfg(not(target_arch = \"wasm32\"))]");
rs.push("#[allow(non_snake_case, clippy::all)]");
rs.push("mod jet_c_abi_routes {");
rs.push("    use super::jet_c_abi::{guard, handle, native, view, JetCString};");
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
rs.push("            let layout = std::alloc::Layout::from_size_align(len * std::mem::size_of::<S>(), 8).unwrap_or_else(|_| range_stop());");
rs.push("            // SAFETY: `layout` has a nonzero size.");
rs.push("            let buffer = unsafe { std::alloc::alloc(layout) } as *mut S;");
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
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_handle_drop_${name}(value: *mut ${full}) {`);
  rs.push("        if !value.is_null() {");
  rs.push("            // SAFETY: the caller transfers its owned handle from `Box::into_raw`.");
  rs.push("            drop(unsafe { Box::from_raw(value) });");
  rs.push("        }");
  rs.push("    }");
  if (!cloneable.has(name)) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_handle_clone_${name}(value: *mut ${full}) -> *mut ${full} {`);
  rs.push("        // SAFETY: `value` is a live handle the caller owns for this call.");
  rs.push("        guard(|| Box::into_raw(Box::new(unsafe { &*value }.clone())))");
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
    if (!rendered(name, full)[kind]) continue;
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_handle_${kind}_${name}(value: *mut ${full}) -> JetCString {`);
    rs.push("        // SAFETY: `value` is a live handle the caller lends for this call.");
    rs.push(`        guard(|| handle(<${full} as crate::${trait}>::${method}(unsafe { &*value })))`);
    rs.push("    }");
  }
}
const comparable = new Set(handleRows.filter(([, full]) => derives(full, "PartialEq")).map(([name]) => name));
for (const [name, full] of handleRows) {
  if (!comparable.has(name)) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_handle_eq_${name}(left: *mut ${full}, right: *mut ${full}) -> bool {`);
  rs.push("        // SAFETY: both are live handles the caller lends for this call.");
  rs.push("        guard(|| unsafe { &*left } == unsafe { &*right })");
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
const recordDecl = (full) => {
  const found = declarationOf(full.replace(/^crate::/, ""));
  if (!found || found.decls.length !== 1) return null;
  const decl = found.decls[0];
  if (decl.generic || decl.gated || decl.at === undefined) return null;
  const open = text.indexOf("{", decl.at);
  const head = text.slice(decl.at, open);
  if (open < 0 || /[;(]/.test(head) || /\bwhere\b/.test(head)) return null;
  let depth = 0;
  let close = open;
  for (; close < text.length; close++) {
    if (text[close] === "{") depth++;
    else if (text[close] === "}" && --depth === 0) break;
  }
  const body = text.slice(open + 1, close).replace(/\/\/[^\n]*/g, "");
  if (/#\[cfg/.test(body)) return null;
  return { kind: decl.kind, path: decl.path, items: splitTop(body.replace(/#\[[^\]]*\]/g, "")) };
};
const recordable = new Map(); // runtime name -> { full, kind, path, members }
for (const [name, full] of handleRows) {
  if (handExports.has(`jet_rt_record_tag_${name}`) || handExports.has(`jet_rt_record_new_${name}`)) continue;
  const decl = recordDecl(full);
  if (!decl) continue;
  const members = [];
  let ok = true;
  for (const item of decl.items) {
    if (decl.kind === "struct") {
      const field = /^(pub(?:\([a-z]+\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*([\s\S]+)$/.exec(item);
      // A struct in a module exposes only its `pub` fields to the adapters.
      if (!field || (decl.path && !field[1])) { ok = false; break; }
      members.push({ name: field[2], ty: field[3].replace(/\s+/g, "") });
    } else {
      const variant = /^([A-Z][A-Za-z0-9_]*)\s*(?:\(([\s\S]*)\)|=\s*[-0-9]+)?$/.exec(item);
      if (!variant || (variant[2] !== undefined && splitTop(variant[2]).length !== 1)) { ok = false; break; }
      members.push({ name: variant[1], ty: variant[2] === undefined ? null : variant[2].replace(/\s+/g, "") });
    }
  }
  if (ok) recordable.set(name, { full, kind: decl.kind, path: decl.path, members });
}
// A member's carrier: { kind, ret, outCty, get(place), cty, make(param) }, or
// null when no export carries it. `place` is a Rust place of the member.
// Recursive DataTree collections cross as temporary owned Vec holders. These
// exports only marshal storage; ordering/duplicate policy remains in Jet.
const recordSequences = new Map();
const recordMember = (ty, path) => {
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
    const element = elementTy === "u8" ? null : recordMember(elementTy, path);
    if (elementTy !== "u8" && !element?.kind.startsWith("Rec.")) return null;
    const kind = `${entries ? "Entries" : "List"}.${element ? element.kind : "Byte"}`;
    const elementFull = element ? element.cty.replace(/^\*mut /, "") : "u8";
    const full = `Vec<${entries ? `(String, ${elementFull})` : elementFull}>`;
    const prefix = `jet_rt_record_seq_${kind.replaceAll(".", "_")}`;
    recordSequences.set(kind, { kind, prefix, full, entries: !!entries, element, elementFull });
    return { kind, ret: `*mut ${full}`, get: (v) => `&${v} as *const ${full} as *mut ${full}`,
      cty: `*mut ${full}`, make: (p) => `*unsafe { Box::from_raw(${p}) }` };
  }
  const tag = tagCarrier(ty, path);
  if (tag) return { kind: `Tag.${opaqueName.exec(ty)[1]}`, ret: "i64", get: (v) => tag.write(`&${v}`), cty: "i64", make: tag.read };
  const option = /^(Option|JetOutcome)<(.+?)(?:,JetAbsent|,(?:crate::|super::)*JetAbsent)?>$/.exec(ty);
  if (option) {
    if (option[1] === "JetOutcome" && !/,(?:(?:crate|super)::)*JetAbsent>$/.test(ty)) return null;
    const inner = recordMember(option[2], path);
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
    const name = opaqueName.exec(ty)[1];
    const nested = handles.get(name);
    if (!nested || nested !== qualify(ty, path) && declarationOf(nested.replace(/^crate::/, ""))?.key !== declarationOf(qualify(ty, path).replace(/^crate::/, ""))?.key) return null;
    if (!cloneable.has(name)) return null;
    return { kind: `Rec.${name}`, ret: `*mut ${nested}`, get: (v) => `Box::into_raw(Box::new(${v}.clone()))`, cty: `*mut ${nested}`, make: (p) => `*unsafe { Box::from_raw(${p}) }` };
  }
  return null;
};
// A record whose members all cross; a nested record must itself cross.
const recordMembers = new Map(); // runtime name -> [{ name, carrier | null }]
for (const [name, record] of recordable) {
  const carried = record.members.map((member) => ({ name: member.name, carrier: member.ty === null ? null : recordMember(member.ty, record.path) }));
  if (carried.every((member, k) => record.members[k].ty === null || member.carrier)) recordMembers.set(name, carried);
}
for (let changed = true; changed;) {
  changed = false;
  for (const [name, members] of recordMembers) {
    const nestedKinds = members.map((member) => member.carrier?.kind.replace(/^(?:Opt|List\.|Entries\.)/, "") ?? "").filter((kind) => kind.startsWith("Rec."));
    if (nestedKinds.every((kind) => recordMembers.has(kind.slice(4)) || handExports.has(`jet_rt_record_new_${kind.slice(4)}`))) continue;
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
  const getter = (export_, param, carrier, place) => {
    const out = carrier.outCty ? `, some: *mut ${carrier.outCty}` : "";
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn ${export_}(value: *mut ${full}${out}) -> ${carrier.ret} {`);
    rs.push(`        // SAFETY: \`value\` is a live handle the caller lends${carrier.outCty ? "; `some` is its zeroed out slot" : " for this call"}.`);
    rs.push(`        guard(|| ${param} ${place})`);
    rs.push("    }");
  };
  if (kind === "struct") {
    // A fieldless struct carries nothing a schema row can check.
    if (members.length === 0) { recordSchemas.pop(); continue; }
    for (const member of members) getter(`jet_rt_record_get_${name}_${member.name}`, `{ let value = unsafe { &*value };`, member.carrier, `${member.carrier.get(`value.${member.name}`)} }`);
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_record_new_${name}(${members.map((member, k) => `m${k}: ${member.carrier.cty}`).join(", ")}) -> *mut ${full} {`);
    rs.push(`        guard(|| Box::into_raw(Box::new(${full} { ${members.map((member, k) => `${member.name}: ${member.carrier.make(`m${k}`)}`).join(", ")} })))`);
    rs.push("    }");
    continue;
  }
  // A fieldless enum crosses as its index: its schema row alone.
  if (members.every((member) => !member.carrier)) continue;
  rs.push("");
  rs.push("    #[no_mangle]");
  rs.push(`    pub extern "C" fn jet_rt_record_tag_${name}(value: *mut ${full}) -> i64 {`);
  rs.push("        // SAFETY: `value` is a live handle the caller lends for this call.");
  rs.push(`        guard(|| match unsafe { &*value } { ${members.map((member, k) => `${full}::${member.name}${member.carrier ? "(_)" : ""} => ${k}`).join(", ")} })`);
  rs.push("    }");
  for (const member of members) {
    if (member.carrier) {
      getter(`jet_rt_record_get_${name}_${member.name}`, `match unsafe { &*value } { ${full}::${member.name}(item) =>`, member.carrier, `${member.carrier.get("(*item)")}, _ => crate::jet_panic("<core.prelude>", 0, "record variant index out of range") }`);
    }
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_record_new_${name}_${member.name}(${member.carrier ? `payload: ${member.carrier.cty}` : ""}) -> *mut ${full} {`);
    rs.push(`        guard(|| Box::into_raw(Box::new(${full}::${member.name}${member.carrier ? `(${member.carrier.make("payload")})` : ""})))`);
    rs.push("    }");
  }
}
// Field places of a handle struct (one the lowering keeps as a runtime handle,
// e.g. TempDir's `path`): jet_rt_handle_field_<R>_<field>(h) is the address of
// the field inside the runtime value, lent while `h` lives. A String or a
// nested handle is that address as a borrowed handle; a Float or a Bool is
// its storage. The lowering reads (never writes or moves) through it.
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
    if (ty === "String") { kind = "Text"; pointee = "String"; }
    else if (ty === "f64") { kind = "Float"; pointee = "f64"; }
    else if (ty === "bool") { kind = "Flag"; pointee = "bool"; }
    else if (opaqueName.test(ty) && !tagCarrier(ty, decl.path)) {
      const nested = opaqueName.exec(ty)[1];
      const held = handles.get(nested);
      if (held && declarationOf(held.replace(/^crate::/, ""))?.key === declarationOf(qualify(ty, decl.path).replace(/^crate::/, ""))?.key) { kind = `Handle.${nested}`; pointee = held; }
    }
    if (!kind) continue;
    handleFields.push({ name, field: field[2], kind });
    rs.push("");
    rs.push("    #[no_mangle]");
    rs.push(`    pub extern "C" fn jet_rt_handle_field_${name}_${field[2]}(value: *mut ${full}) -> *const ${pointee} {`);
    rs.push("        // SAFETY: `value` is a live handle the caller lends; the field outlives this read.");
    rs.push(`        unsafe { std::ptr::addr_of!((*value).${field[2]}) }`);
    rs.push("    }");
  }
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
  rs.push(`    /// ${adapter.rows.join(", ")}`);
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
jet.push("// The carriers of every runtime C-ABI route adapter");
jet.push("// (crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs), keyed by the route's");
jet.push("// Prelude symbol; `export` is the adapter's C name (its last segment).");
jet.push("");
jet.push("// `HandleMut` lends the handle of the caller's Write place: the runtime");
jet.push("// mutates the value in that box. A `Tuple` result writes item `i` through its");
jet.push("// `i`-th out pointer (one zeroed eight-byte slot each) and returns nothing.");
jet.push("pub enum LowerRouteCarrier { Word Flag Float Text Unit Handle(name: String) HandleMut(name: String) List(elem: LowerRouteCarrier) Tag(name: String, variants: String) }");
jet.push("");
jet.push("pub enum LowerRouteResult {");
jet.push("    Plain(carrier: LowerRouteCarrier)");
jet.push("    Option(some: LowerRouteCarrier)");
jet.push("    Result(ok: LowerRouteCarrier, err: LowerRouteCarrier)");
jet.push("    Tuple(items: [LowerRouteCarrier])");
jet.push("}");
jet.push("");
jet.push("pub struct LowerRoute { pub symbol: String, pub export: String, pub params: [LowerRouteCarrier], pub result: LowerRouteResult }");
jet.push("");
jet.push("LOWER_RUNTIME_ROUTES :: prep { [LowerRoute]{");
for (const adapter of adapters) {
  const params = adapter.params.map((param) => `LowerRouteCarrier.${param.jet}`).join(", ");
  jet.push(`    LowerRoute{symbol: "${adapter.symbol}", export: "${adapter.name}", params: [LowerRouteCarrier]{${params}}, result: LowerRouteResult.${adapter.result.jet}},`);
}
jet.push("} }");
jet.push("");
jet.push("pub fn lower_runtime_route(symbol: String) -> LowerRoute? {");
jet.push("    loop route in LOWER_RUNTIME_ROUTES {");
jet.push("        if route.symbol == symbol -> return Val(route)");
jet.push("    }");
jet.push("    None");
jet.push("}");
jet.push("");
jet.push("// The runtime handle type a Jet type generated Rust holds natively crosses the");
jet.push("// boundary as, keyed by the Jet name MIR spells it with: a Core table name");
jet.push("// (`LocalDate`), or a canonical Core source identity");
jet.push("// (`<corelib>/Core/net::Core/net/net.jet::TCPStream`) whose own Jet shape");
jet.push("// generated Rust replaces. It is dropped by `jet_rt_handle_drop_<runtime>`;");
jet.push("// copied by `jet_rt_handle_clone_<runtime>` when `clone`; rendered by");
jet.push("// `jet_rt_handle_display_<runtime>` / `jet_rt_handle_show_<runtime>` /");
jet.push("// `jet_rt_handle_debug_<runtime>` when `display` / `show` / `debug`; compared");
jet.push("// by `jet_rt_handle_eq_<runtime>` when `eq`.");
jet.push("pub struct LowerRuntimeHandle { pub jet_name: String, pub runtime: String, pub clone: Bool, pub display: Bool, pub show: Bool, pub debug: Bool, pub eq: Bool }");
jet.push("");
jet.push("LOWER_RUNTIME_HANDLES :: prep { [LowerRuntimeHandle]{");
const fullOf = new Map(handleRows);
for (const [jetName, name] of [...jetNames].sort((a, b) => a[0].localeCompare(b[0]))) {
  const render = rendered(name, fullOf.get(name));
  jet.push(`    LowerRuntimeHandle{jet_name: "${jetName}", runtime: "${name}", clone: ${cloneable.has(name)}, display: ${render.display}, show: ${render.show}, debug: ${render.debug}, eq: ${comparable.has(name)}},`);
}
jet.push("} }");
jet.push("");
jet.push("pub fn lower_runtime_handle(name: String) -> LowerRuntimeHandle? {");
jet.push("    loop handle in LOWER_RUNTIME_HANDLES {");
jet.push("        if handle.jet_name == name -> return Val(~handle)");
jet.push("    }");
jet.push("    None");
jet.push("}");
jet.push("");
jet.push("// Core records the runtime's generated record exports carry (Records.jet),");
jet.push("// keyed by runtime type: a struct's fields in constructor order or an enum's");
jet.push("// variants in index order, each `name=kind` when it carries a member (Text,");
jet.push("// Int, Flag, Float, Tag.<enum>, Rec.<record>, Opt<kind>).");
jet.push("LOWER_GENERATED_RECORDS :: prep { [LowerRecordSchema]{");
for (const schema of recordSchemas) jet.push(`    LowerRecordSchema{jet_name: "${schema.name}", variants: "${schema.variants}", fields: "${schema.fields}"},`);
jet.push("} }");
jet.push("");
jet.push("// Fields of runtime handle structs readable in place (Places.jet):");
jet.push("// `jet_rt_handle_field_<runtime>_<field>(h)` lends the field's address; kind is");
jet.push("// Text, Float, Flag or Handle.<runtime>.");
jet.push("pub struct LowerHandleField { pub runtime: String, pub field: String, pub kind: String }");
jet.push("");
jet.push("LOWER_HANDLE_FIELDS :: prep { [LowerHandleField]{");
for (const row of handleFields) jet.push(`    LowerHandleField{runtime: "${row.name}", field: "${row.field}", kind: "${row.kind}"},`);
jet.push("} }");
jet.push("");
jet.push("pub fn lower_handle_field(runtime: String, field: String) -> LowerHandleField? {");
jet.push("    loop row in LOWER_HANDLE_FIELDS {");
jet.push("        if row.runtime == runtime && row.field == field -> return Val(~row)");
jet.push("    }");
jet.push("    None");
jet.push("}");
jet.push("// END GENERATED C-ABI ROUTES");

const outputs = [[rsOut, rs.join("\n") + "\n"], [jetOut, jet.join("\n") + "\n"]];
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
