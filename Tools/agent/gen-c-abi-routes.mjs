#!/usr/bin/env node
// Generate the runtime's C-ABI route layer (D-EXEC1, I8) from the Core call
// registry: one `#[no_mangle] extern "C"` adapter per Prelude route whose Rust
// kernel signature has a C carrier, plus the backend's matching route table.
//
// usage: node Tools/agent/gen-c-abi-routes.mjs <runtime.rs> [--check]
//   <runtime.rs> is the compiled runtime's Rust text as emitted for a one-line
//   program (`jet emit --rust seed.jet`, seed `fn run() { print("hello") }`):
//   the exact items the `jet_runtime` crate holds, so every kernel's module
//   path and signature come from the runtime itself, not from a second list.
//
// Routes (the registry):
//   - crates/jet-foundation/src/Syntax/core_calls.rs: every CoreCallRecord row
//     AOT calls directly (`aot_direct`), keyed by its symbol (a Prelude or a
//     Rust-crate symbol: both are one runtime function generated Rust calls);
//   - crates/jet-codegen/src/Codegen/TIR/routes.rs: every StaticPrelude route
//     row (`prelude_route_row(MirPreludeFamily::StaticPrelude, ...)`).
//
// Outputs:
//   - crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs: the adapters, module
//     `jet_c_abi_routes`, appended after CAbi.rs in the runtime text;
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

const routes = new Map();
const addRoute = (symbol, nativeInt, row) => {
  const entry = routes.get(symbol) ?? { nativeInt: false, rows: [] };
  entry.nativeInt ||= nativeInt;
  entry.rows.push(row);
  routes.set(symbol, entry);
};
const coreCalls = readFileSync(`${repo}/crates/jet-foundation/src/Syntax/core_calls.rs`, "utf8");
for (const line of coreCalls.split("\n")) {
  const record = /^\s*CoreCallRecord::new\(\s*"([^"]*)",\s*"([^"]*)",\s*"([^"]*)",\s*(?:true|false)/.exec(line);
  if (!record || line.includes(".without_direct_aot()")) continue;
  const [, module, member, symbol] = record;
  if (!symbol) continue;
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

const text = readFileSync(runtimePath, "utf8");
const fns = new Map(); // "path::name" -> [{ pub, sig, wasmOnly, gated, path }]
const types = new Map(); // "path::Name" -> [{ visible, generic, gated, derives }] (module-level struct / enum)
const globs = new Map(); // module path -> [module paths it glob-imports (`use <module>::*;`)]
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
  const stack = []; // frames: { mod: name | null, gated }
  let i = 0;
  let pendingMod = null;
  let pendingGated = false;
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
      stack.push({ mod: pendingMod, gated: pendingGated });
      pendingMod = null;
      pendingGated = false;
      i++;
      continue;
    }
    if (c === "}") { stack.pop(); i++; continue; }
    if (c === ";") { pendingMod = null; i++; continue; }
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
      } else if (word === "fn" && stack.every((frame) => frame.mod)) {
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
          const path = stack.map((frame) => frame.mod).join("::");
          const key = path ? `${path}::${m[1]}` : m[1];
          const list = fns.get(key) ?? [];
          list.push({
            pub: /\bpub\b/.test(head),
            sig: `fn ${text.slice(j, k)}`.replace(/\s+/g, " ").trim(),
            wasmOnly: cfgs.includes('#[cfg(target_arch = "wasm32")]'),
            // Gated by a platform or feature cfg (the adapter module itself is
            // `not(target_arch = "wasm32")`), here or on an enclosing module.
            gated: cfgs.some((cfg) => cfg !== '#[cfg(not(target_arch = "wasm32"))]') || stack.some((frame) => frame.gated),
            path,
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
          });
          types.set(key, list);
        }
      } else if (word === "use" && stack.every((frame) => frame.mod)) {
        const m = /^\s+((?:[a-z_][a-z0-9_]*::)*)\*\s*;/.exec(text.slice(j, j + 120));
        if (m) {
          const here = stack.map((frame) => frame.mod);
          const target = m[1].split("::").filter(Boolean);
          const base = target[0] === "crate" ? [] : [...here];
          for (const segment of target[0] === "crate" ? target.slice(1) : target) {
            if (segment === "super") base.pop();
            else if (segment !== "self") base.push(segment);
          }
          const key = here.join("::");
          globs.set(key, [...(globs.get(key) ?? []), base.join("::")]);
        }
      }
      i = j;
      continue;
    }
    i++;
  }
}

// The declaration a runtime type path names: `{ key, decls }` for the item at
// that module path, or one a module's glob import (`use super::*;`,
// `pub(crate) use jet_sync::*;`) brings in; null if none.
const declarationOf = (rustPath, seen = new Set()) => {
  if (types.has(rustPath)) return { key: rustPath, decls: types.get(rustPath) };
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
const opaqueName = /^(?:(?:crate|super|[a-z_][a-z0-9_]*)::)*([A-Z][A-Za-z0-9_]*)$/;

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
  if (fixedInts.has(ty)) return { jet: "Word", read: `w as ${ty}`, write: "e as u64" };
  if (ty === "bool") return { jet: "Flag", read: "w != 0", write: "e as u64" };
  if (ty === "f64") return { jet: "Float", read: "f64::from_bits(w)", write: "e.to_bits()" };
  if (ty === "char") return { jet: "Word", read: "scalar(w as i64)", write: "e as u64" };
  return null;
}

// Parameter carrier: { jet, ctys: [C types], expr(names) } or null. A List
// parameter is two words: its slot data pointer and its length.
function paramCarrier(ty, path) {
  ty = ty.replace(/\s+/g, "");
  const one = (jet, cty, expr) => ({ jet, ctys: [cty], expr: ([a]) => expr(a) });
  if (ty === "&str") return one("Text", "JetCString", (a) => `view(${a})`);
  if (ty === "&String") return one("Text", "JetCString", (a) => `unsafe { &*${a} }`);
  if (ty === "String") return one("Text", "JetCString", (a) => `view(${a}).to_owned()`);
  if (ty === "i64") return one("Word", "i64", (a) => a);
  if (fixedInts.has(ty)) return one("Word", "i64", (a) => `fixed::<${ty}>(${a})`);
  if (ty === "bool") return one("Flag", "u8", (a) => `${a} != 0`);
  if (ty === "f64") return one("Float", "f64", (a) => a);
  if (ty === "char") return one("Word", "i64", (a) => `scalar(${a})`);
  // A fieldless enum by value: its variant index (results keep their carrier).
  const tag = tagCarrier(ty, path);
  if (tag) return one(tag.jet, "i64", tag.read);
  // An exact Int by value: the borrowed carrier word, cloned into an owner.
  if (/^(?:[a-z_][a-z0-9_]*::)*(?:Numeric::)?JetInt$/.test(ty)) return one("Word", "i64", (a) => `unsafe { ${qualify(ty, path)}::clone_from_raw(${a}) }`);
  const list = /^(&?)(?:\[(.+)\]|Vec<(.+)>)$/.exec(ty);
  if (list && (list[1] === "&" || list[3])) {
    const element = elementCarrier(list[2] ?? list[3]);
    if (!element) return null;
    const items = (d, n) => `list_in(${d}, ${n}, |w| ${element.read})`;
    return { jet: `List{elem: LowerRouteCarrier.${element.jet}}`, ctys: ["*const u64", "i64"], expr: ([d, n]) => (list[1] === "&" ? `&${items(d, n)}` : items(d, n)) };
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
  if (fixedInts.has(ty)) return { jet: "Word", cty: "i64", wrap: (v) => `crate::jet_std::jet_int_from_i64(i64::try_from(${v}).unwrap_or_else(|_| range_stop()))` };
  if (ty === "bool") return { jet: "Flag", cty: "bool", wrap: (v) => v };
  if (ty === "f64") return { jet: "Float", cty: "f64", wrap: (v) => v };
  if (ty === "char") return { jet: "Word", cty: "i64", wrap: (v) => `(${v}) as i64` };
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
  const generic = /^(Option|Result)<(.*)>$/.exec(ty);
  const listOf = (t) => {
    const list = /^Vec<(.+)>$/.exec(t);
    return list ? elementCarrier(list[1]) : null;
  };
  const list = /^Vec<(.+)>$/.exec(ty);
  if (list) {
    // A List result: its length, with its slot buffer written to `data`.
    const element = elementCarrier(list[1]);
    if (!element) return null;
    return {
      jet: `Plain{carrier: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "data", cty: "*mut u64" }], handles: [],
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
        jet: `Option{some: LowerRouteCarrier.List{elem: LowerRouteCarrier.${element.jet}}}`, ret: "i64", outs: [{ name: "some", cty: "*mut u64" }, { name: "some_len", cty: "i64" }], handles: [],
        body: (call) => `match ${call} {\n            ${listArm("some", element, "Some", 1)}\n            None => 0,\n        }`,
      };
    }
    const v = valueCarrier(args[0], path, nativeInt);
    if (!v || !v.cty) return null;
    return {
      jet: `Option{some: LowerRouteCarrier.${v.jet}}`, ret: "i64", outs: [{ name: "some", cty: v.cty }], handles: v.handle ? [v.handle] : [],
      body: (call) => `match ${call} {\n            Some(value) => { unsafe { some.write(${v.wrap("value")}) }; 1 }\n            None => 0,\n        }`,
    };
  }
  if (args.length !== 2) return null;
  const okList = listOf(args[0]);
  const ok = okList ? null : valueCarrier(args[0], path, nativeInt);
  const err = valueCarrier(args[1], path, false);
  if ((!ok && !okList) || !err || !err.cty) return null;
  const outs = [];
  if (okList) outs.push({ name: "ok", cty: "*mut u64" }, { name: "ok_len", cty: "i64" });
  else if (ok.cty) outs.push({ name: "ok", cty: ok.cty });
  outs.push({ name: "err", cty: err.cty });
  const okArm = okList
    ? listArm("ok", okList, "Ok", 1)
    : ok.cty ? `Ok(value) => { unsafe { ok.write(${ok.wrap("value")}) }; 1 }` : "Ok(()) => 1,";
  const okJet = okList ? `List{elem: LowerRouteCarrier.${okList.jet}}` : ok.jet;
  return {
    jet: `Result{ok: LowerRouteCarrier.${okJet}, err: LowerRouteCarrier.${err.jet}}`, ret: "i64", outs, handles: [ok?.handle, err.handle].filter(Boolean),
    body: (call) => `match ${call} {\n            ${okArm}\n            Err(error) => { unsafe { err.write(${err.wrap("error")}) }; 0 }\n        }`,
  };
}

// ---------------------------------------------------------------------------
// 4. Adapters.

const adapters = [];
const skipped = [];
const exported = new Set(handExports);
const handles = new Map();
for (const [symbol, route] of [...routes].sort((a, b) => a[0].localeCompare(b[0]))) {
  const name = symbol.split("::").pop();
  const path = symbol.includes("::") ? symbol.split("::").slice(0, -1).join("::") : "";
  const label = route.rows[0];
  if (handExports.has(name)) continue;
  if (exported.has(name)) { skipped.push(`${symbol} (${label}): export name taken`); continue; }
  const all = (fns.get(symbol) ?? []).filter((def) => !def.wasmOnly);
  if (all.length === 0) { skipped.push(`${symbol} (${label}): no runtime function`); continue; }
  const defs = all.filter((def) => !def.gated);
  if (defs.length === 0) { skipped.push(`${symbol} (${label}): platform-gated`); continue; }
  if (new Set(defs.map((def) => def.sig)).size !== 1) { skipped.push(`${symbol} (${label}): cfg-dependent signature`); continue; }
  const def = defs[0];
  if (path && !def.pub) { skipped.push(`${symbol} (${label}): not public in ${path}`); continue; }
  const sig = parseSignature(def.sig);
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
  adapters.push({ symbol, name, label, params, result, rows: route.rows });
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
// The `"A" | "B" => Some("T")` and `n if n == Syntax::A || n == "B" => Some("T")`
// arms of one name table (`Some(name)` maps a name to itself).
const nameTable = (source, name) => {
  const table = new Map();
  for (const m of fnBody(source, name).matchAll(/((?:"[^"]+"\s*\|\s*)*"[^"]+"|n if [^=>{}]*==[^>{}]*?)\s*=>\s*Some\((?:"([^"]+)"|name)\)/g)) {
    const names = m[1].startsWith("n if")
      ? [...m[1].matchAll(/Syntax::([A-Z][A-Z0-9_]*)|"([^"]+)"/g)].map((k) => k[2] ?? syntaxConstants.get(k[1]))
      : [...m[1].matchAll(/"([^"]+)"/g)].map((k) => k[1]);
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
const mirRustRs = readFileSync(`${repo}/crates/jet-codegen/src/Codegen/MIRRust.rs`, "utf8");
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
  if (!rustPath || !/^(?:[a-z_][a-z0-9_]*::)*[A-Z][A-Za-z0-9_]*$/.test(rustPath)) return null;
  const name = rustPath.split("::").pop();
  if (handles.get(name) === `crate::${rustPath}`) return { name, full: `crate::${rustPath}` };
  const found = declarationOf(rustPath);
  if (!found || found.decls.length !== 1) return null;
  const [decl] = found.decls;
  if (!decl.visible || decl.generic || decl.gated) return null;
  return { name, full: `crate::${found.key}` };
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
rs.push("    /// A List argument's `len` eight-byte slots at `data`, as Rust values.");
rs.push("    fn list_in<T>(data: *const u64, len: i64, item: impl Fn(u64) -> T) -> Vec<T> {");
rs.push("        if len <= 0 || data.is_null() {");
rs.push("            return Vec::new();");
rs.push("        }");
rs.push("        // SAFETY: the caller passes its List's slot buffer and length.");
rs.push("        unsafe { std::slice::from_raw_parts(data, len as usize) }.iter().map(|w| item(*w)).collect()");
rs.push("    }");
rs.push("");
rs.push("    /// A List result: a slot buffer from the allocator `jet_rt_free` returns");
rs.push("    /// to (null when empty), written to `data`; returns the length.");
rs.push("    fn list_out(words: Vec<u64>, data: *mut *mut u64) -> i64 {");
rs.push("        let len = words.len();");
rs.push("        let buffer = if len == 0 {");
rs.push("            std::ptr::null_mut()");
rs.push("        } else {");
rs.push("            let layout = std::alloc::Layout::from_size_align(len * 8, 8).unwrap_or_else(|_| range_stop());");
rs.push("            // SAFETY: `layout` has a nonzero size.");
rs.push("            let buffer = unsafe { std::alloc::alloc(layout) } as *mut u64;");
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
for (const adapter of adapters) {
  const params = [];
  const names = adapter.params.map((param, index) => param.ctys.map((cty, k) => {
    const name = param.ctys.length === 1 ? `a${index}` : `a${index}_${k}`;
    params.push(`${name}: ${cty}`);
    return name;
  }));
  for (const out of adapter.result.outs) params.push(`${out.name}: *mut ${out.cty}`);
  const ret = adapter.result.ret ? ` -> ${adapter.result.ret}` : "";
  const call = `crate::${adapter.symbol}(${adapter.params.map((param, index) => param.expr(names[index])).join(", ")})`;
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
