#!/usr/bin/env node
// Visibility cutover rewrite (owner directive 2026-10-07, card #4723, D-VIS-MODULE1).
//
// Re-runnable and idempotent: migrates Jet source from the retired visibility
// model (private by default, `pub` / `pub(package)` exports, `#PubFile`,
// declaration-level `priv`) to the current one (public by default, `priv` only
// on modules and fields). Apply it to any branch before merging:
//
//   node Tools/agent/vis-cutover.mjs            # rewrite every tracked file
//   node Tools/agent/vis-cutover.mjs --check    # list files that still need it; exit 1 if any
//   node Tools/agent/vis-cutover.mjs PATH...    # rewrite only these files
//
// Rules applied to Jet code (comments are never touched):
//   - `pub` and `pub(package)` before a declaration or field are deleted.
//     `pub use` stays (re-export ballot pending).
//   - `#PubFile` / `#PublicFile` file markers are deleted.
//   - Jet templates inside Jet string literals (binders and test fixtures that
//     write Jet source) lose `pub` before a declaration keyword; files that emit
//     Rust are skipped (RUST_EMITTERS).
//   - A field that was private (no `pub`) in a type other packages could see
//     keeps that guarantee as a `priv` field: such fields are the type's
//     invariant carriers (the old model already hid them outside the package).
//     Compiler/ is assembled into one compilation unit, so its fields stay plain.
// Jet embedded in Rust string literals (test fixtures) gets the declaration and
// marker rules; Markdown gets them inside ```jet fences.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const repoRoot = resolve(new URL(".", import.meta.url).pathname, "../..");
const args = process.argv.slice(2);
const check = args.includes("--check");
const explicit = args.filter((a) => !a.startsWith("--"));

const IDENT = /[A-Za-z0-9_]/;
const stats = { files: 0, pub: 0, pubFile: 0, privDecl: 0, privField: 0 };

// ---- Jet lexical mask: 1 = code byte, 0 = string/comment/char literal. ----
function codeMask(src) {
  const mask = new Uint8Array(src.length);
  let i = 0;
  const n = src.length;
  function skipString(start) {
    // `"…"` with `\` escapes and `{expr}` interpolation (`{{` is literal).
    let j = start + 1;
    while (j < n) {
      const c = src[j];
      if (c === "\\") { j += 2; continue; }
      if (c === '"') return j + 1;
      if (c === "{") {
        if (src[j + 1] === "{") { j += 2; continue; }
        j = skipInterp(j + 1);
        continue;
      }
      j++;
    }
    return n;
  }
  function skipInterp(start) {
    let depth = 1;
    let j = start;
    while (j < n && depth > 0) {
      const c = src[j];
      if (c === '"') { j = src.startsWith('"""', j) ? skipTriple(j) : skipString(j); continue; }
      if (c === "{") depth++;
      else if (c === "}") depth--;
      j++;
    }
    return j;
  }
  function skipTriple(start) {
    const end = src.indexOf('"""', start + 3);
    return end < 0 ? n : end + 3;
  }
  function skipBacktick(start) {
    let k = start;
    while (src[k] === "`") k++;
    const fence = src.slice(start, k);
    const end = src.indexOf(fence, k);
    return end < 0 ? n : end + fence.length;
  }
  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      const end = src.indexOf("\n", i);
      i = end < 0 ? n : end;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (src[j] === "/" && src[j + 1] === "*") { depth++; j += 2; }
        else if (src[j] === "*" && src[j + 1] === "/") { depth--; j += 2; }
        else j++;
      }
      i = j;
      continue;
    }
    if (c === '"' || c === "`") {
      const end = c === "`" ? skipBacktick(i) : src.startsWith('"""', i) ? skipTriple(i) : skipString(i);
      mask.fill(2, i, end);
      i = end;
      continue;
    }
    if (c === "'") {
      const m = /^'(\\u\{[0-9A-Fa-f]+\}|\\.|[^\\'\n])'/u.exec(src.slice(i, i + 16));
      if (m) { i += m[0].length; continue; }
    }
    mask[i] = 1;
    i++;
  }
  return mask;
}

// ---- Minimal token stream over code bytes. ----
function tokenize(src, mask) {
  const toks = [];
  let i = 0;
  const n = src.length;
  while (i < n) {
    if (mask[i] !== 1) { i++; continue; }
    const c = src[i];
    if (c === "\n") { toks.push({ t: "\n", s: i, e: i + 1 }); i++; continue; }
    if (c === " " || c === "\t" || c === "\r") { i++; continue; }
    if (IDENT.test(c)) {
      let j = i;
      while (j < n && mask[j] && IDENT.test(src[j])) j++;
      toks.push({ t: src.slice(i, j), s: i, e: j, id: true });
      i = j;
      continue;
    }
    if (c === ":" && src[i + 1] === ":") { toks.push({ t: "::", s: i, e: i + 2 }); i += 2; continue; }
    if (c === ":" && src[i + 1] === "=") { toks.push({ t: ":=", s: i, e: i + 2 }); i += 2; continue; }
    toks.push({ t: c, s: i, e: i + 1 });
    i++;
  }
  return toks;
}

function skipBalanced(toks, k, open, close) {
  let depth = 0;
  for (; k < toks.length; k++) {
    if (toks[k].t === open) depth++;
    else if (toks[k].t === close && --depth === 0) return k + 1;
  }
  return k;
}

// Skip `#Name`, `#Name(...)`, `#[...]` markers and newlines starting at k.
function skipMarkers(toks, k) {
  for (;;) {
    while (k < toks.length && toks[k].t === "\n") k++;
    if (toks[k]?.t !== "#") return k;
    if (toks[k + 1]?.t === "[") { k = skipBalanced(toks, k + 1, "[", "]"); continue; }
    if (!toks[k + 1]?.id) return k;
    k += 2;
    if (toks[k]?.t === "(") k = skipBalanced(toks, k, "(", ")");
  }
}

// Was the declaration whose keyword sits at index k exported in the old model?
function exportedBefore(toks, k, pubFile) {
  let sawPub = false;
  let sawPriv = false;
  for (let j = k - 1; j >= 0; j--) {
    const t = toks[j].t;
    if (t === "pub") { sawPub = true; continue; }
    if (t === "priv") { sawPriv = true; continue; }
    if (t === "\n") continue;
    if (t === ")" ) {
      // `pub(package)` or a marker argument list.
      let depth = 0;
      for (; j >= 0; j--) {
        if (toks[j].t === ")") depth++;
        else if (toks[j].t === "(" && --depth === 0) break;
      }
      continue;
    }
    if (t === "]") {
      let depth = 0;
      for (; j >= 0; j--) {
        if (toks[j].t === "]") depth++;
        else if (toks[j].t === "[" && --depth === 0) break;
      }
      continue;
    }
    if (toks[j].id && toks[j - 1]?.t === "#") { j--; continue; }
    if (t === "#") continue;
    break;
  }
  return sawPub || (pubFile && !sawPriv);
}

function rewriteJet(src, { privFields, templates = false, raw = null, map = null }) {
  if (!/pub|priv|#Pub(lic)?File/.test(src)) return raw ?? src;
  const mask = codeMask(src);
  const toks = tokenize(src, mask);
  const edits = []; // { s, e, text }
  let pubFile = false;
  for (let k = 0; k < toks.length; k++) {
    if (toks[k].t === "#" && (toks[k + 1]?.t === "PubFile" || toks[k + 1]?.t === "PublicFile")) {
      pubFile = true;
      let s = toks[k].s;
      let e = toks[k + 1].e;
      const lineStart = src.lastIndexOf("\n", s - 1) + 1;
      const lineEnd = src.indexOf("\n", e);
      if (src.slice(lineStart, s).trim() === "" && src.slice(e, lineEnd < 0 ? src.length : lineEnd).trim() === "") {
        s = lineStart;
        e = lineEnd < 0 ? src.length : lineEnd + 1;
        // Drop one blank line the marker separated from the code.
        if (s === 0 && src[e] === "\n") e++;
      }
      edits.push({ s, e, text: "" });
      stats.pubFile++;
    }
  }
  const keepPriv = new Set();
  // Fields of structs.
  for (let k = 0; k < toks.length; k++) {
    if (toks[k].t !== "struct" || !toks[k + 1]?.id) continue;
    if (k > 0 && toks[k - 1].t === ".") continue;
    let j = k + 2;
    if (toks[j]?.t === "<") j = skipBalanced(toks, j, "<", ">");
    while (j < toks.length && toks[j].t !== "{" && toks[j].t !== "(" && toks[j].t !== "\n" && toks[j].t !== ";") j++;
    if (toks[j]?.t !== "{") continue;
    const exported = exportedBefore(toks, k, pubFile);
    const bodyEnd = skipBalanced(toks, j, "{", "}") - 1;
    let entry = true;
    let depth = 0;
    for (let f = j + 1; f < bodyEnd; f++) {
      const t = toks[f].t;
      if (depth === 0 && (t === "\n" || t === "," || t === ";")) { entry = true; continue; }
      if (t === "{" || t === "(" || t === "[") { depth++; entry = false; continue; }
      if (t === "}" || t === ")" || t === "]") { depth--; continue; }
      if (depth !== 0 || !entry) continue;
      entry = false;
      const m = skipMarkers(toks, f);
      if (m !== f) { f = m - 1; entry = true; continue; }
      let q = f;
      let hadPub = false;
      let hadPriv = false;
      if (toks[q].t === "pub") {
        hadPub = true;
        q++;
        if (toks[q]?.t === "(") q = skipBalanced(toks, q, "(", ")");
      } else if (toks[q].t === "priv") {
        hadPriv = true;
        keepPriv.add(q);
        q++;
      }
      if (!toks[q]?.id || toks[q + 1]?.t !== ":") continue;
      if (/^(state|fn|impl|const)$/.test(toks[q].t)) continue;
      if (!hadPub && !hadPriv && exported && privFields) {
        edits.push({ s: toks[q].s, e: toks[q].s, text: "priv " });
        stats.privField++;
      }
    }
  }
  for (let k = 0; k < toks.length; k++) {
    const t = toks[k].t;
    if (t === "pub") {
      if (toks[k - 1]?.t === "." || toks[k + 1]?.t === "::" || toks[k + 1]?.t === ":=" || toks[k + 1]?.t === ".") continue;
      if (toks[k + 1]?.t === "use") continue;
      let end = toks[k].e;
      if (toks[k + 1]?.t === "(" && toks[k + 2]?.id && toks[k + 3]?.t === ")") end = toks[k + 3].e;
      if (toks[k + 1]?.t === ":") continue; // section label: left for E0415
      while (src[end] === " " || src[end] === "\t") end++;
      edits.push({ s: toks[k].s, e: end, text: "" });
      stats.pub++;
    } else if (t === "priv") {
      if (keepPriv.has(k)) continue;
      if (toks[k - 1]?.t === "." || toks[k + 1]?.t === "::" || toks[k + 1]?.t === ":=" || toks[k + 1]?.t === "." || toks[k + 1]?.t === ":") continue;
      // `priv module` / `priv(package) module` is the current form.
      let q = k + 1;
      if (toks[q]?.t === "(") q = skipBalanced(toks, q, "(", ")");
      if (toks[skipMarkers(toks, q)]?.t === "module") continue;
      let end = toks[k].e;
      while (src[end] === " " || src[end] === "\t") end++;
      edits.push({ s: toks[k].s, e: end, text: "" });
      stats.privDecl++;
    }
  }
  if (templates) {
    // Jet source written from Jet string literals (binders, fixtures).
    // `(?<=\\[nt])`: a template line written as `"...\npub fn ..."`.
    const re = /(?:\b|(?<=\\[nt]))pub(\(package\))?[ \t]+(?=(fn|struct|enum|trait|tag|module|alias|protocol|distinct|policy)\b)/g;
    for (let m; (m = re.exec(src)); ) {
      if (mask[m.index] !== 2) continue;
      const lineStart = src.lastIndexOf("\n", m.index) + 1;
      const lineEnd = src.indexOf("\n", m.index);
      if (RUST_LINE.test(src.slice(lineStart, lineEnd < 0 ? src.length : lineEnd))) continue;
      edits.push({ s: m.index, e: m.index + m[0].length, text: "" });
      stats.pub++;
    }
  }
  if (edits.length === 0) return raw ?? src;
  edits.sort((a, b) => b.s - a.s || b.e - a.e);
  // `raw`/`map`: `src` is the decoded body of an escaped Rust string; edits
  // land on the raw spelling through the decoded->raw offset map.
  let out = raw ?? src;
  let floor = Infinity;
  for (const edit of edits) {
    const s = map ? map[edit.s] : edit.s;
    const e = map ? map[edit.e] : edit.e;
    if (e > floor) continue; // overlapping edit (marker line already removed)
    out = out.slice(0, s) + edit.text + out.slice(e);
    floor = s;
  }
  return out;
}

// Decode a non-raw Rust string body (`\"`, `\\`, `\n`, `\t`) for scanning and
// rewrite it in place.
function rewriteEscaped(body) {
  let decoded = "";
  const map = [];
  for (let i = 0; i < body.length; ) {
    map.push(i);
    if (body[i] === "\\" && i + 1 < body.length) {
      const c = body[i + 1];
      decoded += c === "n" ? "\n" : c === "t" ? "\t" : c === "\n" ? " " : c;
      i += 2;
    } else {
      decoded += body[i];
      i++;
    }
  }
  map.push(body.length);
  return rewriteJet(decoded, { privFields: false, raw: body, map });
}

// Rust string literals that carry Jet source (test fixtures).
const RUST_ONLY = /\blet\s+(mut\s+)?[a-z_]|#!?\[|println!|&self\b|&mut\b|<'|&'|&str\b|\b[iu](8|16|32|64|128|size)\b|__jet_|pub\(crate\)|pub\(super\)|\bimpl\b[^\n{]*\bfor\b[^\n{]*\{[^}]*\bfn\b[^}]*&|\bmod\s+[a-z_]+\s*[;{]|\bVec<|::new\(|\buse\s+(std|crate|super)::|\bextern\s+\\?"C\\?"|-> Self\b|\bunsafe\s+fn\b|\bdyn\s|\bstruct\s+\w+\s*;|\bstruct\s+\w+(<[^>]*>)?\(|\btype\s+\w+\s*=|<Self>|\)\s*void\b/;
function looksJet(body) {
  // Test the decoded text: in an escaped literal `\npub` would hide `pub`
  // (and `\nlet`, `\nfn`) from the `\b` boundaries below.
  const text = body.replace(/\\[nt]/g, (m) => (m === "\\n" ? "\n" : " "));
  if (!/\bpub\b|\bpriv\b|#Pub(lic)?File/.test(text)) return false;
  // A fixture program: at least one declaration with a body or a file marker.
  if (!/#Pub(lic)?File|\b(fn|struct|enum|trait|module)\s+[A-Za-z_][A-Za-z0-9_]*[^\n]*[{;]/.test(text)) return false;
  return !RUST_ONLY.test(text);
}

// A literal whose line names Rust (`web.wasm_rust.contains("pub struct ...")`)
// is generated-Rust text, not a Jet fixture.
function rustContext(src, i) {
  return /(?:^|[^a-z])rust/i.test(src.slice(src.lastIndexOf("\n", i - 1) + 1, i));
}

function rewriteRust(src) {
  if (!/pub|priv|PubFile/.test(src)) return src;
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      const end = src.indexOf("\n", i);
      const stop = end < 0 ? n : end;
      out += src.slice(i, stop);
      i = stop;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      const stop = end < 0 ? n : end + 2;
      out += src.slice(i, stop);
      i = stop;
      continue;
    }
    if (c === "'" ) {
      // char literal or lifetime
      const m = /^'(\\u\{[0-9A-Fa-f]+\}|\\.|[^\\'])'/u.exec(src.slice(i, i + 14));
      if (m) { out += m[0]; i += m[0].length; continue; }
      out += c; i++; continue;
    }
    const raw = /^(b?r)(#*)"/.exec(src.slice(i, i + 12));
    if (raw && (i === 0 || !IDENT.test(src[i - 1]))) {
      const hashes = raw[2];
      const open = i + raw[0].length;
      const closeSeq = '"' + hashes;
      const close = src.indexOf(closeSeq, open);
      const stop = close < 0 ? n : close;
      const body = src.slice(open, stop);
      out += src.slice(i, open) + (looksJet(body) && !rustContext(src, i) ? rewriteJet(body, { privFields: false }) : body);
      i = stop;
      if (close >= 0) { out += closeSeq; i += closeSeq.length; }
      continue;
    }
    if (c === '"' || (c === "b" && src[i + 1] === '"' && !IDENT.test(src[i - 1] ?? " "))) {
      const open = c === '"' ? i + 1 : i + 2;
      let j = open;
      while (j < n && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      const body = src.slice(open, j);
      out += src.slice(i, open) + (looksJet(body) && !rustContext(src, i) ? rewriteEscaped(body) : body);
      i = Math.min(j + 1, n);
      if (j < n) out += '"';
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

function rewriteMarkdown(src) {
  return src.replace(/(```jet[^\n]*\n)([\s\S]*?)(\n```)/g, (_, open, body, close) =>
    open + rewriteJet(body, { privFields: false }) + close);
}

// Sources that hold Rust runtime text only, and build scripts that parse Rust.
// Emitters elsewhere (Codegen, Bootstrap) keep their Rust literals through
// RUST_ONLY / rustContext while their Jet test fixtures are rewritten.
const RUST_SKIP = [/^crates\/jet-codegen\/src\/Prelude\/(Core|CoreLib)\//, /^crates\/[^/]+\/build\//];
// Jet files whose string literals emit Rust (or carry compiler/registry text
// that must match the Rust half verbatim) are left alone by the template rule.
const RUST_EMITTERS = [/^Compiler\//, /^Jetpack\/Store\//, /\/RustBridge\//];
const RUST_LINE = /extern\s+\\?"C\\?"|&self\b|\*mut\b|-> Self\b|#\[|pub\(crate\)|\bimpl\b|\bmut\s/;

function trackedFiles() {
  const listed = execFileSync("git", ["ls-files", "-z", "*.jet", "*.rs", "*.md"], { cwd: repoRoot, encoding: "utf8", maxBuffer: 1 << 28 });
  return listed.split("\0").filter(Boolean);
}

const files = explicit.length ? explicit : trackedFiles();
const pending = [];
for (const rel of files) {
  if (RUST_SKIP.some((re) => re.test(rel))) continue;
  if (rel.startsWith("Docs/audits/") || rel.startsWith("Docs/site/") || rel.startsWith("Tools/tower/.tower/")) continue;
  // UI fixtures that spell the retired forms on purpose (E0411-E0418).
  if (rel.startsWith("tests/ui/vis_")) continue;
  const path = resolve(repoRoot, rel);
  let src;
  try { src = readFileSync(path, "utf8"); } catch { continue; }
  let next = src;
  if (rel.endsWith(".jet")) {
    next = rewriteJet(src, {
      privFields: !rel.startsWith("Compiler/"),
      templates: !RUST_EMITTERS.some((re) => re.test(rel)),
    });
  }
  else if (rel.endsWith(".rs")) next = rewriteRust(src);
  else if (rel.endsWith(".md")) next = rewriteMarkdown(src);
  if (next === src) continue;
  pending.push(rel);
  stats.files++;
  if (!check) writeFileSync(path, next);
}
if (check) {
  for (const rel of pending) console.log(rel);
  console.error(`vis-cutover --check: ${pending.length} file(s) need the rewrite`);
  process.exit(pending.length ? 1 : 0);
}
console.log(JSON.stringify(stats));
