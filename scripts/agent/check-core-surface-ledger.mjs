#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * One source-derived Core inventory, compared against every language Jet
 * competes with. Read by the #1398 release gate.
 *
 * The compiler tables are authoritative for what Jet ships. Each recorded
 * competitor surface is authoritative for what that language ships. This file
 * holds only the comparison policy and the parser for those tables; the JSON
 * and Markdown artifacts are generated, and hand-editing either is rejected
 * by --check.
 *
 * The ledger is a report. It records what is true today. It does not track
 * work, and coverage is a number it prints rather than a gate.
 */

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const LEDGER_PATH = join(ROOT, "docs/reference/core-surface-ledger.json");
const README_PATH = join(ROOT, "docs/reference/core-surface-ledger.md");
const PYTHON_SURFACE_PATH = join(ROOT, "docs/reference/python-surface.json");
const TOWER_PATH = join(ROOT, "plugins/tower/.tower/tower.json");
const MODULE_ITEMS_PATH = "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs";
const FIXED_SIGS_PATH = "crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs";
const COLLECTIONS_PATH = "crates/jet-foundation/src/Collections.rs";
const PREDICATES_PATH = "crates/jet-foundation/src/Syntax/predicates.rs";
const POLICY_PATH = "crates/jet-foundation/src/Policy.rs";
const SYNTAX_PATH = "crates/jet-foundation/src/Syntax/core_surface.rs";

// Every language the owner named on 2026-08-03, each with a recorded surface
// read from that language's own primary source. Python keeps its interpreter
// snapshot; the other ten are read from a runtime, from standard-library
// source, or from official machine-readable documentation.
const SURFACE_FILES = {
  Rust: "docs/reference/surfaces/rust-surface.json",
  Go: "docs/reference/surfaces/go-surface.json",
  Swift: "docs/reference/surfaces/swift-surface.json",
  Kotlin: "docs/reference/surfaces/kotlin-surface.json",
  "C#": "docs/reference/surfaces/csharp-surface.json",
  TypeScript: "docs/reference/surfaces/js-surface.json",
  Ruby: "docs/reference/surfaces/ruby-surface.json",
  Elixir: "docs/reference/surfaces/elixir-surface.json",
  Julia: "docs/reference/surfaces/julia-surface.json",
  R: "docs/reference/surfaces/r-surface.json",
};

// Python's snapshot predates the container shape, so it is projected onto the
// shared containers rather than re-read. The map runs source to container, not
// container to source: the earlier shape let one module feed three containers,
// so every os function minted three separate gap rows.
const PYTHON_SOURCE_CONTAINER = {
  "type:list": "List",
  "type:tuple": "List",
  "type:range": "Iter",
  "type:dict": "Map",
  "type:set": "Set",
  "type:str": "String",
  "type:bytes": "ByteBuffer",
  "type:int": "core.math",
  "type:float": "core.math",
  "mod:builtins": "core.io",
  "mod:itertools": "Iter",
  "mod:collections": "Deque",
  "mod:heapq": "PriorityQueue",
  "mod:functools": "Cache",
  "mod:io": "ByteBuffer",
  "mod:string": "String",
  "mod:textwrap": "String",
  "mod:math": "core.math",
  "mod:decimal": "core.math",
  "mod:fractions": "core.math",
  "mod:random": "core.random",
  "mod:secrets": "core.crypto.random",
  "mod:hashlib": "core.crypto",
  "mod:hmac": "core.crypto",
  "mod:datetime": "core.time",
  "mod:time": "core.time",
  "mod:json": "core.encoding.json",
  "mod:csv": "core.encoding.csv",
  "mod:tomllib": "core.encoding.toml",
  "mod:base64": "core.encoding.base64",
  "mod:binascii": "core.encoding.hex",
  "mod:re": "core.regex",
  "mod:pathlib": "core.path",
  "mod:os": "core.os",
  "mod:sys": "core.os",
  "mod:shutil": "core.files",
  "mod:glob": "core.files",
  "mod:tempfile": "core.files",
  "mod:subprocess": "core.process",
  "mod:socket": "core.net",
  "mod:ssl": "core.tls",
  "mod:http": "core.http",
  "mod:http.server": "core.http",
  "mod:urllib.parse": "core.url",
  "mod:uuid": "core.uuid",
  "mod:sqlite3": "core.db",
  "mod:asyncio": "core.tasks",
  "mod:threading": "core.tasks",
  "mod:queue": "core.tasks",
  "mod:unittest": "core.testing",
  "mod:logging": "core.log",
  "mod:struct": "core.binary",
  "mod:zipfile": "core.archive",
  "mod:tarfile": "core.archive",
  "mod:gzip": "core.archive",
  "mod:zlib": "core.archive",
  "mod:statistics": "core.data",
  "mod:unicodedata": "core.text.unicode",
};

// A recorded Python module with no container yet. Listing it keeps the gap
// countable; dropping it silently would hide a whole workflow.
const PYTHON_UNASSIGNED = {
  "mod:argparse": "Jet's core.args has no container in this ledger yet",
  "mod:email": "Jet's core.email has no container in this ledger yet",
  "mod:inspect": "Jet's core.reflect has no container in this ledger yet",
  "mod:mimetypes": "Jet's core.mime has no container in this ledger yet",
  "mod:xml.etree.ElementTree": "Jet's core.encoding.xml has no container in this ledger yet",
  "mod:copy": "Jet's core.mem has no container in this ledger yet",
  "type:bool": "bool mirrors int, which is recorded under core.math",
};

const PYTHON_ABSENT = {
  SortedSet: "no Python standard-library ordered set",
  BitSet: "no Python standard-library bit set; int carries bit operations",
  "core.env": "environment access lives in the os module, recorded under core.os",
  "core.encoding.base32": "base32 lives in the base64 module, recorded under core.encoding.base64",
  "core.fmt": "formatting lives on str.format, recorded under String",
  "core.text": "text handling lives on str, recorded under String",
};

// A Jet module whose workflow is the one an existing container already
// records. Aliasing keeps one comparison per workflow instead of splitting the
// same competitor surface across two names.
const CONTAINER_ALIASES = {
  "core.http.client": "core.http",
  "core.http.server": "core.http",
  "core.time.date": "core.time",
  "core.time.datetime": "core.time",
  "core.time.expiring": "core.time",
  "core.compress.gzip": "core.archive",
  "core.compress.zstd": "core.archive",
  "core.encoding": "core.encoding.json",
  "core.crypto.expert": "core.crypto",
};

// Interchangeable spellings of one operation. This was keyed by an idealised
// name and looked up by Jet's actual spelling, so almost none of it applied:
// Jet spells the operation to_lower, has_key, each, dedup and skip, and the
// table was keyed lower, contains, for_each, unique and drop. Every one of
// those scored a gap against a workflow Jet already ships. It is now an
// equivalence table: any name in a group matches any other.
//
// Groups stay tight on purpose. Merging a name Jet lacks into a group Jet has
// would hide a real gap, so is_subset is not a kind of contains.
const SYNONYM_GROUPS = [
  ["push", "append", "add", "add_last", "push_back", "conj", "add_range", "insert"],
  ["pop", "remove_last", "pop_last", "pop_back"],
  ["len", "size", "count", "length"],
  ["get", "at", "try_get", "item", "nth", "fetch", "get_value"],
  ["set", "put", "store", "set_value"],
  ["remove", "delete", "del", "discard", "erase", "unlink", "remove_at"],
  ["clear", "truncate", "remove_all", "reset", "empty_out"],
  ["contains", "includes", "has", "member", "has_key", "contains_key", "is_member", "in"],
  ["contains_value", "has_value"],
  ["index_of", "index", "find_index", "position", "find_first", "search"],
  ["sort", "sorted", "sort_by", "order_by", "sort_with", "order", "sort_with_comparator"],
  ["reverse", "reversed", "rev"],
  ["map", "select", "convert", "map_values"],
  ["filter", "where", "find_all", "reject"],
  ["fold", "reduce", "inject", "aggregate", "foldl", "foldr", "accumulate"],
  ["each", "for_each", "iterate", "apply_each"],
  ["first", "head", "front", "peek", "first_or_null", "peek_front"],
  ["last", "back", "peek_last", "peek_back"],
  ["indexed", "enumerate", "enumerated", "with_index"],
  ["skip", "drop"],
  ["skip_while", "drop_while"],
  ["take", "limit"],
  ["take_while", "take_until"],
  ["dedup", "unique", "uniq", "distinct", "nub"],
  ["to_lower", "lower", "lowercase", "to_lowercase", "downcase"],
  ["to_upper", "upper", "uppercase", "to_uppercase", "upcase"],
  ["trim", "strip"],
  ["trim_start", "trim_left", "lstrip", "trim_leading"],
  ["trim_end", "trim_right", "rstrip", "chomp", "trim_trailing"],
  ["starts_with", "has_prefix", "start_with"],
  ["ends_with", "has_suffix", "end_with"],
  ["difference", "subtract", "diff", "except", "symmetric_difference_with"],
  ["intersection", "intersect", "intersect_with"],
  ["union", "unite", "union_with"],
  ["concat", "chain", "extend", "append_all", "add_all"],
  ["is_empty", "empty", "is_blank", "none"],
  ["keys", "key_set", "names"],
  ["values", "value_set"],
  ["join", "mk_string", "intercalate"],
  ["split", "split_n"],
  ["split_once", "split_at", "cut"],
  ["replace", "sub", "gsub", "replace_all", "replacing"],
  ["read", "read_text", "read_all", "read_all_text", "read_to_string", "read_file"],
  ["write", "write_text", "write_all", "write_all_text", "write_file"],
  ["exists", "is_file", "is_dir", "file_exists", "is_path"],
  ["parse", "loads", "decode", "deserialize", "try_parse", "from_string"],
  ["to_string", "dumps", "encode", "serialize", "inspect", "format", "describe"],
  ["now", "today", "current_time", "utc_now", "system_time"],
  ["sleep", "delay", "pause"],
  ["abs", "fabs", "magnitude"],
  ["round", "rint"],
  ["floor", "round_down"],
  ["ceil", "ceiling", "round_up"],
  ["sqrt", "square_root"],
  ["pow", "power"],
  ["random", "rand", "next_double", "next_float"],
  ["shuffle", "shuffled", "randomize"],
  ["zip", "zipped", "zip_with"],
  ["chunk", "chunks", "chunked", "grouped", "batch"],
  ["window", "windows", "windowed", "sliding"],
  ["min", "minimum", "min_by", "argmin"],
  ["max", "maximum", "max_by", "argmax"],
  ["sum", "total", "fsum"],
  ["product", "prod"],
  ["any", "some", "any_match", "any_satisfy"],
  ["all", "every", "all_match", "every_match", "all_satisfy"],
  ["count_by", "tally", "counting"],
  ["flat_map", "collect_concat"],
  ["flatten", "flat"],
  ["group_by", "chunk_by", "partition_by"],
  ["to_list", "to_array", "to_vec", "collect_list", "entries"],
  ["slice", "sub_string", "substring", "sub_sequence"],
  ["pad_start", "pad_left", "left_pad", "just_right"],
  ["pad_end", "pad_right", "right_pad", "just_left"],
  ["lines", "each_line", "split_lines", "read_lines"],
  ["chars", "characters", "each_char", "code_points"],
  ["repeat", "times", "cycle_n"],
  ["merge", "put_all", "update_all", "combine"],
];

// normalized name -> every normalized name it is interchangeable with.
const SYNONYM_INDEX = (function () {
  const index = new Map();
  for (const group of SYNONYM_GROUPS) {
    const keys = group.map(function (name) { return name.toLowerCase().replace(/[_!?.\-]/g, ""); });
    for (const key of keys) {
      if (!index.has(key)) index.set(key, new Set());
      for (const other of keys) index.get(key).add(other);
    }
  }
  return index;
})();

const COLLECTION_METHOD_FUNCTIONS = {
  task_list_method_return: "TaskList",
  list_method_return: "List",
  iter_method_return: "Iter",
  view_method_return: "View",
  option_method_return: "Option",
  map_method_return: "Map",
  string_method_return: "String",
  stopwatch_method_return: "Stopwatch",
  clock_method_return: "Clock",
  rng_method_return: "Rng",
  solver_method_return: "Solver",
  duration_method_return: "Duration",
  task_method_return: "Task",
  receiver_method_return: "Receiver",
  sender_method_return: "Sender",
  pool_method_return: "Pool",
  shared_method_return: "Shared",
  shared_weak_method_return: "SharedWeak",
  shared_guard_method_return: "SharedGuard",
  condition_method_return: "Condition",
  cell_method_return: "Cell",
  cell_guard_method_return: "CellGuard",
  signal_method_return: "Signal",
  derived_method_return: "Derived",
  event_method_return: "Event",
  async_event_method_return: "AsyncEvent",
  hook_method_return: "Hook",
  decision_hook_method_return: "DecisionHook",
  subscription_method_return: "Subscription",
  event_scope_method_return: "EventScope",
  event_trace_method_return: "EventTrace",
  dispatch_report_method_return: "DispatchReport",
  watch_handle_method_return: "WatchHandle",
  watch_set_method_return: "WatchSet",
  set_method_return: "Set",
  sorted_set_method_return: "SortedSet",
  priority_queue_method_return: "PriorityQueue",
  lru_method_return: "Cache",
  bit_set_method_return: "BitSet",
  byte_buffer_method_return: "ByteBuffer",
  bag_method_return: "Bag",
  deque_method_return: "Deque",
};

// A container whose losses were carded before this ledger existed. The card is
// recorded so the same gap is not carded twice; --check rejects a reference to
// a card that is closed or missing, which is how a stale owner surfaces
// instead of quietly reading as covered.
const CLUSTER_OWNER_HISTORY = {
  Set: 1404,
  SortedSet: 1404,
  String: 1409,
  List: 1410,
  Map: 1410,
  Iter: 1400,
  "core.io": 1402,
  "core.files": 288,
  "core.path": 288,
};

// ---------------------------------------------------------------------------
// Source-table parsing. The compiler's own tables decide what Jet ships.

function read(relativePath) {
  const absolute = join(ROOT, relativePath);
  if (!existsSync(absolute)) throw new Error("missing source: " + relativePath);
  return readFileSync(absolute, "utf8");
}

function sha256(text) {
  return createHash("sha256").update(text).digest("hex");
}

function stable(value) {
  if (Array.isArray(value)) return "[" + value.map(stable).join(",") + "]";
  if (value && typeof value === "object") {
    return "{" + Object.keys(value).sort().map(function (key) {
      return JSON.stringify(key) + ":" + stable(value[key]);
    }).join(",") + "}";
  }
  return JSON.stringify(value);
}

function quoted(text) {
  return Array.from(text.matchAll(/"([^"\\]*(?:\\.[^"\\]*)*)"/g))
    .map(function (match) { return match[1].replace(/\\"/g, '"'); });
}

function lineAt(text, offset) {
  return text.slice(0, offset).split(/\r?\n/).length;
}

function functionBody(text, name) {
  const start = text.indexOf("fn " + name + "(");
  if (start < 0) throw new Error("function not found: " + name);
  const open = text.indexOf("{", start);
  let depth = 0;
  let state = "code";
  let escaped = false;
  let blockDepth = 0;
  for (let index = open; index < text.length; index += 1) {
    const char = text[index];
    const next = text[index + 1];
    if (state === "line") {
      if (char === "\n") state = "code";
      continue;
    }
    if (state === "block") {
      if (char === "/" && next === "*") { blockDepth += 1; index += 1; }
      else if (char === "*" && next === "/") {
        blockDepth -= 1;
        index += 1;
        if (blockDepth === 0) state = "code";
      }
      continue;
    }
    if (state === "string") {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === "\"") state = "code";
      continue;
    }
    if (char === "/" && next === "/") { state = "line"; index += 1; continue; }
    if (char === "/" && next === "*") { state = "block"; blockDepth = 1; index += 1; continue; }
    if (char === "\"") { state = "string"; continue; }
    if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, index);
    }
  }
  throw new Error("unterminated function: " + name);
}

/*
 * Parse one Rust match. The returned arms are exact top-level arms; nested
 * matches, strings, comments, and commas inside expressions are ignored.
 */
function matchArms(source, needle) {
  const matchAt = source.indexOf(needle);
  if (matchAt < 0) throw new Error("match not found: " + needle);
  const open = source.indexOf("{", matchAt);
  let brace = 0;
  let paren = 0;
  let bracket = 0;
  let state = "code";
  let escaped = false;
  let blockDepth = 0;
  let armStart = open + 1;
  let arrow = -1;
  const arms = [];

  function push(end) {
    if (arrow >= 0) {
      arms.push({
        lhs: source.slice(armStart, arrow),
        rhs: source.slice(arrow + 2, end),
      });
    }
    armStart = end + 1;
    arrow = -1;
  }

  for (let index = open; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];
    if (state === "line") {
      if (char === "\n") state = "code";
      continue;
    }
    if (state === "block") {
      if (char === "/" && next === "*") { blockDepth += 1; index += 1; }
      else if (char === "*" && next === "/") {
        blockDepth -= 1;
        index += 1;
        if (blockDepth === 0) state = "code";
      }
      continue;
    }
    if (state === "string") {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === "\"") state = "code";
      continue;
    }
    if (char === "/" && next === "/") { state = "line"; index += 1; continue; }
    if (char === "/" && next === "*") { state = "block"; blockDepth = 1; index += 1; continue; }
    if (char === "\"") { state = "string"; continue; }
    if (char === "{") { brace += 1; continue; }
    if (char === "}") {
      if (brace === 1 && arrow >= 0) push(index);
      brace -= 1;
      if (brace === 0) break;
      continue;
    }
    if (brace !== 1) continue;
    if (char === "(") paren += 1;
    else if (char === ")") paren -= 1;
    else if (char === "[") bracket += 1;
    else if (char === "]") bracket -= 1;
    else if (char === "=" && next === ">" && paren === 0 && bracket === 0 && arrow < 0) {
      arrow = index;
      index += 1;
    } else if (char === "," && paren === 0 && bracket === 0 && arrow >= 0) {
      push(index);
    }
  }
  return arms;
}

function syntaxConstants() {
  const values = new Map();
  for (const file of ["crates/jet-foundation/src/Syntax.rs", SYNTAX_PATH]) {
    const source = read(file);
    for (const match of source.matchAll(/pub const ([A-Z][A-Z0-9_]*):\s*&str\s*=\s*"([^"]*)"/g)) {
      values.set(match[1], match[2]);
    }
  }
  return values;
}

function resolveItems(body, constants) {
  const result = new Set(quoted(body));
  for (const match of body.matchAll(/\b(?:Syntax::)?([A-Z][A-Z0-9_]*)\b/g)) {
    if (constants.has(match[1])) result.add(constants.get(match[1]));
  }
  return Array.from(result).sort();
}

function canonicalModule(name) {
  if (name.startsWith("jet.")) return "core." + name.slice(4);
  return name;
}

function moduleToken(name) {
  return name === "app" || name.startsWith("core.") || name.startsWith("jet.");
}

function moduleInventory() {
  const source = read(MODULE_ITEMS_PATH);
  const constants = syntaxConstants();
  const body = functionBody(source, "core_module_items");
  const entries = new Map();

  for (const arm of matchArms(body, "match module")) {
    if (!arm.rhs.includes("&[")) continue;
    const rawModules = quoted(arm.lhs).filter(moduleToken);
    if (rawModules.length === 0) continue;
    const members = resolveItems(arm.rhs, constants);
    for (const raw of rawModules) {
      const module = canonicalModule(raw);
      if (!entries.has(module)) {
        entries.set(module, {
          module: module,
          rawModules: [],
          members: new Set(),
          sourceLine: lineAt(source, source.indexOf("\"" + raw + "\"")),
        });
      }
      const entry = entries.get(module);
      if (!entry.rawModules.includes(raw)) entry.rawModules.push(raw);
      for (const member of members) entry.members.add(member);
    }
  }

  const policy = read(POLICY_PATH);
  if (!source.includes("Policy::RULE_ARG_DECLARATIONS")) {
    throw new Error("core.lang dynamic source anchor disappeared from module_items.rs");
  }
  const appliedAt = policy.indexOf("pub const APPLIED_RULES");
  const applied = policy.slice(appliedAt, policy.indexOf("\n];", appliedAt) + 3);
  const langMembers = new Set(["Track"]);
  for (const match of applied.matchAll(/=>\s*"([^"]+)"/g)) langMembers.add(match[1]);
  entries.set("core.lang", {
    module: "core.lang",
    rawModules: ["core.lang (Policy::RULE_ARG_DECLARATIONS)"],
    members: langMembers,
    sourceLine: lineAt(source, source.indexOf("core.lang")),
  });

  const predicates = read(PREDICATES_PATH);
  const knownAt = predicates.indexOf("pub const KNOWN_CORE_MODULES");
  const knownBody = predicates.slice(knownAt, predicates.indexOf("];", knownAt) + 2);
  const known = resolveItems(knownBody, constants).filter(function (name) {
    return moduleToken(name);
  }).map(canonicalModule);

  const actual = new Set(entries.keys());
  const missing = known.filter(function (name) { return !actual.has(name); });
  const extra = Array.from(actual).filter(function (name) {
    return !known.includes(name);
  });
  if (missing.length || extra.length) {
    throw new Error("module_items/KNOWN_CORE_MODULES drift: missing=" +
      missing.join(",") + " extra=" + extra.join(","));
  }

  return Array.from(entries.values()).map(function (entry) {
    return {
      module: entry.module,
      rawModules: entry.rawModules.sort(),
      members: Array.from(entry.members).sort(),
      sourceLine: entry.sourceLine,
    };
  }).sort(function (left, right) { return left.module.localeCompare(right.module); });
}

function fixedSignaturePairs(modules) {
  const source = read(FIXED_SIGS_PATH);
  const knownModules = new Set(modules.map(function (entry) { return entry.module; }));
  const arms = matchArms(functionBody(source, "core_fixed_sig"), "match (module, name)");
  const pairs = new Set();
  for (const arm of arms) {
    const strings = quoted(arm.lhs);
    const moduleIndexes = strings.map(function (value, index) {
      return moduleToken(value) ? index : -1;
    }).filter(function (index) { return index >= 0; });
    if (moduleIndexes.length === 0) continue;
    const lastModule = moduleIndexes[moduleIndexes.length - 1];
    const methods = strings.slice(lastModule + 1).filter(function (value) {
      return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value) && !moduleToken(value);
    });
    for (const raw of moduleIndexes.map(function (index) { return strings[index]; })) {
      const module = canonicalModule(raw);
      if (!knownModules.has(module)) {
        throw new Error("fixed_sigs names an unknown Core module: " + raw);
      }
      for (const method of methods) pairs.add(module + "." + method);
    }
  }
  return Array.from(pairs).sort();
}

function collectionInventory() {
  const source = read(COLLECTIONS_PATH);
  return Object.keys(COLLECTION_METHOD_FUNCTIONS).map(function (functionName) {
    const body = functionBody(source, functionName);
    const methods = new Set();
    for (const arm of matchArms(body, "match (")) {
      for (const method of quoted(arm.lhs)) methods.add(method);
    }
    return {
      function: functionName,
      type: COLLECTION_METHOD_FUNCTIONS[functionName],
      methods: Array.from(methods).sort(),
      sourceLine: lineAt(source, source.indexOf("fn " + functionName + "(")),
    };
  });
}

// ---------------------------------------------------------------------------
// Competitor surfaces.

function pythonSurface() {
  const snapshot = JSON.parse(readFileSync(PYTHON_SURFACE_PATH, "utf8"));
  const byContainer = new Map();
  const record = function (container, key) {
    if (!byContainer.has(container)) byContainer.set(container, { operations: new Set(), sources: [] });
    byContainer.get(container).sources.push(key);
    return byContainer.get(container).operations;
  };
  for (const [key, container] of Object.entries(PYTHON_SOURCE_CONTAINER)) {
    const name = key.slice(key.indexOf(":") + 1);
    if (key.startsWith("type:")) {
      const entry = snapshot.builtinTypes[name];
      if (!entry) throw new Error("Python builtin type absent from the snapshot: " + name);
      const into = record(container, key);
      for (const member of entry.members) into.add(member);
      continue;
    }
    const entry = snapshot.stdlibModules[name];
    if (!entry) throw new Error("Python module absent from the snapshot: " + name);
    const into = record(container, key);
    for (const member of entry.operations) into.add(member);
    for (const members of Object.values(entry.types || {})) {
      for (const member of members) into.add(member);
    }
  }
  const containers = {};
  for (const [name, entry] of byContainer.entries()) {
    containers[name] = {
      present: true,
      pythonSources: entry.sources.sort(),
      operations: Array.from(entry.operations).sort(),
    };
  }
  for (const [name, reason] of Object.entries(PYTHON_ABSENT)) {
    containers[name] = { present: false, reason: reason, operations: [] };
  }
  return {
    language: "Python",
    sourceKind: "runtime introspection",
    runtime: "python " + snapshot.pythonVersion,
    scopeRule: snapshot.scopeRule,
    unassignedSources: PYTHON_UNASSIGNED,
    officialReferences: [
      "https://docs.python.org/3/library/functions.html",
      "https://docs.python.org/3/library/stdtypes.html",
      "https://docs.python.org/3/library/index.html",
    ],
    containers: containers,
    totals: {
      containers: Object.keys(containers).length,
      presentContainers: Object.values(containers).filter(function (c) { return c.present; }).length,
      operations: Object.values(containers).reduce(function (count, c) {
        return count + c.operations.length;
      }, 0),
    },
  };
}

function loadSurfaces() {
  const surfaces = {};
  for (const [language, path] of Object.entries(SURFACE_FILES)) {
    const surface = JSON.parse(read(path));
    if (!surface.containers) throw new Error("surface has no containers: " + path);
    surfaces[language] = { path: path, surface: surface };
  }
  surfaces.Python = { path: "docs/reference/python-surface.json", surface: pythonSurface() };
  return surfaces;
}

// The canonical container set is the union of every recorded surface. A
// language missing one of them is a hidden exclusion, not a silent absence.
function canonicalContainers(surfaces) {
  const names = new Set();
  for (const entry of Object.values(surfaces)) {
    for (const name of Object.keys(entry.surface.containers)) names.add(name);
  }
  return Array.from(names).sort();
}

// An operator is syntax, not a named operation. Ruby alone exposes %, *, +, +@,
// <<, =~, [] and []= as String members; scoring them as missing calls compared
// Jet's method surface against another language's punctuation.
function isNamedOperation(name) {
  return /^[A-Za-z_][A-Za-z0-9_]*[!?=]?$/.test(name);
}

function normalize(name) {
  return name.toLowerCase().replace(/[_!?.\-]/g, "");
}

function synonymsFor(jetMember) {
  const base = normalize(jetMember);
  const keys = new Set([base]);
  for (const alias of SYNONYM_INDEX.get(base) || []) keys.add(alias);
  return keys;
}

function containerFor(name) {
  return CONTAINER_ALIASES[name] || name;
}

// ---------------------------------------------------------------------------
// Rows.

function competitorCells(surfaces, container, jetMember) {
  const keys = synonymsFor(jetMember);
  const cells = {};
  for (const [language, entry] of Object.entries(surfaces)) {
    const record = entry.surface.containers[container];
    if (!record) {
      cells[language] = { status: "no_container_recorded", operation: null };
      continue;
    }
    if (!record.present) {
      cells[language] = { status: "container_absent", operation: null, reason: record.reason };
      continue;
    }
    const exact = normalize(jetMember);
    const hit = record.operations.find(function (operation) {
      return normalize(operation) === exact;
    }) || record.operations.find(function (operation) {
      return keys.has(normalize(operation));
    });
    cells[language] = hit ? { status: "has", operation: hit } : { status: "lacks", operation: null };
  }
  return cells;
}

// One verdict per row. A row no recorded competitor answers is a Jet win; a row
// at least one answers is equal. A container no surface records cannot be
// scored either way, and says so.
function verdictFor(cells) {
  const values = Object.values(cells);
  if (values.every(function (cell) { return cell.status === "no_container_recorded"; })) {
    return "not_compared";
  }
  return values.some(function (cell) { return cell.status === "has"; }) ? "equal" : "jet_wins";
}

function rowForModule(entry, member, fixedOnly, surfaces) {
  const container = containerFor(entry.module);
  const cells = competitorCells(surfaces, container, member);
  return {
    id: "module." + entry.module + "." + member,
    source: {
      kind: fixedOnly ? "fixed_sig" : "module_item",
      module: entry.module,
      member: member,
      sourceLine: entry.sourceLine,
    },
    container: container,
    jetSpelling: entry.module + "." + member,
    workflow: "Core module workflow for " + container,
    verdict: verdictFor(cells),
    competitors: cells,
    evidence: ["source:" + MODULE_ITEMS_PATH, "source:" + FIXED_SIGS_PATH],
  };
}

function rowForCollection(entry, method, surfaces) {
  const container = containerFor(entry.type);
  const cells = competitorCells(surfaces, container, method);
  return {
    id: "collection." + entry.type + "." + method,
    source: {
      kind: "collection_method_return",
      type: entry.type,
      function: entry.function,
      member: method,
      sourceLine: entry.sourceLine,
    },
    container: container,
    jetSpelling: entry.type + "." + method,
    workflow: "Core type workflow for " + container,
    verdict: verdictFor(cells),
    competitors: cells,
    evidence: ["source:" + COLLECTIONS_PATH],
  };
}

// Walking only Jet's own tables can never surface a feature Jet is missing, so
// every recorded competitor operation that no Jet row matched becomes its own
// visible row.
function competitorRows(surfaces, jetRows) {
  // Matching is set to set, not one to one. Recording only the single operation
  // each Jet member happened to match left every other spelling of the same
  // workflow scored as a gap: List.push matched Rust append, and Rust push,
  // Ruby push and Python append all still counted as separate losses.
  const covered = new Map();
  const jetContainers = new Set();
  for (const row of jetRows) {
    jetContainers.add(row.container);
    if (!covered.has(row.container)) covered.set(row.container, new Set());
    const keys = covered.get(row.container);
    for (const key of synonymsFor(row.source.member)) keys.add(key);
  }

  // A gap is one workflow Jet lacks, not one row per language. Ten languages
  // shipping sqrt is one missing operation with ten witnesses, and minting a
  // row each multiplied the backlog by the size of the comparison set.
  const gaps = new Map();
  for (const [language, entry] of Object.entries(surfaces)) {
    for (const [container, record] of Object.entries(entry.surface.containers)) {
      if (!record.present) continue;
      // Jet must ship the container before an operation in it can be a loss. A
      // container Jet has no table for is reported as an uncompared domain
      // instead, so the shortfall is named without being invented.
      if (!jetContainers.has(container)) continue;
      // A package-level index can confirm that the language documents a name,
      // but cannot place that name in this container. Minting a gap row from
      // it would score Jet against operations the index never attributed here.
      // The skip is listed in packageAttributedContainers, never silent.
      if (record.attribution === "package") continue;
      const keys = covered.get(container);
      for (const operation of record.operations) {
        // An operator is syntax, not a named operation. Ruby alone exposes %,
        // *, +, +@, <<, =~, [] and []= as String members.
        if (!isNamedOperation(operation)) continue;
        const key = normalize(operation);
        if (keys.has(key)) continue;
        const id = "gap." + container + "." + key;
        if (!gaps.has(id)) {
          gaps.set(id, { id: id, container: container, key: key, spellings: {}, evidence: new Set() });
        }
        const gap = gaps.get(id);
        // One row per distinct operation per language: Ruby ships chop and
        // chop!, which are one workflow spelled twice.
        if (!gap.spellings[language]) gap.spellings[language] = operation;
        gap.evidence.add("surface:" + entry.path);
      }
    }
  }

  return Array.from(gaps.values()).map(function (gap) {
    const languages = Object.keys(gap.spellings).sort();
    const competitors = {};
    for (const language of languages) {
      competitors[language] = { status: "has", operation: gap.spellings[language] };
    }
    return {
      id: gap.id,
      source: {
        kind: "competitor_operation",
        container: gap.container,
        member: gap.key,
        languages: languages,
        sourceLine: null,
      },
      container: gap.container,
      jetSpelling: null,
      workflow: "operation " + languages.length + " of " + Object.keys(surfaces).length +
        " compared languages ship in " + gap.container + ", with no matching Jet spelling",
      // One language shipping a name is not evidence that Jet is missing a
      // workflow: 8446 of 9117 gaps had a single witness, and they are that
      // language's own internals, such as Rust's align_to and as_mut_ptr,
      // which a memory-safe language cannot and should not expose. A gap two
      // compared languages agree on is a real one. Single-witness rows stay in
      // the ledger and stay counted; they are recorded, not scored.
      verdict: languages.length >= 2 ? "jet_loses" : "single_witness",
      witnessCount: languages.length,
      competitors: competitors,
      evidence: Array.from(gap.evidence).sort(),
    };
  });
}

function buildRows(modules, fixedPairs, collections, surfaces) {
  const rows = [];
  const moduleKeys = new Set();
  for (const entry of modules) {
    for (const member of entry.members) {
      moduleKeys.add(entry.module + "." + member);
      rows.push(rowForModule(entry, member, false, surfaces));
    }
  }
  for (const pair of fixedPairs) {
    if (moduleKeys.has(pair)) continue;
    const split = pair.lastIndexOf(".");
    const module = pair.slice(0, split);
    const member = pair.slice(split + 1);
    const entry = modules.find(function (item) { return item.module === module; });
    if (!entry) throw new Error("fixed signature module missing from inventory: " + module);
    rows.push(rowForModule(entry, member, true, surfaces));
  }
  for (const entry of collections) {
    for (const method of entry.methods) rows.push(rowForCollection(entry, method, surfaces));
  }
  rows.sort(function (left, right) { return left.id.localeCompare(right.id); });
  return rows;
}

// ---------------------------------------------------------------------------
// Owners.

function towerBoard() {
  if (!existsSync(TOWER_PATH)) return null;
  return JSON.parse(readFileSync(TOWER_PATH, "utf8"));
}

function towerCards(board) {
  if (!board) return null;
  const cards = new Map();
  for (const card of board.cards || []) cards.set(card.num, card);
  return cards;
}

// A cluster is one container's losses. Owning a gap per container is what the
// existing cards already do, so the ledger folds into them rather than opening
// a second owner for the same surface.
function lossClusters(rows, cards) {
  const byContainer = new Map();
  for (const row of rows) {
    if (row.verdict !== "jet_loses") continue;
    if (!byContainer.has(row.container)) {
      byContainer.set(row.container, { container: row.container, lossCount: 0, languages: new Set() });
    }
    const cluster = byContainer.get(row.container);
    cluster.lossCount += 1;
    for (const language of row.source.languages || []) cluster.languages.add(language);
  }
  return Array.from(byContainer.values()).map(function (cluster) {
    const card = CLUSTER_OWNER_HISTORY[cluster.container] ?? null;
    const record = card !== null && cards ? cards.get(card) : null;
    let ownerState = "needs_card";
    if (card !== null && !cards) ownerState = "unverified";
    else if (record && record.phase !== "done") ownerState = "live";
    else if (record) ownerState = "closed";
    else if (card !== null) ownerState = "missing";
    return {
      container: cluster.container,
      lossCount: cluster.lossCount,
      languages: Array.from(cluster.languages).sort(),
      priorCard: card,
      priorCardPhase: record ? record.phase : null,
      ownerState: ownerState,
    };
  }).sort(function (left, right) {
    return right.lossCount - left.lossCount || left.container.localeCompare(right.container);
  });
}

// A container whose surface is indexed per package rather than per container.
// It can confirm a Jet match but cannot mint a gap row, so it is listed here
// and the skip stays countable.
function packageAttributedContainers(surfaces) {
  const out = [];
  for (const [language, entry] of Object.entries(surfaces)) {
    for (const [container, record] of Object.entries(entry.surface.containers)) {
      if (record.present && record.attribution === "package") {
        out.push({
          language: language,
          container: container,
          recordedOperations: record.operations.length,
          reason: "the recorded index is per package, so it cannot attribute an operation to this container",
        });
      }
    }
  }
  return out.sort(function (left, right) {
    return left.language.localeCompare(right.language) ||
      left.container.localeCompare(right.container);
  });
}

function uncomparedDomains(modules, containers) {
  const recorded = new Set(containers);
  return modules.map(function (entry) { return entry.module; })
    .filter(function (module) { return !recorded.has(containerFor(module)); })
    .sort();
}

function sourceFiles() {
  return [
    MODULE_ITEMS_PATH,
    FIXED_SIGS_PATH,
    COLLECTIONS_PATH,
    PREDICATES_PATH,
    POLICY_PATH,
    "crates/jet-foundation/src/Syntax.rs",
    SYNTAX_PATH,
    "docs/reference/python-surface.json",
  ].concat(Object.values(SURFACE_FILES)).map(function (path) {
    const source = read(path);
    return { path: path, sha256: sha256(source), lineCount: source.split(/\r?\n/).length };
  });
}

function buildLedger() {
  const surfaces = loadSurfaces();
  const containers = canonicalContainers(surfaces);
  const modules = moduleInventory();
  const fixedPairs = fixedSignaturePairs(modules);
  const collections = collectionInventory();
  const jetRows = buildRows(modules, fixedPairs, collections, surfaces);
  const rows = jetRows.concat(competitorRows(surfaces, jetRows));
  const clusters = lossClusters(rows, towerCards(towerBoard()));
  const uncompared = uncomparedDomains(modules, containers);

  const byVerdict = {};
  for (const row of rows) byVerdict[row.verdict] = (byVerdict[row.verdict] || 0) + 1;

  const perLanguage = {};
  for (const [language, entry] of Object.entries(surfaces)) {
    perLanguage[language] = {
      sourceKind: entry.surface.sourceKind,
      runtime: entry.surface.runtime,
      surface: entry.path,
      recordedContainers: Object.keys(entry.surface.containers).length,
      presentContainers: entry.surface.totals ? entry.surface.totals.presentContainers : null,
      recordedOperations: entry.surface.totals ? entry.surface.totals.operations : null,
      jetRowsMatched: jetRows.filter(function (row) {
        return row.competitors[language] && row.competitors[language].status === "has";
      }).length,
      lossRows: rows.filter(function (row) {
        return row.verdict === "jet_loses" && (row.source.languages || []).includes(language);
      }).length,
      officialReferences: entry.surface.officialReferences,
    };
  }

  return {
    schemaVersion: 2,
    title: "Jet Core surface ledger",
    sourceOfTruth: "docs/reference/core-surface-ledger.json",
    generatedOn: new Date().toISOString().slice(0, 10),
    ruling: "Owner ruling 2026-08-03: the bar is not Python; it is every language Jet competes with.",
    sourceFiles: sourceFiles(),
    canonicalContainers: containers,
    containerAliases: CONTAINER_ALIASES,
    synonymGroups: SYNONYM_GROUPS,
    competitors: perLanguage,
    consumer: {
      card: 1398,
      input: "docs/reference/core-surface-ledger.json",
      manualWorkflowInventory: false,
      rule: "Load rows from this file. Do not copy the inventory into a second workflow rubric.",
    },
    inventory: { modules: modules, fixedSignaturePairs: fixedPairs, collections: collections },
    lossClusters: clusters,
    packageAttributedContainers: packageAttributedContainers(surfaces),
    uncomparedDomains: uncompared,
    rows: rows,
    summary: {
      languageCount: Object.keys(surfaces).length,
      containerCount: containers.length,
      moduleCount: modules.length,
      moduleMemberCount: modules.reduce(function (count, entry) {
        return count + entry.members.length;
      }, 0),
      fixedSignatureOnlyCount: rows.filter(function (row) {
        return row.source.kind === "fixed_sig";
      }).length,
      collectionFunctionCount: collections.length,
      collectionMethodCount: collections.reduce(function (count, entry) {
        return count + entry.methods.length;
      }, 0),
      rowCount: rows.length,
      jetRowCount: jetRows.length,
      verdicts: byVerdict,
      lossClusterCount: clusters.length,
      clustersNeedingCard: clusters.filter(function (c) { return c.ownerState !== "live"; }).length,
      uncomparedDomainCount: uncompared.length,
      packageAttributedContainerCount: packageAttributedContainers(surfaces).length,
    },
  };
}

// ---------------------------------------------------------------------------
// Validation. Truthfulness is gated; coverage is printed.

function validateSurfaces(ledger, surfaces) {
  surfaces = surfaces || loadSurfaces();
  const containers = canonicalContainers(surfaces);
  if (Object.keys(surfaces).length < 11) {
    throw new Error("the owner named eleven languages; only " +
      Object.keys(surfaces).length + " surfaces are recorded");
  }
  for (const [language, entry] of Object.entries(surfaces)) {
    const recorded = Object.keys(entry.surface.containers);
    const missing = containers.filter(function (name) { return !recorded.includes(name); });
    if (missing.length) {
      throw new Error("hidden exclusion: " + language + " records no verdict for " + missing.join(", "));
    }
    for (const [name, record] of Object.entries(entry.surface.containers)) {
      if (!record.present && !record.reason) {
        throw new Error("hidden exclusion: " + language + " marks " + name + " absent with no reason");
      }
      if (record.present && record.operations.length === 0) {
        throw new Error("empty recorded container: " + language + " " + name);
      }
    }
    if (!entry.surface.sourceKind || !entry.surface.scopeRule ||
        !(entry.surface.officialReferences || []).length) {
      throw new Error("surface without recorded provenance: " + language);
    }
  }
  if (stable(ledger.canonicalContainers) !== stable(containers)) {
    throw new Error("ledger container set drifted from the recorded surfaces");
  }
}

function validateRows(ledger, surfaces) {
  surfaces = surfaces || loadSurfaces();
  const ids = new Set();
  const sourceKeys = new Set();
  const verdicts = new Set(["equal", "jet_wins", "jet_loses", "single_witness", "not_compared", "declined"]);
  for (const row of ledger.rows) {
    if (ids.has(row.id)) throw new Error("duplicate row id: " + row.id);
    ids.add(row.id);
    const sourceKey = row.source.kind + ":" +
      (row.source.module || row.source.type || row.source.container) + ":" + row.source.member;
    if (sourceKeys.has(sourceKey)) throw new Error("duplicate source row: " + sourceKey);
    sourceKeys.add(sourceKey);
    if (!row.workflow || !row.verdict || !row.container) throw new Error("incomplete row: " + row.id);
    if (!verdicts.has(row.verdict)) {
      throw new Error("invalid verdict in " + row.id + ": " + row.verdict);
    }
    if (row.verdict !== "jet_loses" && row.verdict !== "single_witness" && !row.jetSpelling) {
      throw new Error("row without a Jet spelling: " + row.id);
    }
    // A row may not assert an operation the recorded surface does not have.
    for (const [language, cell] of Object.entries(row.competitors)) {
      if (cell.status !== "has") continue;
      const record = surfaces[language] && surfaces[language].surface.containers[row.container];
      if (!record || !record.present || !record.operations.includes(cell.operation)) {
        throw new Error("unverified competitor claim in " + row.id + ": " + language + " " + cell.operation);
      }
    }
    if (row.verdict === "equal" &&
        !Object.values(row.competitors).some(function (cell) { return cell.status === "has"; })) {
      throw new Error("equal verdict with no matching competitor operation: " + row.id);
    }
  }
}

// A cluster may say it has no card. It may not name one that is closed or
// absent, which is how a gap silently reads as owned after its card is done.
function validateOwners(ledger, board) {
  board = board === undefined ? towerBoard() : board;
  const cards = towerCards(board);
  for (const cluster of ledger.lossClusters) {
    if (cluster.ownerState === "live") {
      if (!cards) throw new Error("cluster claims a live owner but no board is readable: " + cluster.container);
      const card = cards.get(cluster.priorCard);
      if (!card) {
        throw new Error("stale owner: " + cluster.container + " names missing card #" + cluster.priorCard);
      }
      if (card.phase === "done") {
        throw new Error("stale owner: " + cluster.container + " names closed card #" + cluster.priorCard);
      }
    }
    if (cluster.ownerState === "needs_card" && cluster.priorCard !== null) {
      throw new Error("cluster " + cluster.container + " both names card #" +
        cluster.priorCard + " and claims to need one");
    }
  }
  for (const row of ledger.rows) {
    if (row.verdict !== "declined") continue;
    if (!row.declinedBy) throw new Error("declined row without a decision id: " + row.id);
    const decision = board && (board.decisions || []).find(function (item) {
      return item.id === row.declinedBy;
    });
    if (!decision) throw new Error("declined row names an unknown decision: " + row.declinedBy);
    if (decision.status !== "ratified") {
      throw new Error("unratified scope exclusion in " + row.id + ": " + row.declinedBy);
    }
  }
}

function validateCoverage(ledger) {
  const expected = uncomparedDomains(moduleInventory(), ledger.canonicalContainers);
  if (stable(expected) !== stable(ledger.uncomparedDomains)) {
    const hidden = expected.filter(function (name) {
      return !ledger.uncomparedDomains.includes(name);
    });
    throw new Error("uncompared Core domains are not fully listed; the ledger hides " +
      (hidden.join(", ") || "nothing, but the list has drifted"));
  }
}

function compareLedger(stored, expected) {
  const left = JSON.parse(JSON.stringify(stored));
  const right = JSON.parse(JSON.stringify(expected));
  delete left.generatedOn;
  delete right.generatedOn;
  if (stable(left) !== stable(right)) {
    throw new Error("core surface ledger drifted; run --refresh only after reviewing source and policy");
  }
}

function loadJson(path) {
  if (!existsSync(path)) throw new Error("missing ledger: " + path);
  return JSON.parse(readFileSync(path, "utf8"));
}

// ---------------------------------------------------------------------------

function markdown(ledger) {
  const v = ledger.summary.verdicts;
  const lines = [
    "# Jet Core surface ledger",
    "",
    "Owner ruling 2026-08-03: the bar is not Python. It is every language Jet",
    "competes with, and a missing feature is not acceptable.",
    "",
    "This page is the durable review index. The JSON file beside it is the",
    "machine-readable source that card #1398 reads. Do not keep a second",
    "hand-written workflow inventory.",
    "",
    "Generated on: " + ledger.generatedOn,
    "",
    "## What decides a row",
    "",
    "- What Jet ships comes from the compiler tables: module_items.rs,",
    "  fixed_sigs.rs, and Collections.rs.",
    "- What a competitor ships comes from that language's own recorded surface,",
    "  read from a runtime, from standard-library source, or from official",
    "  machine-readable documentation.",
    "- A row carries one verdict. `equal` means at least one recorded competitor",
    "  answers the same workflow. `jet_wins` means none does. `jet_loses` is an",
    "  operation two or more compared languages ship and Jet has no spelling for.",
    "  `single_witness` is an operation exactly one language ships.",
    "  `not_compared` means no surface records that container yet.",
    "- A gap is one workflow, not one row per language. Ten languages shipping",
    "  `sqrt` is one missing operation with ten witnesses.",
    "- One language is not evidence. A single-witness row is almost always that",
    "  language's own internals, such as Rust's `align_to` and `as_mut_ptr`,",
    "  which a memory-safe language must not expose. Those rows stay in the",
    "  ledger and stay counted, but they are recorded rather than scored.",
    "- `--check` rejects source drift, a competitor claim the recorded surface",
    "  does not support, a duplicate row, a container a language silently",
    "  skipped, an owner card that is closed or missing, and an unratified",
    "  scope exclusion.",
    "",
    "## Inventory",
    "",
    "| Measure | Count |",
    "| --- | ---: |",
    "| Languages compared | " + ledger.summary.languageCount + " |",
    "| Shared containers | " + ledger.summary.containerCount + " |",
    "| Core modules | " + ledger.summary.moduleCount + " |",
    "| Module members | " + ledger.summary.moduleMemberCount + " |",
    "| Collection method rows | " + ledger.summary.collectionMethodCount + " |",
    "| Jet-side rows | " + ledger.summary.jetRowCount + " |",
    "| Total rows | " + ledger.summary.rowCount + " |",
    "",
    "## Verdicts",
    "",
    "| Verdict | Rows |",
    "| --- | ---: |",
    "| Jet wins | " + (v.jet_wins || 0) + " |",
    "| Equal | " + (v.equal || 0) + " |",
    "| Jet loses (two or more languages agree) | " + (v.jet_loses || 0) + " |",
    "| Single witness (recorded, not scored) | " + (v.single_witness || 0) + " |",
    "| Not compared | " + (v.not_compared || 0) + " |",
    "| Deliberately declined | " + (v.declined || 0) + " |",
    "",
    "## Competitors",
    "",
    "| Language | Surface read from | Recorded operations | Jet rows matched | Loss rows |",
    "| --- | --- | ---: | ---: | ---: |",
  ];
  for (const [language, entry] of Object.entries(ledger.competitors)) {
    lines.push("| " + language + " | " + entry.sourceKind + " | " +
      (entry.recordedOperations ?? 0) + " | " + entry.jetRowsMatched + " | " + entry.lossRows + " |");
  }
  lines.push(
    "",
    "## Loss clusters",
    "",
    "A cluster is one container's losses. Owning a gap per container is what",
    "the existing cards already do, so the ledger folds into them rather than",
    "opening a second owner for the same surface. `needs_card` means no card",
    "owns that container today, and `closed` means the card that used to owns",
    "it is done while losses remain.",
    "",
    "| Container | Loss rows | Prior card | Card phase | Owner |",
    "| --- | ---: | --- | --- | --- |",
  );
  for (const cluster of ledger.lossClusters) {
    lines.push("| " + cluster.container + " | " + cluster.lossCount + " | " +
      (cluster.priorCard ? "#" + cluster.priorCard : "none") + " | " +
      (cluster.priorCardPhase || "n/a") + " | " + cluster.ownerState + " |");
  }
  lines.push(
    "",
    "## Containers indexed per package",
    "",
    "These surfaces are indexed a whole package at a time, so the index can",
    "confirm that the language documents a name but cannot place that name in",
    "one container. They still confirm a Jet match; they do not mint a gap row,",
    "because that would score Jet against operations the index never attributed",
    "here. The skip is listed so it stays countable.",
    "",
    "| Language | Container | Recorded operations |",
    "| --- | --- | ---: |",
  );
  for (const entry of ledger.packageAttributedContainers) {
    lines.push("| " + entry.language + " | " + entry.container + " | " + entry.recordedOperations + " |");
  }
  lines.push(
    "",
    "## Core domains not yet compared",
    "",
    "No competitor surface records a container for these Core modules, so no",
    "row scores them. They are listed so the shortfall stays countable rather",
    "than invisible.",
    "",
    ledger.uncomparedDomains.map(function (name) { return "`" + name + "`"; }).join(", ") || "none",
    "",
    "## Consumer",
    "",
    "Card #1398 reads docs/reference/core-surface-ledger.json as its only",
    "workflow inventory.",
    "",
    "Regenerate and check from the repository root:",
    "",
    "~~~sh",
    "node scripts/agent/check-core-surface-ledger.mjs --refresh",
    "node scripts/agent/check-core-surface-ledger.mjs --check",
    "~~~",
    "",
    "Full rows stay in the JSON artifact so the release rubric can read",
    "structured data without duplicating this inventory.",
    "",
  );
  return lines.join("\n");
}

function refresh() {
  const ledger = buildLedger();
  writeFileSync(LEDGER_PATH, JSON.stringify(ledger, null, 2) + "\n");
  writeFileSync(README_PATH, markdown(ledger));
  process.stdout.write("wrote " + LEDGER_PATH + "\n");
  process.stdout.write("rows=" + ledger.summary.rowCount +
    " verdicts=" + JSON.stringify(ledger.summary.verdicts) + "\n");
}

function check() {
  const stored = loadJson(LEDGER_PATH);
  // Order matters: a hand-edited fabrication must be reported as an unverified
  // claim, not masked by the drift hash that would otherwise fire first.
  validateSurfaces(stored);
  validateRows(stored);
  validateOwners(stored);
  validateCoverage(stored);
  compareLedger(stored, buildLedger());
  const v = stored.summary.verdicts;
  process.stdout.write("core surface ledger: source-derived, verified against " +
    stored.summary.languageCount + " recorded competitor surfaces, and unique\n");
  process.stdout.write("rows=" + stored.summary.rowCount +
    " wins=" + (v.jet_wins || 0) +
    " equal=" + (v.equal || 0) +
    " loses=" + (v.jet_loses || 0) +
    " single-witness=" + (v.single_witness || 0) +
    " not-compared=" + (v.not_compared || 0) +
    " clusters-needing-a-card=" + stored.summary.clustersNeedingCard + "\n");
}

// ---------------------------------------------------------------------------
// Hostile fixtures.
//
// A checker that only ever sees a correct ledger proves nothing: it would pass
// just as happily with every gate deleted. Each fixture below takes the real
// ledger, breaks exactly one thing, and requires the matching gate to reject
// it. A gate that stops firing fails here rather than going quiet in CI.

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

// The fixture must fail for its own reason. Accepting any throw let a broken
// fixture pass on its own setup error: with validateOwners deleted, the stale
// owner fixture still reported success because "no closed cluster to break"
// satisfied the catch. `expected` is the gate's own wording, so a setup error
// now fails loudly instead of forging a green.
function rejects(name, expected, run) {
  let failed = null;
  try {
    run();
  } catch (error) {
    failed = error.message;
  }
  if (!failed) throw new Error("fixture was accepted but must be rejected: " + name);
  if (!failed.includes(expected)) {
    throw new Error("fixture '" + name + "' failed for the wrong reason.\n" +
      "  expected the gate to say: " + expected + "\n" +
      "  actually failed with:     " + failed.split("\n")[0]);
  }
  return name + " -> " + failed.split("\n")[0];
}

// A fixture that cannot be built is a broken fixture, not a passing one.
function must(value, what) {
  if (value === undefined || value === null) {
    throw new Error("cannot build the fixture: " + what);
  }
  return value;
}

function hostileFixtures() {
  const ledger = loadJson(LEDGER_PATH);
  const surfaces = loadSurfaces();
  const board = towerBoard();
  const results = [];

  const jetRow = must(ledger.rows.find(function (row) { return row.verdict === "equal"; }),
    "the ledger has no equal row");
  const lossRow = must(ledger.rows.find(function (row) { return row.verdict === "jet_loses"; }),
    "the ledger has no loss row");

  results.push(rejects("duplicate row id", "duplicate row id:", function () {
    const broken = clone(ledger);
    broken.rows.push(clone(broken.rows[0]));
    validateRows(broken, surfaces);
  }));

  results.push(rejects("fabricated competitor member", "unverified competitor claim in", function () {
    const broken = clone(ledger);
    const row = broken.rows.find(function (item) { return item.id === jetRow.id; });
    const language = must(Object.keys(row.competitors).find(function (key) {
      return row.competitors[key].status === "has";
    }), "the sample equal row has no matched competitor");
    row.competitors[language] = { status: "has", operation: "jet_ledger_operation_that_does_not_exist" };
    validateRows(broken, surfaces);
  }));

  results.push(rejects("equal verdict with no competitor operation",
    "equal verdict with no matching competitor operation:", function () {
    const broken = clone(ledger);
    const row = broken.rows.find(function (item) { return item.id === jetRow.id; });
    for (const key of Object.keys(row.competitors)) {
      row.competitors[key] = { status: "lacks", operation: null };
    }
    validateRows(broken, surfaces);
  }));

  results.push(rejects("unmapped shipped method", "row without a Jet spelling:", function () {
    const broken = clone(ledger);
    broken.rows.find(function (item) { return item.id === jetRow.id; }).jetSpelling = null;
    validateRows(broken, surfaces);
  }));

  results.push(rejects("invalid verdict", "invalid verdict in", function () {
    const broken = clone(ledger);
    broken.rows.find(function (item) { return item.id === jetRow.id; }).verdict = "probably_fine";
    validateRows(broken, surfaces);
  }));

  results.push(rejects("hidden exclusion: a language skips a container",
    "records no verdict for", function () {
    const broken = clone(surfaces);
    delete broken.Rust.surface.containers[ledger.canonicalContainers[0]];
    validateSurfaces(ledger, broken);
  }));

  results.push(rejects("hidden exclusion: absent container with no reason",
    "absent with no reason", function () {
    const broken = clone(surfaces);
    const name = must(Object.keys(broken.Rust.surface.containers).find(function (key) {
      return !broken.Rust.surface.containers[key].present;
    }), "Rust records no absent container");
    delete broken.Rust.surface.containers[name].reason;
    validateSurfaces(ledger, broken);
  }));

  results.push(rejects("a language is dropped from the comparison",
    "the owner named eleven languages", function () {
    const broken = clone(surfaces);
    delete broken.Julia;
    validateSurfaces(ledger, broken);
  }));

  results.push(rejects("surface without recorded provenance",
    "surface without recorded provenance:", function () {
    const broken = clone(surfaces);
    delete broken.Go.surface.scopeRule;
    validateSurfaces(ledger, broken);
  }));

  results.push(rejects("stale owner: cluster claims a closed card", "names closed card #", function () {
    const broken = clone(ledger);
    const cluster = must(broken.lossClusters.find(function (item) {
      return item.ownerState === "closed";
    }), "no closed cluster to break");
    cluster.ownerState = "live";
    validateOwners(broken, board);
  }));

  results.push(rejects("stale owner: cluster claims a card that is not on the board",
    "names missing card #", function () {
    const broken = clone(ledger);
    const cluster = must(broken.lossClusters[0], "the ledger has no loss cluster");
    cluster.ownerState = "live";
    cluster.priorCard = 999999;
    validateOwners(broken, board);
  }));

  results.push(rejects("cluster both names a card and claims to need one",
    "claims to need one", function () {
    const broken = clone(ledger);
    // A cluster is synthesised rather than found: once every cluster has a
    // card, searching the live ledger would fail on setup instead of proving
    // the gate.
    broken.lossClusters.push({
      container: "LedgerFixtureContainer",
      lossCount: 1,
      languages: ["Rust"],
      priorCard: 1404,
      priorCardPhase: "done",
      ownerState: "needs_card",
    });
    validateOwners(broken, board);
  }));

  // The board holds no unratified decision today, so the fixture injects one.
  // Reading the live board would let the fixture pass by finding nothing.
  results.push(rejects("unratified scope exclusion", "unratified scope exclusion in", function () {
    const broken = clone(ledger);
    const openBoard = clone(board);
    openBoard.decisions.push({ id: "D-LEDGER-FIXTURE-1", status: "open" });
    const row = broken.rows.find(function (item) { return item.id === lossRow.id; });
    row.verdict = "declined";
    row.declinedBy = "D-LEDGER-FIXTURE-1";
    validateOwners(broken, openBoard);
  }));

  results.push(rejects("decline naming a decision the board does not have",
    "declined row names an unknown decision:", function () {
    const broken = clone(ledger);
    const row = broken.rows.find(function (item) { return item.id === lossRow.id; });
    row.verdict = "declined";
    row.declinedBy = "D-LEDGER-NO-SUCH-DECISION";
    validateOwners(broken, board);
  }));

  results.push(rejects("decline with no decision id", "declined row without a decision id:", function () {
    const broken = clone(ledger);
    broken.rows.find(function (item) { return item.id === lossRow.id; }).verdict = "declined";
    validateOwners(broken, board);
  }));

  results.push(rejects("hidden uncompared Core domain",
    "uncompared Core domains are not fully listed", function () {
    const broken = clone(ledger);
    must(broken.uncomparedDomains[0], "the ledger lists no uncompared domain");
    broken.uncomparedDomains = broken.uncomparedDomains.slice(1);
    validateCoverage(broken);
  }));

  results.push(rejects("source-surface drift", "core surface ledger drifted", function () {
    const broken = clone(ledger);
    broken.sourceFiles[0].sha256 = "0".repeat(64);
    compareLedger(broken, buildLedger());
  }));

  results.push(rejects("a shipped method is dropped from the ledger",
    "core surface ledger drifted", function () {
    const broken = clone(ledger);
    broken.rows = broken.rows.filter(function (item) { return item.id !== jetRow.id; });
    compareLedger(broken, buildLedger());
  }));

  results.push(rejects("a competitor member is dropped from the ledger",
    "core surface ledger drifted", function () {
    const broken = clone(ledger);
    broken.rows = broken.rows.filter(function (item) { return item.id !== lossRow.id; });
    compareLedger(broken, buildLedger());
  }));

  for (const line of results) process.stdout.write("rejected: " + line + "\n");
  process.stdout.write("core surface ledger: " + results.length + " hostile fixtures all rejected\n");
}

const args = process.argv.slice(2);
try {
  if (args.includes("--refresh")) refresh();
  else if (args.includes("--check")) check();
  else if (args.includes("--hostile-fixtures")) hostileFixtures();
  else throw new Error("usage: check-core-surface-ledger.mjs --refresh|--check|--hostile-fixtures");
} catch (error) {
  process.stderr.write(error.message + "\n");
  process.exitCode = 1;
}
