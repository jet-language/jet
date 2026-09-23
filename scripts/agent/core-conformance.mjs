#!/usr/bin/env node

import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { dirname, join, relative, resolve } from "node:path";

/*
 * Registry-driven Core conformance corpus (#2286).
 *
 * CoreModuleExports.rs is the denominator consumed by sema. The corpus is not
 * guessed from fixed_sigs.rs or core_calls.rs: an exported operation without a
 * second route still needs a witness or a named carve-out.
 * Recipes below are only small, known-good seeds. Hard or effectful operations
 * are hand-authored under tests/conformance/corpus and remain visible as
 * uncovered until someone supplies their real arguments and authority.
 * Generic core rows may be called through a typed receiver (`mem.Ptr<T>.from_addr`)
 * even when the denominator row is published under `core.mem`. The source
 * matcher accepts that generic-receiver shape for every row; sema remains the
 * authority for whether the receiver and method are valid.
 */

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MODULE_EXPORTS = join(ROOT, "crates/jet-foundation/src/CoreModuleExports.rs");
const CORPUS = join(ROOT, "tests/conformance/corpus");
const EXCLUSIONS = join(ROOT, "tests/conformance/exclusions.tsv");

// The Core registry also publishes these math fields. `zero` is deliberately
// absent: sema treats `core.math.zero()` as a callable operation.
const VALUE_NAMES = new Set(["pi", "e", "tau", "infinity", "nan"]);

// Seed only witnesses whose call shape is known. The denominator check, not
// this map, decides whether the rest of Core is covered.
const RECIPES = new Map([
  ["core.math.sqrt", `// core-conformance: core.math.sqrt
use core.math as math

fn run() {
    result :: math.sqrt(4.0)
    print(result)
}
`],
  ["core.math.round", `// core-conformance: core.math.round
use core.math as math

fn run() {
    result :: math.round(2.5)
    print(result)
}
`],
  ["core.math.zero", `// core-conformance: core.math.zero
use core.math as math

fn run() {
    result :: math.zero()
    print(result)
}
`],
  ["core.math.gcd", `// core-conformance: core.math.gcd
use core.math as math

fn run() {
    result :: math.gcd(18, 12)
    print(result)
}
`],
  ["core.math.is_even", `// core-conformance: core.math.is_even
use core.math as math

fn run() {
    result :: math.is_even(18)
    print(result)
}
`],
  ["core.text.lower", `// core-conformance: core.text.lower
use core.text as text

fn run() {
    result :: text.lower("JET")
    print(result)
}
`],
  ["core.text.trim", `// core-conformance: core.text.trim
use core.text as text

fn run() {
    result :: text.trim("  jet  ")
    print(result)
}
`],
  ["core.text.byte_count", `// core-conformance: core.text.byte_count
use core.text as text

fn run() {
    result :: text.byte_count("jet")
    print(result)
}
`],
  ["core.text.fmt.grouped", `// core-conformance: core.text.fmt.grouped
use core.text.fmt as fmt

fn run() {
    result :: fmt.grouped(Float{1234.5678}, 2)
    print(result)
}
`],
  ["core.args.spec", `// core-conformance: core.args.spec
use core.args as args

fn run() {
    result :: args.spec()
    print(result.completion("bash").contains("--help"))
}
`],
  ["core.crypto.uuid.v4", `// core-conformance: core.crypto.uuid.v4
use core.crypto.uuid as uuid

fn run() {
    result :: uuid.v4()
    print(result.len())
}
`],
  ["core.time.parse_rfc3339", `// core-conformance: core.time.parse_rfc3339
use core.time as time

fn run() {
    result :: time.parse_rfc3339("2024-03-01T12:00:00Z") ?? panic("parse")
    print(result.to_timestamp())
}
`],
  ["core.math.random.choices", `// core-conformance: core.math.random.choices
use core.math.random as random

fn run() {
    print(random.choices([Int]{1, 2, 3}, 4))
}
`],
  ["core.math.random.triangular", `// core-conformance: core.math.random.triangular
use core.math.random as random

fn run() {
    print(random.triangular(0.0, 1.0, 0.5))
}
`],
  ["core.math.random.gammavariate", `// core-conformance: core.math.random.gammavariate
use core.math.random as random

fn run() {
    print(random.gammavariate(2.0, 1.0))
}
`],
  ["core.math.random.betavariate", `// core-conformance: core.math.random.betavariate
use core.math.random as random

fn run() {
    print(random.betavariate(2.0, 3.0))
}
`],
  ["core.math.random.lognormvariate", `// core-conformance: core.math.random.lognormvariate
use core.math.random as random

fn run() {
    print(random.lognormvariate(0.0, 1.0))
}
`],
  ["core.math.random.paretovariate", `// core-conformance: core.math.random.paretovariate
use core.math.random as random

fn run() {
    print(random.paretovariate(2.0))
}
`],
  ["core.math.random.weibullvariate", `// core-conformance: core.math.random.weibullvariate
use core.math.random as random

fn run() {
    print(random.weibullvariate(1.0, 2.0))
}
`],
  ["core.math.random.vonmisesvariate", `// core-conformance: core.math.random.vonmisesvariate
use core.math.random as random

fn run() {
    print(random.vonmisesvariate(0.0, 1.0))
}
`],
  ["core.math.random.binomialvariate", `// core-conformance: core.math.random.binomialvariate
use core.math.random as random

fn run() {
    print(random.binomialvariate(4, 0.5))
}
`],
  ["core.encoding.json.dump", `// core-conformance: core.encoding.json.dump
use core.encoding.json as json

fn run() {
    value :: json.loads("{\"ok\":true}")
    print(json.dump(value))
}
`],
  ["core.encoding.json.load", `// core-conformance: core.encoding.json.load
use core.encoding.json as json

fn run() {
    print(json.load("{\"ok\":true}"))
}
`],
  ["core.encoding.toml.loads", `// core-conformance: core.encoding.toml.loads
use core.encoding.toml as toml

fn run() {
    print(toml.loads("ok = true"))
}
`],
  ["core.encoding.toml.load", `// core-conformance: core.encoding.toml.load
use core.encoding.toml as toml

fn run() {
    print(toml.load("ok = true"))
}
`],
  ["core.net.tls.flags", `// core-conformance: core.net.tls.flags
use core.net.tls as tls

fn run() {
    cfg :: tls.config("example.test")
    print(tls.flags(cfg).len())
}
`],
  ["core.net.tls.unwrap", `// core-conformance: core.net.tls.unwrap
use core.net.tls as tls

fn run() {
    print(tls.unwrap(tls.TLSStream{fd: 0, host: "example.test", port: 443}).peer_port())
}
`],
  ["core.tasks.channel", `// core-conformance: core.tasks.channel
use core.tasks as tasks

fn run() {
    state :: tasks.channel(2)
    print(state.capacity)
}
`],
  ["core.tasks.exception", `// core-conformance: core.tasks.exception
use core.tasks as tasks

fn run() {
    print(tasks.exception("boom").raised)
}
`],
  ["core.tasks.run", `// core-conformance: core.tasks.run
use core.tasks as tasks

fn run() {
    print(tasks.run(tasks.after(0, 1)))
}
`],
  ["core.tasks.start", `// core-conformance: core.tasks.start
use core.tasks as tasks

fn run() {
    print(tasks.start(tasks.after(0, 1)).value)
}
`],
  ["core.tasks.waitall", `// core-conformance: core.tasks.waitall
use core.tasks as tasks

fn run() {
    print(tasks.waitall([tasks.after(0, 1), tasks.after(0, 2)]).len())
}
`],
  ["core.tasks.waitany", `// core-conformance: core.tasks.waitany
use core.tasks as tasks

fn run() {
    print(tasks.waitany([tasks.after(0, 1)]))
}
`],
  ["core.tasks.put", `// core-conformance: core.tasks.put
use core.tasks as tasks

fn run() {
    state :: tasks.channel(1)
    print(tasks.put(state, 7).values.len())
}
`],
  ["core.tasks.shutdown", `// core-conformance: core.tasks.shutdown
use core.tasks as tasks

fn run() {
    print(tasks.shutdown(tasks.channel(1)).closed)
}
`],
  ["core.tasks.stop", `// core-conformance: core.tasks.stop
use core.tasks as tasks

fn run() {
    print(tasks.stop(tasks.channel(1)).closed)
}
`],
  ["core.tasks.acquire", `// core-conformance: core.tasks.acquire
use core.tasks as tasks

fn run() {
    print(tasks.acquire(tasks.lock()).locked)
}
`],
  ["core.tasks.run", `// core-conformance: core.tasks.run
use core.tasks as tasks

fn run() {
    tasks.run()
    print(true)
}
`],
  ["core.tasks.lock", `// core-conformance: core.tasks.lock
use core.tasks as tasks

fn run() {
    print(tasks.lock().locked)
}
`],
  ["core.tasks.notify", `// core-conformance: core.tasks.notify
use core.tasks as tasks

fn run() {
    print(tasks.notify(tasks.lock()).generation)
}
`],
  ["core.tasks.release", `// core-conformance: core.tasks.release
use core.tasks as tasks

fn run() {
    print(tasks.release(tasks.acquire(tasks.lock())).locked)
}
`],
  ["core.tasks.reset", `// core-conformance: core.tasks.reset
use core.tasks as tasks

fn run() {
    print(tasks.reset(tasks.lock()).generation)
}
`],
  ["core.tasks.clear", `// core-conformance: core.tasks.clear
use core.tasks as tasks

fn run() {
    print(tasks.clear(tasks.channel(1)).values.len())
}
`],
]);
// Dynamic Core types do not have module_items rows, but their witnesses still
// belong to the conformance corpus. Keep the ledger's canonical row IDs here
// instead of inventing denominator keys from fixture filenames. A fixture may
// prove several rows, and each row may require several deterministic witnesses.
// Paths are relative to the conformance corpus root.
const FIXTURE_BINDINGS = new Map([
  ["collection.Condition.notify_one", [
    "core/sync/condition_deadline.jet",
    "core/sync/condition_notify_all.jet",
    "core/sync/condition_notify_one.jet",
    "core/sync/condition_spurious.jet",
  ]],
  ["collection.Condition.notify_all", [
    "core/sync/condition_deadline.jet",
    "core/sync/condition_notify_all.jet",
    "core/sync/condition_notify_one.jet",
    "core/sync/condition_spurious.jet",
  ]],
  ["core.data.query.collect", ["core/data/collect.jet"]],
  ["core.data.query.filter", [
    "core/data/filter.jet",
    "core/data/lazy_filter.jet",
    "core/data/missing_count.jet",
  ]],
  ["core.data.query.group_by.count", ["core/data/group_count.jet"]],
  ["core.data.query.group_by.mean", ["core/data/group_mean.jet"]],
  ["core.data.query.group_by.sum", ["core/data/group_sum.jet"]],
  ["core.data.query.plan", [
    "core/data/lazy.jet",
    "core/data/lazy_filter.jet",
    "core/data/lazy_sort_by.jet",
  ]],
  ["core.data.query.sort_by", [
    "core/data/sort_by.jet",
    "core/data/lazy_sort_by.jet",
    "core/data/sort_by_selector.jet",
    "core/data/sort_by_view.jet",
  ]],
]);

// Calls whose checked result is exactly Unit must still be observed without
// asking Display to render Unit.  Keep this list aligned with the sema return
// projections; the generator applies one observer shape to every such seed.
const UNIT_RESULT_KEYS = new Set([
  "core.term.binwrite",
  "core.term.progress",
  "core.net.set_timeout",
  "core.net.unix_write_all_bytes",
  "core.net.tcp_write_text",
  "core.net.set_write_timeout",
  "core.net.tcp_write",
  "core.net.unix_write",
  "core.net.tcp_reply",
  "core.net.unix_close",
  "core.net.set_nodelay",
  "core.net.set_ttl",
  "core.net.tcp_close",
  "core.net.tcp_shutdown",
  "core.net.tcp_write_all_bytes",
  "core.net.set_read_timeout",
  "core.net.unix_shutdown",
  "core.net.udp_set_timeout",
  "core.ui.mount",
  "core.files.write",
  "core.files.write_atomic",
  "core.files.remove_dir",
  "core.files.set_mode",
  "core.files.remove_all",
  "core.files.remove",
  "core.files.rename",
  "core.files.write_at",
  "core.files.symlink",
  "core.files.hard_link",
  "core.files.write_bytes",
  "core.mem.volatile_write",
  "core.log.fatal",
  "core.http.server.static_files",
  "core.http.server.cors",
  "core.http.server.request_id",
  "core.http.server.serve_once_listener",
  "core.perf.reset_fidelity",
  "core.perf.override_fidelity",
  "core.math.random.seed",
  "core.math.random.shuffle",
  "core.time.sleep",
  "core.sys.close_fd",
  "core.sys.sync",
  "core.sys.set",
  "core.sys.stop",
  "core.tasks.run",
  "core.tasks.yield_now",
]);
// Never-returning Core effects terminate the witness instead of yielding a
// value. Their standalone call is the observable contract.
const NEVER_RESULT_KEYS = new Set(["core.process.exit"]);

function matching(text, start, opening, closing) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = start; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") lineComment = false;
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        i += 1;
      }
      continue;
    }
    if (quote === "triple") {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"' && text.slice(i, i + 3) === '"""') {
        quote = null;
        i += 2;
      }
      continue;
    }
    if (quote === "regular") {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      quote = "triple";
      i += 2;
      continue;
    }
    if (c === '"') {
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      lineComment = true;
      i += 1;
      continue;
    }
    if (c === "/" && n === "*") {
      blockDepth = 1;
      i += 1;
      continue;
    }
    if (c === opening) depth += 1;
    else if (c === closing && --depth === 0) return i;
  }
  throw new Error(`unbalanced ${opening} at ${start}`);
}

// Keep source offsets and line structure while removing syntax that must not
// participate in registry or witness discovery.
function withoutComments(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") {
        lineComment = false;
        out += c;
      } else {
        out += c === "\r" ? c : " ";
      }
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        out += "  ";
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        out += "  ";
        i += 1;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
      }
      continue;
    }
    if (quote === "triple") {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"' && text.slice(i, i + 3) === '"""') {
        out += '""';
        i += 2;
        quote = null;
      }
      continue;
    }
    if (quote === "regular") {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      out += '"""';
      i += 2;
      quote = "triple";
      continue;
    }
    if (c === '"') {
      out += c;
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      out += "  ";
      i += 1;
      lineComment = true;
      continue;
    }
    if (c === "/" && n === "*") {
      out += "  ";
      i += 1;
      blockDepth = 1;
      continue;
    }
    out += c;
  }
  return out;
}

// The complement of withoutComments: strings are blanked too, so names and
// calls in comments or string literals can never become witness evidence.
function codeOnly(text) {
  let out = "";
  let quote = null;
  let escaped = false;
  let lineComment = false;
  let blockDepth = 0;
  for (let i = 0; i < text.length; i += 1) {
    const c = text[i];
    const n = text[i + 1];
    if (lineComment) {
      if (c === "\n") {
        lineComment = false;
        out += c;
      } else {
        out += c === "\r" ? c : " ";
      }
      continue;
    }
    if (blockDepth > 0) {
      if (c === "/" && n === "*") {
        blockDepth += 1;
        out += "  ";
        i += 1;
      } else if (c === "*" && n === "/") {
        blockDepth -= 1;
        out += "  ";
        i += 1;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
      }
      continue;
    }
    if (quote === "triple") {
      if (c === '"' && text.slice(i, i + 3) === '"""') {
        out += "   ";
        i += 2;
        quote = null;
      } else {
        out += c === "\n" || c === "\r" ? c : " ";
        if (escaped) escaped = false;
        else if (c === "\\") escaped = true;
      }
      continue;
    }
    if (quote === "regular") {
      out += c === "\n" || c === "\r" ? c : " ";
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') quote = null;
      continue;
    }
    if (c === '"' && text.slice(i, i + 3) === '"""') {
      out += "   ";
      i += 2;
      quote = "triple";
      continue;
    }
    if (c === '"') {
      out += " ";
      quote = "regular";
      continue;
    }
    if (c === "/" && n === "/") {
      out += "  ";
      i += 1;
      lineComment = true;
      continue;
    }
    if (c === "/" && n === "*") {
      out += "  ";
      i += 1;
      blockDepth = 1;
      continue;
    }
    out += c;
  }
  return out;
}

function decodeRustString(value) {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value;
  }
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g), (m) => decodeRustString(m[1]));
}

function escapedRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function moduleItems() {
  const source = withoutComments(readFileSync(MODULE_EXPORTS, "utf8"));
  const members = new Map();
  const arrays = /const\s+(CORE_MODULE_\d+_MEMBERS)\s*:\s*&\[&str\]\s*=\s*&\[([^\]]*)\];/g;
  for (const match of source.matchAll(arrays)) {
    const names = quoted(match[2]);
    const values = new Set(names);
    if (values.size !== names.length || members.has(match[1])) {
      throw new Error(`duplicate Core export members in ${match[1]}`);
    }
    members.set(match[1], values);
  }
  const out = new Map();
  const modules = /CoreModuleDeclaration\s*\{\s*module:\s*"([^"]+)",\s*members:\s*(CORE_MODULE_\d+_MEMBERS),/g;
  for (const match of source.matchAll(modules)) {
    const values = members.get(match[2]);
    if (!values) throw new Error(`missing Core export members for ${match[1]}`);
    if (out.has(match[1])) throw new Error(`duplicate Core module ${match[1]}`);
    out.set(match[1], values);
  }
  if (out.size === 0 || out.size !== members.size) {
    throw new Error("Core export declarations do not account for every member table");
  }
  return out;
}

function inventory() {
  const modules = moduleItems();
  const moduleNames = new Set(modules.keys());
  const rows = [];
  for (const [module, names] of modules) {
    for (const name of names) {
      if (/^[A-Z]/.test(name)) continue;
      if (VALUE_NAMES.has(name) || moduleNames.has(`${module}.${name}`)) continue;
      rows.push(`${module}.${name}`);
    }
  }
  return rows.sort();
}

function walk(dir) {
  if (!existsSync(dir)) return [];
  const files = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".")) continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...walk(path));
    else if (entry.isFile() && entry.name.endsWith(".jet") && entry.name !== "package.jet") files.push(path);
  }
  return files.sort();
}

function keyForPath(path) {
  const rel = relative(CORPUS, path).replaceAll("\\", "/");
  const parts = rel.split("/");
  const name = parts.pop().replace(/\.jet$/, "");
  return `${parts.join(".")}.${name}`;
}

function fixtureBindingPaths(bindings) {
  const byPath = new Map();
  for (const [row, paths] of bindings) {
    if (!Array.isArray(paths) || paths.length === 0) {
      throw new Error(`fixture binding row has no paths: ${row}`);
    }
    const seen = new Set();
    for (const path of paths) {
      if (
        typeof path !== "string"
        || path.length === 0
        || path.startsWith("/")
        || path.includes("\\")
        || path.split("/").some((part) => !part || part === "." || part === "..")
      ) {
        throw new Error(`invalid conformance fixture binding path: ${path}`);
      }
      if (seen.has(path)) throw new Error(`duplicate fixture binding path: ${row} -> ${path}`);
      seen.add(path);
      if (!byPath.has(path)) byPath.set(path, []);
      byPath.get(path).push(row);
    }
  }
  for (const rows of byPath.values()) rows.sort();
  return byPath;
}

function parseExclusions(source = existsSync(EXCLUSIONS) ? readFileSync(EXCLUSIONS, "utf8") : "") {
  const rows = new Map();
  for (const [index, raw] of source.split(/\r?\n/).entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const fields = raw.split("\t").map((field) => field.trim());
    if (fields.length !== 4 || fields.some((field) => !field)) {
      throw new Error(
        `malformed conformance carve-out at line ${index + 1}: expected key<TAB>reason<TAB>owner<TAB>decision`,
      );
    }
    const [key, reason] = fields;
    if (rows.has(key)) throw new Error(`duplicate conformance carve-out: ${key}`);
    rows.set(key, reason);
  }
  return rows;
}

function stringInterpolatesValue(text, value) {
  const clean = withoutComments(text);
  let quote = null;
  let escaped = false;
  for (let i = 0; i < clean.length; i += 1) {
    const c = clean[i];
    if (!quote) {
      if (c === '"' && clean.slice(i, i + 3) === '"""') {
        quote = "triple";
        i += 2;
      } else if (c === '"') {
        quote = "regular";
      }
      continue;
    }
    if (escaped) {
      escaped = false;
      continue;
    }
    if (c === "\\") {
      escaped = true;
      continue;
    }
    if (quote === "triple" && c === '"' && clean.slice(i, i + 3) === '"""') {
      quote = null;
      i += 2;
      continue;
    }
    if (quote === "regular" && c === '"') {
      quote = null;
      continue;
    }
    if (c !== "{") continue;
    if (clean[i + 1] === "{") {
      i += 1;
      continue;
    }
    let close;
    try {
      close = matching(clean, i, "{", "}");
    } catch {
      return false;
    }
    if (expressionConsumesValue(clean.slice(i + 1, close), value)) return true;
    i = close;
  }
  return false;
}

function expressionConsumesValue(text, value) {
  const valueUse = new RegExp(
    `(?<![A-Za-z0-9_])${escapedRegExp(value)}(?![A-Za-z0-9_])`,
  );
  return valueUse.test(codeOnly(text)) || stringInterpolatesValue(text, value);
}

function observerCalls(code) {
  const observers = [];
  const pattern = /(?<![A-Za-z0-9_.])(?:print|eprint|assert)\s*\(/g;
  for (const match of code.matchAll(pattern)) {
    const operation = match[0].match(/^(print|eprint|assert)/)[1];
    const open = match.index + match[0].lastIndexOf("(");
    try {
      observers.push({ operation, open, close: matching(code, open, "(", ")") });
    } catch {
      // An unbalanced observer cannot prove result consumption.
    }
  }
  return observers;
}

// A registered result also counts as consumed when it reaches an observer
// through later bindings: `first :: reader.next()` followed by `print(first)`
// reads `reader`. Walking that chain to a fixpoint keeps genuinely unread
// bindings failing while accepting a program that really consumes the value.
function valueReachesObserver(code, source, observers, value, after) {
  const bindings = [];
  const pattern = /(?:^|[{};\n])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*([^\n;]*)/g;
  for (const match of code.matchAll(pattern)) {
    bindings.push({ name: match[1], init: match[2] });
  }
  const reached = new Set([value]);
  for (let pass = 0; pass <= bindings.length; pass += 1) {
    let grew = false;
    for (const held of Array.from(reached)) {
      const seen = observers.some(
        ({ open, close }) => open > after && expressionConsumesValue(source.slice(open + 1, close), held),
      );
      if (seen) return true;
      // Builder values are also consumed by instance methods used as bare
      // statements. This keeps `server.html(...); server.port(...)` honest
      // without treating an unobserved binding as covered.
      const methodUse = new RegExp(
        `(?<![A-Za-z0-9_.])${escapedRegExp(held)}\\s*\\.\\s*[A-Za-z_][A-Za-z0-9_]*\\s*\\(`,
        "g",
      );
      if (Array.from(code.matchAll(methodUse)).some(({ index }) => index > after)) return true;
      if (matchArmObserves(code, observers, held)) return true;
      for (const candidate of bindings) {
        if (reached.has(candidate.name)) continue;
        if (!expressionConsumesValue(candidate.init, held)) continue;
        reached.add(candidate.name);
        grew = true;
      }
    }
    if (!grew) break;
  }
  return false;
}

// Branching on a value and printing from the arms observes it. The arm binders
// are introduced by the pattern, not by a `::` binding, so the chain above
// cannot see them; this reads the match as one consumption site.
function matchArmObserves(code, observers, held) {
  const pattern = /(?<![A-Za-z0-9_.])if\s+([^\n{]*?)==\s*\{/g;
  for (const match of code.matchAll(pattern)) {
    if (!expressionConsumesValue(match[1], held)) continue;
    const open = match.index + match[0].length - 1;
    let close;
    try {
      close = matching(code, open, "{", "}");
    } catch {
      continue;
    }
    if (observers.some(({ open: obs }) => obs > open && obs < close)) return true;
  }
  return false;
}

function sourceErrors(key, source) {
  const errors = [];
  const expectedMarker = `// core-conformance: ${key}`;
  const firstLine = source.split(/\r?\n/, 1)[0];
  if (firstLine !== expectedMarker) {
    const marker = firstLine.match(/^\s*\/\/\s*core-conformance:\s*(.*?)\s*$/);
    if (marker) errors.push(`marker names ${marker[1] || "<empty>"}`);
    else errors.push(`file must start with ${expectedMarker}`);
  }
  const markerCount = source
    .split(/\r?\n/)
    .filter((line) => line.trim() === expectedMarker)
    .length;
  if (markerCount > 1) errors.push(`expected exactly one ${expectedMarker} marker, found ${markerCount}`);

  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const name = key.slice(dot + 1);
  const code = codeOnly(source);
  const unitRun = code.match(/\bfn\s+run\s*\(\s*\)\s*\{/);
  if (unitRun) {
    const opening = unitRun.index + unitRun[0].lastIndexOf("{");
    const closing = matching(code, opening, "{", "}");
    if (/\?\?\s*return\s+Err\s*\(/.test(code.slice(opening + 1, closing))) {
      errors.push("Unit run cannot use ?? return Err(...) propagation");
    }
  }
  const broadPattern = new RegExp(
    `^\\s*use\\s+${escapedRegExp(module)}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)[ \\t]*(?:;[ \\t]*)?\\r?$`,
    "gm",
  );
  const selectivePattern = new RegExp(
    `^\\s*use\\s+${escapedRegExp(module)}\\.\\[\\s*${escapedRegExp(name)}(?:\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*))?\\s*\\][ \\t]*(?:;[ \\t]*)?\\r?$`,
    "gm",
  );
  const broad = Array.from(code.matchAll(broadPattern));
  const selective = Array.from(code.matchAll(selectivePattern));
  if (broad.length + selective.length !== 1) {
    errors.push(`expected one import for ${module}.${name}, found ${broad.length + selective.length}`);
    return errors;
  }
  const alias = broad.length === 1 ? broad[0][1] : selective[0][1] || name;
  // A registry row may name a method whose canonical surface is a generic
  // receiver (`alias.Type<T>.method(...)`) rather than `alias.method(...)`.
  // Match both shapes without hard-coding one type or method.
  const call = broad.length === 1
    ? new RegExp(
      `(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\.\\s*(?:${escapedRegExp(name)}\\s*(?:<[^{}]*>\\s*)?|[A-Za-z_][A-Za-z0-9_]*\\s*<[^{}]*>\\s*\\.\\s*${escapedRegExp(name)}\\s*)\\(`,
      "g",
    )
    : new RegExp(`(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\(`, "g");
  const calls = Array.from(code.matchAll(call));

  const callStart = calls[0].index;
  const callOpen = callStart + calls[0][0].lastIndexOf("(");
  let callClose;
  try {
    callClose = matching(code, callOpen, "(", ")");
  } catch {
    errors.push(`malformed ${module}.${name} call: unbalanced parentheses`);
    return errors;
  }

  const lineStart = code.lastIndexOf("\n", callStart) + 1;
  const beforeCall = code.slice(lineStart, callStart);
  const binding = beforeCall.match(/(?:^|[{};])\s*(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*$/);
  const observers = observerCalls(code);
  if (binding) {
    const value = binding[1];
    if (value.startsWith("_")) errors.push(`result is bound to discard name ${value}`);
    if (!valueReachesObserver(code, source, observers, value, callClose)) {
      errors.push(`bound result ${value} is never consumed by print/eprint/assert`);
    }
  } else {
    const directObserver = observers.some(({ open, close }) => open < callStart && callStart < close);
    const lineEnd = code.indexOf("\n", callClose);
    const tail = code.slice(callClose + 1, lineEnd < 0 ? code.length : lineEnd);
    const propagated = /\?\?\s*panic\s*\(/.test(tail);
    const observesSuccess = observers.some(({ open }) => open > callClose);
    // Unit-returning rows are consumed by the effect they perform. A later
    // observer in the same witness records that effect without manufacturing
    // a value or wrapping the call in a lambda.
    const observesUnitEffect = UNIT_RESULT_KEYS.has(key) && observesSuccess;
    // Never-returning effects terminate before any later observer can run;
    // their standalone call is the witness's complete observation.
    const observesNeverEffect = NEVER_RESULT_KEYS.has(key);
    if (
      !directObserver
      && !(propagated && observesSuccess)
      && !observesUnitEffect
      && !observesNeverEffect
    ) {
      errors.push("direct result is not consumed by print/eprint/assert or explicit error propagation");
    }
  }
  return errors;
}
function auditEntries(expected, witnesses, exclusions) {
  const expectedRows = Array.from(expected).sort();
  const expectedSet = new Set(expectedRows);
  const errors = [];
  for (let i = 1; i < expectedRows.length; i += 1) {
    if (expectedRows[i] === expectedRows[i - 1]) {
      errors.push(`${expectedRows[i]}: denominator contains a duplicate row`);
    }
  }
  const files = new Map();
  for (const witness of witnesses) {
    const { key, path, source } = witness;
    if (!expectedSet.has(key)) {
      errors.push(`${key}: file is not a public Core function`);
      continue;
    }
    if (files.has(key)) {
      errors.push(`${key}: duplicate files ${files.get(key)} and ${path}`);
      continue;
    }
    files.set(key, path);
    errors.push(...sourceErrors(key, source).map((error) => `${key}: ${error}`));
  }
  for (const [key, reason] of Array.from(exclusions).sort(([left], [right]) => (
    left < right ? -1 : left > right ? 1 : 0
  ))) {
    if (!expectedSet.has(key)) errors.push(`${key}: carve-out is not a public Core function`);
    if (!reason) errors.push(`${key}: carve-out has no reason`);
    if (files.has(key)) errors.push(`${key}: has both a program and a carve-out`);
  }
  const missing = expectedRows.filter((key) => !files.has(key) && !exclusions.has(key));
  return {
    errors: errors.sort(),
    exclusions,
    files,
    missing,
  };
}

function audit() {
  const expected = inventory();
  const bindingPaths = fixtureBindingPaths(FIXTURE_BINDINGS);
  const discovered = walk(CORPUS);
  const mappedPaths = new Set(
    Array.from(bindingPaths.keys(), (path) => join(CORPUS, path)),
  );
  const witnesses = discovered
    .filter((path) => !mappedPaths.has(path))
    .map((path) => ({
      key: keyForPath(path),
      path,
      source: readFileSync(path, "utf8"),
    }));
  const result = auditEntries(expected, witnesses, parseExclusions());
  const bindingErrors = [];
  for (const [relativePath] of bindingPaths) {
    const path = join(CORPUS, relativePath);
    if (!discovered.includes(path)) {
      bindingErrors.push(`fixture binding path does not name a discovered witness: ${relativePath}`);
      continue;
    }
    const key = keyForPath(path);
    if (expected.includes(key)) {
      bindingErrors.push(`fixture binding path collides with denominator row: ${relativePath}`);
      continue;
    }
    // Supplemental semantic witnesses are counted by the strict corpus gate,
    // but do not become denominator rows or pass through sourceErrors' module
    // call-shape contract. Their canonical path-key marker remains in each
    // source file; the map is the explicit binding contract for this shape.
    result.files.set(`fixture:${relativePath}`, path);
  }
  const { exclusions, files, missing } = result;
  const errors = [...result.errors, ...bindingErrors].sort();
  console.log(`core conformance denominator: ${expected.length} public function(s); ${files.size} program(s); ${exclusions.size} carve-out(s); ${missing.length} uncovered row(s)`);
  for (const key of missing) console.log(`  ${key}`);
  for (const error of errors) console.error(`error: ${error}`);
  return missing.length || errors.length ? 1 : 0;
}

function fixtureBindingsJson() {
  fixtureBindingPaths(FIXTURE_BINDINGS);
  return Object.fromEntries(
    Array.from(FIXTURE_BINDINGS.entries())
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([row, paths]) => [row, [...paths].sort()]),
  );
}

function unitObserver(expression, indent) {
  const body = expression
    .trim()
    .split(/\r?\n/)
    .map((line) => `${indent}    ${line.trim()}`)
    .join("\n");
  return `(() -> {\n${body}\n${indent}    true\n${indent}})()`;
}

function lineRange(source, index) {
  const start = source.lastIndexOf("\n", index - 1) + 1;
  const endAt = source.indexOf("\n", index);
  return { start, end: endAt < 0 ? source.length : endAt };
}

function canonicalizeUnitWitness(key, source) {
  if (!UNIT_RESULT_KEYS.has(key)) return source;
  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  const name = key.slice(dot + 1);
  const code = codeOnly(source);
  const usePattern = new RegExp(
    `^\\s*use\\s+${escapedRegExp(module)}\\s+as\\s+([A-Za-z_][A-Za-z0-9_]*)[ \\t]*(?:;[ \\t]*)?\\r?$`,
    "gm",
  );
  const aliases = Array.from(code.matchAll(usePattern));
  if (aliases.length !== 1) return source;
  const alias = aliases[0][1];
  const call = new RegExp(
    `(?<![A-Za-z0-9_.])${escapedRegExp(alias)}\\s*\\.\\s*${escapedRegExp(name)}\\s*(?:<[^{}]*>\\s*)?\\(`,
    "g",
  );
  const calls = Array.from(code.matchAll(call));
  if (calls.length !== 1) return source;
  const callStart = calls[0].index;
  const callOpen = callStart + calls[0][0].lastIndexOf("(");
  let callClose;
  try {
    callClose = matching(code, callOpen, "(", ")");
  } catch {
    return source;
  }
  const observers = observerCalls(code);
  const directObserver = observers.find(({ open, close }) => open < callStart && callStart < close);
  if (directObserver) {
    const argument = source.slice(directObserver.open + 1, directObserver.close);
    const argumentCode = code.slice(directObserver.open + 1, directObserver.close).trim();
    if (argumentCode.startsWith("(() ->")) {
      const staleNames = ["result", "changed"].filter(
        (value) => !new RegExp(`(?:^|[{};\\n])\\s*${escapedRegExp(value)}\\s*(?:::|:=)`).test(code),
      );
      if (staleNames.length === 0) return source;
      const staleLine = new RegExp(
        `^[ \\t]*print\\((${staleNames.map(escapedRegExp).join("|")})\\)[ \\t]*(?:\\r?\\n|$)`,
        "gm",
      );
      const tailStart = directObserver.close + 1;
      const tail = source.slice(tailStart).replace(staleLine, "");
      return `${source.slice(0, tailStart)}${tail}`;
    }
    const targetCode = code.slice(callStart, callClose + 1).trim();
    if (!argumentCode.startsWith(targetCode)) return source;
    const suffix = argumentCode.slice(targetCode.length).trim();
    const suffixIsPropagation = /^\?\?\s*panic\s*\(/.test(suffix);
    const suffixIsAdditionalArgument = /^,/.test(suffix);
    if (suffix && !suffixIsPropagation && !suffixIsAdditionalArgument) return source;
    const { start } = lineRange(source, directObserver.open);
    const indent = source.slice(start).match(/^\s*/)[0];
    const expression = suffixIsAdditionalArgument
      ? source.slice(callStart, callClose + 1)
      : argument;
    const marker = unitObserver(expression, indent);
    const rewrittenArgument = suffixIsAdditionalArgument
      ? `${marker}${argument.slice(argument.indexOf(source.slice(callClose + 1, directObserver.close)))}`
      : marker;
    return `${source.slice(0, directObserver.open + 1)}${rewrittenArgument}${source.slice(directObserver.close)}`;
  }

  const { start: callLineStart, end: callLineEnd } = lineRange(source, callStart);
  const callLine = source.slice(callLineStart, callLineEnd);
  const binding = callLine.match(/^(\s*)(@?[A-Za-z_][A-Za-z0-9_]*)\s*(?:::|:=)\s*(.*?)\s*$/);
  if (!binding) return source;
  const expression = binding[3].trim();
  const expressionCode = code.slice(callLineStart, callLineEnd).match(
    /^\s*@?[A-Za-z_][A-Za-z0-9_]*\s*(?:::|:=)\s*(.*?)\s*$/,
  )?.[1]?.trim();
  const targetCode = code.slice(callStart, callClose + 1).trim();
  if (!expressionCode || !expressionCode.startsWith(targetCode)) return source;
  const suffix = expressionCode.slice(targetCode.length).trim();
  if (suffix && !/^\?\?\s*panic\s*\(/.test(suffix)) return source;
  const value = binding[2];
  const observer = observers.find(
    ({ open, close }) =>
      open > callClose &&
      new RegExp(`(?<![A-Za-z0-9_])${escapedRegExp(value)}(?![A-Za-z0-9_])`).test(
        code.slice(open + 1, close),
      ),
  );
  if (!observer) return source;
  const observerArgument = source.slice(observer.open + 1, observer.close).trim();
  const valuePattern = new RegExp(`^${escapedRegExp(value)}(?:\\s*,\\s*(.*))?$`);
  const observerMatch = observerArgument.match(valuePattern);
  if (!observerMatch) return source;
  const indent = binding[1];
  const marker = unitObserver(expression, indent);
  const bindingReplacement = `${indent}print(${marker})`;
  const observerLine = lineRange(source, observer.open);
  const observerLineText = source.slice(observerLine.start, observerLine.end);
  const observerCallText = observerLineText.trim();
  const observerText = `${observer.operation}(${observerArgument})`;
  const removeObserverLine = observerCallText === observerText;
  const edits = [
    { start: callLineStart, end: callLineEnd, text: bindingReplacement },
  ];
  if (removeObserverLine) {
    edits.push({
      start: observerLine.start,
      end: observerLine.end < source.length ? observerLine.end + 1 : observerLine.end,
      text: "",
    });
  } else if (observerMatch[1]) {
    edits.push({
      start: observer.open + 1,
      end: observer.close,
      text: observerMatch[1],
    });
  }
  edits.sort((left, right) => right.start - left.start);
  let rewritten = source;
  for (const edit of edits) rewritten = `${rewritten.slice(0, edit.start)}${edit.text}${rewritten.slice(edit.end)}`;
  return rewritten;
}

function normalizeUnitObservers(expected) {
  for (const key of UNIT_RESULT_KEYS) {
    if (!expected.has(key)) throw new Error(`Unit observer key is not public Core: ${key}`);
  }
  let normalized = 0;
  for (const path of walk(CORPUS)) {
    const key = keyForPath(path);
    if (!UNIT_RESULT_KEYS.has(key)) continue;
    const source = readFileSync(path, "utf8");
    const rewritten = canonicalizeUnitWitness(key, source);
    if (rewritten === source) continue;
    writeFileSync(path, rewritten);
    normalized += 1;
  }
  return normalized;
}

function coreSourceFiles() {
  const sourceRoot = join(ROOT, "Core");
  const files = [];
  const visit = (directory) => {
    if (!existsSync(directory)) return;
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const path = join(directory, entry.name);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && entry.name.endsWith(".jet")) files.push(path);
    }
  };
  visit(sourceRoot);
  return files.sort();
}

function sourceModule(path) {
  const parts = relative(join(ROOT, "Core"), path)
    .replaceAll("\\", "/")
    .replace(/\.jet$/, "")
    .split("/");
  if (parts.length > 1 && parts.at(-1) === parts.at(-2)) parts.pop();
  return parts[0] === "app" ? parts.join(".") : `core.${parts.join(".")}`;
}

function sourceSignatures() {
  const signatures = new Map();
  for (const path of coreSourceFiles()) {
    const source = readFileSync(path, "utf8");
    const code = withoutComments(source);
    const pattern = /^pub\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/gm;
    for (const match of code.matchAll(pattern)) {
      const open = match.index + match[0].lastIndexOf("(");
      let close;
      try {
        close = matching(code, open, "(", ")");
      } catch {
        continue;
      }
      const key = `${sourceModule(path)}.${match[1]}`;
      if (!signatures.has(key)) signatures.set(key, code.slice(open + 1, close));
    }
  }
  return signatures;
}

function splitParameters(source) {
  const parts = [];
  let start = 0;
  let parens = 0;
  let brackets = 0;
  let braces = 0;
  let quote = null;
  let escaped = false;
  for (let i = 0; i < source.length; i += 1) {
    const c = source[i];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'") {
      quote = c;
      continue;
    }
    if (c === "(") parens += 1;
    else if (c === ")") parens -= 1;
    else if (c === "[") brackets += 1;
    else if (c === "]") brackets -= 1;
    else if (c === "{") braces += 1;
    else if (c === "}") braces -= 1;
    else if (c === "," && parens === 0 && brackets === 0 && braces === 0) {
      parts.push(source.slice(start, i).trim());
      start = i + 1;
    }
  }
  const tail = source.slice(start).trim();
  if (tail) parts.push(tail);
  return parts;
}

function parameterType(parameter) {
  let depth = 0;
  let quote = null;
  let escaped = false;
  for (let i = 0; i < parameter.length; i += 1) {
    const c = parameter[i];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'") {
      quote = c;
      continue;
    }
    if (c === "<" || c === "(" || c === "[" || c === "{") depth += 1;
    else if (c === ">" || c === ")" || c === "]" || c === "}") depth -= 1;
    else if (c === ":" && depth === 0) {
      const type = parameter.slice(i + 1).trim();
      return type.replace(/\s*=\s*.*$/, "").trim();
    }
  }
  return null;
}

function signatureArgument(type) {
  if (!type) return "0";
  let normalized = type
    .replace(/^mut\s+/, "")
    .replace(/^&+/, "")
    .replace(/^\^+/, "")
    .replace(/\?$/, "")
    .trim();
  if (normalized.includes("->") || normalized.startsWith("fn")) return "() -> { true }";
  if (normalized.startsWith("Option<")) return "None";
  if (normalized.startsWith("Result<")) return "Ok(0)";
  if (normalized === "String" || normalized === "Str") return '"jet"';
  if (normalized === "Bool") return "true";
  if (normalized === "Float" || normalized === "F32" || normalized === "F64") return "1.0";
  if (normalized === "U8") return "U8{1}";
  if (/^(U16|U32|U64|U128|I8|I16|I32|I64|I128|Int)$/.test(normalized)) return "1";
  if (normalized === "Unit") return "()";
  if (normalized.startsWith("[U8")) return "[U8]{106, 101, 116}";
  if (normalized.startsWith("[String")) return '[String]{"jet"}';
  if (normalized.startsWith("[Float")) return "[Float]{1.0, 2.0}";
  if (normalized.startsWith("[Bool")) return "[Bool]{true}";
  if (normalized.startsWith("[Int")) return "[Int]{1, 2, 3}";
  if (normalized.startsWith("[")) return `${normalized}{}`;
  if (normalized === "Duration") return "Duration{seconds: 0, nanoseconds: 0}";
  if (normalized === "Path") return '"."';
  if (normalized === "Regex") return 'regex.compile(".*")';
  if (/^[A-Z][A-Za-z0-9_]*(<.*>)?$/.test(normalized)) return `${normalized}{}`;
  return "0";
}

const GENERATED_SIGNATURE_OVERRIDES = new Map([
  ["core.data.stream.take", "stream: Stream, n: Int"],
  ["core.encoding.hex.a2b_base64", "data: [U8]"],
  ["core.encoding.hex.b2a_base64", "data: [U8]"],
  ["core.files.chown", "path: String, uid: Int, gid: Int"],
  ["core.files.is_fifo", "path: String"],
  ["core.files.is_socket", "path: String"],
  ["core.files.lstat", "path: String"],
  ["core.files.mkdtemp", "prefix: String"],
  ["core.files.mktemp", "prefix: String"],
  ["core.math.combinatorics.take", "items: [Int], n: Int"],
  ["core.net.gethostname", ""],
  ["core.units.byte_unit", ""],
  ["core.units.centi", ""],
  ["core.units.gibibyte", ""],
  ["core.units.giga", ""],
  ["core.units.gram", ""],
  ["core.units.kibibyte", ""],
  ["core.units.kilo", ""],
  ["core.units.mebibyte", ""],
  ["core.units.mega", ""],
  ["core.units.meter", ""],
  ["core.units.micro", ""],
  ["core.units.milli", ""],
  ["core.units.nano", ""],
  ["core.units.second", ""],
]);

function nominalWitness(key) {
  const dot = key.lastIndexOf(".");
  const module = key.slice(0, dot);
  if (module !== "core.collections" && module !== "core.collections.set") return null;
  const name = key.slice(dot + 1);
  if (module === "core.collections.set") {
    const setBodies = {
      new: `s :: api.new()
print(api.len(s))`,
      from_list: `s :: api.from_list([String]{"jet", "jet"})
print(api.len(s))`,
      add: `s :: api.add(api.new(), "jet")
print(api.len(s))`,
      discard: `s :: api.discard(api.from_list([String]{"jet"}), "jet")
print(api.len(s))`,
      remove: `s :: api.remove(api.from_list([String]{"jet"}), "jet") ?? api.new()
print(api.len(s))`,
      contains: `s :: api.from_list([String]{"jet"})
print(api.contains(s, "jet"))`,
      len: `s :: api.from_list([String]{"jet"})
print(api.len(s))`,
      is_empty: `print(api.is_empty(api.new()))`,
      to_list: `s :: api.from_list([String]{"jet"})
print(api.to_list(s))`,
      clear: `s :: api.clear(api.from_list([String]{"jet"}))
print(api.len(s))`,
      union: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"lang"})
print(api.len(api.union(a, b)))`,
      intersection: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"lang"})
print(api.len(api.intersection(a, b)))`,
      difference: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"lang"})
print(api.len(api.difference(a, b)))`,
      symmetric_difference: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"lang"})
print(api.len(api.symmetric_difference(a, b)))`,
      issubset: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"jet", "lang"})
print(api.issubset(a, b))`,
      issuperset: `a :: api.from_list([String]{"jet", "lang"})
b :: api.from_list([String]{"jet"})
print(api.issuperset(a, b))`,
      isdisjoint: `a :: api.from_list([String]{"jet"})
b :: api.from_list([String]{"lang"})
print(api.isdisjoint(a, b))`,
      clone_set: `s :: api.clone_set(api.from_list([String]{"jet"}))
print(api.len(s))`,
    };
    const body = setBodies[name];
    if (body === undefined) return null;
    const indented = body.split("\n").map((line) => `    ${line}`).join("\n");
    return `// core-conformance: ${key}
use core.collections.set as api

fn run() {
${indented}
}
`;
  }
  const bodies = {
    counter: `c :: api.counter()
print(api.total(c))`,
    counter_from: `c :: api.counter_from([String]{"jet"})
print(api.total(c))`,
    add: `c :: api.counter()
out :: api.add(c, "jet", 1)
print(api.total(out))`,
    inc: `c :: api.counter()
out :: api.inc(c, "jet")
print(api.total(out))`,
    dec: `c :: api.counter()
out :: api.dec(c, "jet")
print(api.total(out))`,
    get: `c :: api.counter_from([String]{"jet"})
print(api.get(c, "jet"))`,
    set_count: `c :: api.counter()
out :: api.set_count(c, "jet", 2)
print(api.total(out))`,
    total: `c :: api.counter_from([String]{"jet"})
print(api.total(c))`,
    names: `c :: api.counter_from([String]{"jet"})
print(api.names(c))`,
    elements: `c :: api.counter_from([String]{"jet"})
print(api.elements(c))`,
    most_common: `c :: api.counter_from([String]{"jet"})
out :: api.most_common(c, 1)
print(api.total(out))`,
    subtract: `c :: api.counter_from([String]{"jet"})
out :: api.subtract(c, c)
print(api.total(out))`,
    merge_add: `c :: api.counter_from([String]{"jet"})
out :: api.merge_add(c, c)
print(api.total(out))`,
    clear_counter: `c :: api.counter_from([String]{"jet"})
out :: api.clear_counter(c)
print(api.total(out))`,
    deque: `d :: api.deque()
print(api.deque_len(d))`,
    deque_from: `d :: api.deque_from([String]{"jet"})
print(api.deque_len(d))`,
    deque_len: `d :: api.deque()
print(api.deque_len(d))`,
    deque_is_empty: `d :: api.deque()
print(api.deque_is_empty(d))`,
    append: `d :: api.deque()
out :: api.append(d, "jet")
print(api.deque_len(out))`,
    appendleft: `d :: api.deque()
out :: api.appendleft(d, "jet")
print(api.deque_len(out))`,
    pop: `d :: api.deque()
pair :: api.pop(d)
print(api.deque_len(pair.deque))`,
    popleft: `d :: api.deque()
pair :: api.popleft(d)
print(api.deque_len(pair.deque))`,
    peek: `print(api.peek(api.deque()) == None)`,
    peekleft: `print(api.peekleft(api.deque()) == None)`,
    extend: `d :: api.deque()
out :: api.extend(d, [String]{"jet"})
print(api.deque_len(out))`,
    extendleft: `d :: api.deque()
out :: api.extendleft(d, [String]{"jet"})
print(api.deque_len(out))`,
    rotate: `d :: api.deque_from([String]{"jet"})
out :: api.rotate(d, 1)
print(api.deque_len(out))`,
    deque_items: `d :: api.deque_from([String]{"jet"})
print(api.deque_items(d))`,
    ordered_map: `m :: api.ordered_map()
print(api.map_len(m))`,
    map_get: `m :: api.ordered_map()
print(api.map_get(m, "jet") == None)`,
    map_set: `m :: api.ordered_map()
out :: api.map_set(m, "jet", "value")
print(api.map_len(out))`,
    map_remove: `m :: api.ordered_map()
out :: api.map_remove(m, "jet")
print(api.map_len(out))`,
    map_keys: `m :: api.ordered_map()
print(api.map_keys(m))`,
    map_values: `m :: api.ordered_map()
print(api.map_values(m))`,
    map_contains: `m :: api.ordered_map()
print(api.map_contains(m, "jet"))`,
    map_len: `m :: api.ordered_map()
print(api.map_len(m))`,
    chain: `c :: api.chain()
print(api.chain_contains(c, "jet"))`,
    chain_push: `c :: api.chain()
out :: api.chain_push(c, [String]{"jet"}, [String]{"value"})
print(api.chain_contains(out, "jet"))`,
    chain_get: `c :: api.chain()
print(api.chain_get(c, "jet") == None)`,
    chain_contains: `c :: api.chain()
print(api.chain_contains(c, "jet"))`,
  };
  const body = bodies[name];
  if (body === undefined) return null;
  const indented = body.split("\n").map((line) => `    ${line}`).join("\n");
  return `// core-conformance: ${key}
use core.collections as api

fn run() {
${indented}
}
`;
}

function signatureDerivedRecipes(expected, exclusions) {
  const signatures = sourceSignatures();
  const generated = new Map();
  for (const key of expected) {
    if (RECIPES.has(key) || exclusions.has(key)) continue;
    const nominal = nominalWitness(key);
    if (nominal !== null) {
      generated.set(key, nominal);
      continue;
    }
    const signature = signatures.get(key) ?? GENERATED_SIGNATURE_OVERRIDES.get(key);
    if (signature === undefined) continue;
    const dot = key.lastIndexOf(".");
    const module = key.slice(0, dot);
    const name = key.slice(dot + 1);
    const args = splitParameters(signature)
      .map(parameterType)
      .filter((type) => type !== null)
      .map(signatureArgument)
      .join(", ");
    generated.set(key, `// core-conformance: ${key}
use ${module} as api

fn run() {
    print(api.${name}(${args}))
}
`);
  }
  return generated;
}

function allRecipes(expected, exclusions) {
  return new Map([...RECIPES, ...signatureDerivedRecipes(expected, exclusions)]);
}

function generate() {
  const expected = new Set(inventory());
  const exclusions = parseExclusions();
  for (const key of UNIT_RESULT_KEYS) {
    if (!expected.has(key)) throw new Error(`Unit observer key is not public Core: ${key}`);
  }
  const recipes = allRecipes(expected, exclusions);
  let generated = 0;

  let derived = 0;
  for (const [key, source] of recipes) {
    if (!expected.has(key)) throw new Error(`recipe names non-public Core function: ${key}`);
    const dot = key.lastIndexOf(".");
    const module = key.slice(0, dot);
    const name = key.slice(dot + 1);
    const path = join(CORPUS, key.slice(0, dot).replaceAll(".", "/"), `${name}.jet`);
    if (existsSync(path)) {
      const existing = readFileSync(path, "utf8");
      const oldDerivedPrefix = `${key}
use ${module} as api

fn run() {
    print(api.${name}(`;
      const isNominal = nominalWitness(key) !== null;
      const legacySelectivePrefix = `${key}
use ${module}.[${name}]

fn run() {
    print(${name}(`;
      const isLegacyGenerated = existing.startsWith(`// core-conformance: ${oldDerivedPrefix}`)
        || existing.startsWith(`// core-conformance: ${legacySelectivePrefix}`);
      if (RECIPES.has(key) || (isNominal ? existing === source : !isLegacyGenerated)) {
        continue;
      }
    }
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, source);
    generated += 1;
    if (!RECIPES.has(key)) derived += 1;
  }
  const normalized = normalizeUnitObservers(expected);
  console.log(
    `core conformance generator: emitted ${generated} witness program(s) from ${RECIPES.size} explicit and ${derived} signature-derived recipe(s); normalized ${normalized} Unit observer(s)`,
  );
  return audit();
}

function hostileFixtures() {
  const walkFixture = mkdtempSync(join(ROOT, "core-conformance-hostile-"));
  try {
    const visible = join(walkFixture, "visible", "ordinary.jet");
    const manifest = join(walkFixture, "visible", "package.jet");
    const hidden = join(walkFixture, "visible", ".jet", "receipts", ".package.jet");
    const hiddenFile = join(walkFixture, "visible", ".ghost.jet");
    mkdirSync(dirname(visible), { recursive: true });
    mkdirSync(dirname(hidden), { recursive: true });
    writeFileSync(visible, "");
    writeFileSync(manifest, "manifest");
    writeFileSync(hidden, "");
    writeFileSync(hiddenFile, "");
    const discovered = walk(walkFixture);
    if (discovered.length !== 1 || discovered[0] !== visible) {
      throw new Error("walk included hidden file/receipt or visible package manifest as a witness");
    }
    const ordinaryErrors = sourceErrors("core.fake.ordinary", "");
    if (!ordinaryErrors.includes("file must start with // core-conformance: core.fake.ordinary")) {
      throw new Error("ordinary witness without a marker was accepted");
    }
  } finally {
    rmSync(walkFixture, { recursive: true, force: true });
  }

  const key = "core.crypto.uuid.v4";
  const valid = `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print(result)
}
`;
  const cases = [
    [
      "bind-and-discard",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    _result :: uuid.v4()
    print("ok")
}
`,
      "discard name",
    ],
    [
      "observerless binding",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    sink(result)
}
`,
      "never consumed",
    ],
    [
      "direct unobserved call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4()
}
`,
      "direct result is not consumed",
    ],
    [
      "comment call ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    // uuid.v4()
    print("ok")
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "string call ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print("uuid.v4()")
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "comment observer ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    /* print(result) */
}
`,
      "never consumed",
    ],
    [
      "string observer ghost",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4()
    print("result")
}
`,
      "never consumed",
    ],
    [
      "malformed marker",
      `// core-conformance: ${key} extra
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4())
}
`,
      "marker names",
    ],
    [
      "malformed call shape",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4)
}
`,
      "expected one core.crypto.uuid.v4 call, found 0",
    ],
    [
      "unbalanced call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4(}
}
`,
      "malformed core.crypto.uuid.v4 call",
    ],
    [
      "duplicate call",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    print(uuid.v4())
    print(uuid.v4())
}
`,
      "expected one core.crypto.uuid.v4 call, found 2",
    ],
    [
      "direct Unit return propagation",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4() ?? return Err("uuid")
    print("ok")
}
`,
      "Unit run cannot use ?? return Err(...) propagation",
    ],
    [
      "bound Unit return propagation",
      `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    result :: uuid.v4() ?? return Err("uuid")
    print(result)
}
`,
      "Unit run cannot use ?? return Err(...) propagation",
    ],
  ];
  for (const [label, source, expected] of cases) {
    const errors = sourceErrors(key, source);
    if (!errors.some((error) => error.includes(expected))) {
      throw new Error(`${label} fixture was accepted: ${errors.join("; ")}`);
    }
  }

  const directPanic = `// core-conformance: ${key}
use core.crypto.uuid as uuid
fn run() {
    uuid.v4() ?? panic("uuid")
    print("ok")
}
`;
  if (sourceErrors(key, directPanic).length !== 0) {
    throw new Error("direct panic propagation fixture was rejected");
  }

  const interpolated = valid.replace("print(result)", 'print("value={result}")');
  if (sourceErrors(key, interpolated).length !== 0) {
    throw new Error("interpolated observer fixture was rejected");
  }

  const comptime = valid.replace("result ::", "@result ::").replace("print(result)", "print(@result)");
  if (sourceErrors(key, comptime).length !== 0) {
    throw new Error("comptime binding fixture was rejected");
  }

  const generic = `// core-conformance: core.data.csv
use core.data as data
fn run() {
    result :: data.csv<Ticket>("rows")
    print(result)
}
`;
  if (sourceErrors("core.data.csv", generic).length !== 0) {
    throw new Error("generic call fixture was rejected");
  }

  const parsed = parseExclusions("core.fake.one\tplain reason\towner\tD-TEST\n");
  if (parsed.get("core.fake.one") !== "plain reason") {
    throw new Error("owner-ratified exclusion did not return its reason");
  }
  const malformedExclusion = (label, source) => {
    try {
      parseExclusions(source);
    } catch (error) {
      if (error.message.includes("expected key<TAB>reason<TAB>owner<TAB>decision")) return;
      throw error;
    }
    throw new Error(`${label} exclusion was accepted`);
  };
  malformedExclusion("reasonless", "core.fake.one\t\towner\tD-TEST\n");
  malformedExclusion("ownerless", "core.fake.one\tplain reason\t\tD-TEST\n");
  malformedExclusion("decisionless", "core.fake.one\tplain reason\towner\t\n");
  malformedExclusion("extra-field", "core.fake.one\tplain reason\towner\tD-TEST\textra\n");

  const expectedRows = ["core.fake.one", "core.fake.two"];
  const witness = (row, path) => ({ key: row, path, source: `// core-conformance: ${row}
use core.fake as fake
fn run() {
    print(fake.${row.slice(row.lastIndexOf(".") + 1)}())
}
` });
  const assertLedgerError = (label, result, expected) => {
    if (!result.errors.some((error) => error.includes(expected))) {
      throw new Error(`${label} ledger fixture was accepted: ${result.errors.join("; ")}`);
    }
  };
  const missing = auditEntries(expectedRows, [witness("core.fake.one", "core/fake/one.jet")], new Map());
  if (!missing.missing.includes("core.fake.two")) throw new Error("missing ledger row was accepted");
  const duplicateDenominator = auditEntries(["core.fake.one", "core.fake.one"], [], new Map());
  assertLedgerError("duplicate-denominator", duplicateDenominator, "denominator contains a duplicate row");
  const duplicate = auditEntries(
    expectedRows,
    [witness("core.fake.one", "core/fake/one.jet"), witness("core.fake.one", "other/one.jet")],
    new Map(),
  );
  assertLedgerError("duplicate", duplicate, "duplicate files");
  const nonPublic = auditEntries(
    expectedRows,
    [witness("core.fake.ghost", "core/fake/ghost.jet")],
    new Map(),
  );
  assertLedgerError("non-public", nonPublic, "not a public Core function");
  const reasonless = auditEntries(expectedRows, [], new Map([["core.fake.one", ""]]))
  assertLedgerError("reasonless", reasonless, "carve-out has no reason");
  const both = auditEntries(
    expectedRows,
    [witness("core.fake.one", "core/fake/one.jet")],
    new Map([["core.fake.one", "owner-approved test"]]),
  );
  assertLedgerError("witness-plus-exclusion", both, "both a program and a carve-out");

  console.log("core conformance hostile fixtures: rejected hidden/package witnesses, unmarked ordinary files, missing exclusion metadata, bind-and-discard result, observerless/direct calls, comment/string ghosts, malformed marker/calls, and denominator rows");
  return 0;
}

export { FIXTURE_BINDINGS };

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const command = process.argv[2] || "--check";
  try {
    if (command === "--generate") process.exitCode = generate();
    else if (command === "--check") process.exitCode = audit();
    else if (command === "--hostile-fixtures") process.exitCode = hostileFixtures();
    else if (command === "--inventory") {
      console.log(JSON.stringify(inventory(), null, 2));
    } else if (command === "--fixture-bindings") {
      console.log(JSON.stringify(fixtureBindingsJson(), null, 2));
    } else {
      console.error(`usage: ${process.argv[1]} --generate|--check|--hostile-fixtures|--inventory|--fixture-bindings`);
      process.exitCode = 2;
    }
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exitCode = 2;
  }
}
