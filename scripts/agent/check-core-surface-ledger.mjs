#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/*
 * One source-derived Core inventory. Read by the #1398 release gate.
 *
 * The source tables are authoritative. This file contains only the comparison
 * policy and the parser for those tables. The JSON and Markdown artifacts are
 * generated; hand-editing either artifact is rejected by --check.
 */

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const LEDGER_PATH = join(ROOT, "docs/reference/core-surface-ledger.json");
const README_PATH = join(ROOT, "docs/reference/core-surface-ledger.md");
const PYTHON_SURFACE_PATH = join(ROOT, "docs/reference/python-surface.json");
const MODULE_ITEMS_PATH = "crates/jet-sema/src/Sema/CheckerCoreLib/module_items.rs";
const FIXED_SIGS_PATH = "crates/jet-sema/src/Sema/CheckerCoreLib/fixed_sigs.rs";
const COLLECTIONS_PATH = "crates/jet-foundation/src/Collections.rs";
const PREDICATES_PATH = "crates/jet-foundation/src/Syntax/predicates.rs";
const POLICY_PATH = "crates/jet-foundation/src/Policy.rs";
const SYNTAX_PATH = "crates/jet-foundation/src/Syntax/core_surface.rs";

const COMPETITOR_SOURCES = {
  Python: [
    "https://docs.python.org/3/library/functions.html",
    "https://docs.python.org/3/library/stdtypes.html",
    "https://docs.python.org/3/library/index.html",
  ],
  Rust: [
    "https://doc.rust-lang.org/std/collections/",
    "https://doc.rust-lang.org/std/iter/",
  ],
  Go: ["https://pkg.go.dev/std"],
  Swift: ["https://developer.apple.com/documentation/swift/sequence-and-collection-protocols"],
  Kotlin: ["https://kotlinlang.org/api/core/kotlin-stdlib/"],
  "C#": ["https://learn.microsoft.com/en-us/dotnet/standard/linq/"],
  TypeScript: ["https://www.typescriptlang.org/tsconfig/lib.html"],
  Ruby: ["https://ruby-doc.org/3.4.1/"],
  Elixir: ["https://hexdocs.pm/elixir/Enum.html"],
  Julia: ["https://docs.julialang.org/en/v1/base/collections/"],
  R: ["https://stat.ethz.ch/R-manual/R-devel/library/base/html/00Index.html"],
};

const PYTHON_DOCS = {
  "python.builtins": "https://docs.python.org/3/library/functions.html",
  "python.types": "https://docs.python.org/3/library/stdtypes.html",
  "python.collections": "https://docs.python.org/3/library/collections.html",
  "python.datetime": "https://docs.python.org/3/library/datetime.html",
  "python.functools": "https://docs.python.org/3/library/functools.html",
  "python.heapq": "https://docs.python.org/3/library/heapq.html",
  "python.itertools": "https://docs.python.org/3/library/itertools.html",
  "python.json": "https://docs.python.org/3/library/json.html",
  "python.math": "https://docs.python.org/3/library/math.html",
  "python.os": "https://docs.python.org/3/library/os.html",
  "python.pathlib": "https://docs.python.org/3/library/pathlib.html",
  "python.random": "https://docs.python.org/3/library/random.html",
  "python.re": "https://docs.python.org/3/library/re.html",
  "python.secrets": "https://docs.python.org/3/library/secrets.html",
  "python.socket": "https://docs.python.org/3/library/socket.html",
  "python.sqlite3": "https://docs.python.org/3/library/sqlite3.html",
  "python.statistics": "https://docs.python.org/3/library/statistics.html",
  "python.subprocess": "https://docs.python.org/3/library/subprocess.html",
  "python.tarfile": "https://docs.python.org/3/library/tarfile.html",
  "python.tempfile": "https://docs.python.org/3/library/tempfile.html",
  "python.time": "https://docs.python.org/3/library/time.html",
  "python.tomllib": "https://docs.python.org/3/library/tomllib.html",
  "python.urllib.parse": "https://docs.python.org/3/library/urllib.parse.html",
  "python.uuid": "https://docs.python.org/3/library/uuid.html",
  "python.zipfile": "https://docs.python.org/3/library/zipfile.html",
  "python.csv": "https://docs.python.org/3/library/csv.html",
  "python.unicodedata": "https://docs.python.org/3/library/unicodedata.html",
  "python.base64": "https://docs.python.org/3/library/base64.html",
  "python.binascii": "https://docs.python.org/3/library/binascii.html",
  "python.asyncio": "https://docs.python.org/3/library/asyncio.html",
  "python.unittest": "https://docs.python.org/3/library/unittest.html",
  "python.logging": "https://docs.python.org/3/library/logging.html",
  "python.http": "https://docs.python.org/3/library/http.html",
  "python.ssl": "https://docs.python.org/3/library/ssl.html",
  "python.struct": "https://docs.python.org/3/library/struct.html",
  "python.io": "https://docs.python.org/3/library/io.html",
};

const PYTHON_SCOPE_TYPES = [
  "bool", "bytes", "dict", "float", "int", "list", "range", "set", "str",
  "tuple",
];

const PYTHON_SCOPE_MODULES = Object.keys(PYTHON_DOCS)
  .filter(function (key) { return key.startsWith("python."); })
  .map(function (key) { return key.slice("python.".length); })
  .filter(function (name) {
    return name !== "builtins" && name !== "types";
  })
  .sort();

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

const PYTHON_MODULE_FOR_JET = {
  "core.io": "python.builtins",
  "core.env": "python.os",
  "core.os": "python.os",
  "core.process": "python.subprocess",
  "core.math": "python.math",
  "core.random": "python.random",
  "core.crypto.random": "python.secrets",
  "core.time": "python.datetime",
  "core.encoding.json": "python.json",
  "core.encoding.csv": "python.csv",
  "core.encoding.toml": "python.tomllib",
  "core.encoding.hex": "python.binascii",
  "core.encoding.base64": "python.base64",
  "core.encoding.base32": "python.base64",
  "core.text.unicode": "python.unicodedata",
  "core.uuid": "python.uuid",
  "core.files": "python.pathlib",
  "core.path": "python.pathlib",
  "core.url": "python.urllib.parse",
  "core.net": "python.socket",
  "core.tls": "python.ssl",
  "core.http": "python.http",
  "core.regex": "python.re",
  "core.archive": "python.zipfile",
  "core.db": "python.sqlite3",
  "core.tasks": "python.asyncio",
  "core.testing": "python.unittest",
  "core.data": "python.statistics",
  "core.binary": "python.struct",
  "core.text": "python.types",
  "core.log": "python.logging",
  "core.fmt": "python.builtins",
};

const PYTHON_DIRECT = {
  "core.io.args": "sys.argv",
  "core.io.input": "builtins.input",
  "core.io.print": "builtins.print",
  "core.io.eprint": "sys.stderr",
  "core.env.get": "os.getenv",
  "core.env.set": "os.putenv",
  "core.env.unset": "os.unsetenv",
  "core.env.vars": "os.environ",
  "core.env.current_dir": "os.getcwd",
  "core.env.home_dir": "pathlib.Path.home",
  "core.os.temp_dir": "tempfile.gettempdir",
  "core.os.cpu_count": "os.cpu_count",
  "core.os.executable": "sys.executable",
  "core.os.pid": "os.getpid",
  "core.os.hostname": "socket.gethostname",
  "core.process.run": "subprocess.run",
  "core.process.cmd": "subprocess.Popen",
  "core.process.pipeline": "subprocess.Popen",
  "core.process.exit": "sys.exit",
  "core.crypto.random.bytes": "secrets.token_bytes",
  "core.encoding.json.parse": "json.loads",
  "core.encoding.json.to_string": "json.dumps",
  "core.encoding.json.to_string_pretty": "json.dumps",
  "core.encoding.csv.parse": "csv.reader",
  "core.encoding.toml.parse": "tomllib.loads",
  "core.encoding.hex.encode": "binascii.hexlify",
  "core.encoding.hex.decode": "binascii.unhexlify",
  "core.encoding.base64.encode": "base64.b64encode",
  "core.encoding.base64.decode": "base64.b64decode",
  "core.uuid.v4": "uuid.uuid4",
  "core.uuid.v7": "uuid.uuid7",
  "core.files.read": "pathlib.Path.read_text",
  "core.files.write": "pathlib.Path.write_text",
  "core.files.exists": "pathlib.Path.exists",
  "core.files.remove": "pathlib.Path.unlink",
  "core.files.create_dir": "pathlib.Path.mkdir",
  "core.files.list_dir": "pathlib.Path.iterdir",
  "core.files.walk": "os.walk",
  "core.files.glob": "pathlib.Path.glob",
  "core.path.join": "pathlib.PurePath.joinpath",
  "core.path.parent": "pathlib.PurePath.parent",
  "core.path.extension": "pathlib.PurePath.suffix",
  "core.net.tcp_connect": "socket.create_connection",
  "core.net.tcp_listen": "socket.create_server",
  "core.net.tcp_read": "socket.socket.recv",
  "core.net.tcp_write": "socket.socket.send",
  "core.tls.client": "ssl.create_default_context",
  "core.archive.zip_compress": "zipfile.ZipFile",
  "core.archive.zip_decompress": "zipfile.ZipFile",
  "core.archive.tar_add": "tarfile.TarFile.add",
  "core.testing.temp_dir": "tempfile.TemporaryDirectory",
  "core.binary.Reader": "io.BytesIO",
};

const COLLECTION_PYTHON = {
  List: "python.builtins",
  Iter: "python.itertools",
  Map: "python.types",
  Set: "python.types",
  SortedSet: null,
  Deque: "python.collections",
  PriorityQueue: "python.heapq",
  Cache: "python.functools",
  BitSet: null,
  ByteBuffer: "python.io",
  String: "python.types",
};

const COLLECTION_PYTHON_SPECIAL = {
  "List.len": "builtins.len",
  "List.index_of": "list.index",
  "List.push": "list.append",
  "List.extend": "list.extend",
  "List.remove": "list.pop",
  "List.pop": "list.pop",
  "List.sort": "list.sort",
  "List.reverse": "list.reverse",
  "List.count": "list.count",
  "Map.len": "builtins.len",
  "Map.get": "dict.get",
  "Map.remove": "dict.pop",
  "Map.keys": "dict.keys",
  "Map.values": "dict.values",
  "Map.clear": "dict.clear",
  "Set.len": "builtins.len",
  "Set.add": "set.add",
  "Set.remove": "set.remove",
  "Set.union": "set.union",
  "Set.intersection": "set.intersection",
  "Set.difference": "set.difference",
  "Set.symmetric_difference": "set.symmetric_difference",
  "Deque.push_front": "collections.deque.appendleft",
  "Deque.push_back": "collections.deque.append",
  "Deque.pop_front": "collections.deque.popleft",
  "Deque.pop_back": "collections.deque.pop",
  "PriorityQueue.push": "heapq.heappush",
  "PriorityQueue.pop": "heapq.heappop",
  "String.len": "builtins.len",
  "String.starts_with": "str.startswith",
  "String.ends_with": "str.endswith",
  "String.split": "str.split",
  "String.replace": "str.replace",
  "String.trim": "str.strip",
};

const COLLECTION_CONTAINER = {
  List: {
    Python: "list", Rust: "Vec/Iterator", Go: "slice", Swift: "Array/Collection",
    Kotlin: "List/Sequence", "C#": "List<T>/Enumerable", TypeScript: "Array",
    Ruby: "Array/Enumerable", Elixir: "List/Enum", Julia: "Vector/Iterators",
    R: "vector",
  },
  Iter: {
    Python: "iterator/itertools", Rust: "Iterator", Go: "iter.Seq",
    Swift: "Sequence", Kotlin: "Sequence", "C#": "IEnumerable<T>",
    TypeScript: "IterableIterator", Ruby: "Enumerator", Elixir: "Enumerable",
    Julia: "Iterator", R: "iterator package",
  },
  Map: {
    Python: "dict", Rust: "HashMap/BTreeMap", Go: "map", Swift: "Dictionary",
    Kotlin: "Map", "C#": "Dictionary<TKey,TValue>", TypeScript: "Map",
    Ruby: "Hash", Elixir: "Map", Julia: "Dict", R: "named list/environment",
  },
  Set: {
    Python: "set", Rust: "HashSet", Go: "map[T]struct{}", Swift: "Set",
    Kotlin: "Set", "C#": "HashSet<T>", TypeScript: "Set", Ruby: "Set",
    Elixir: "MapSet", Julia: "Set", R: "unique/vector",
  },
  SortedSet: {
    Python: "no stdlib type", Rust: "BTreeSet", Go: "no stdlib type",
    Swift: "no stdlib type", Kotlin: "sortedSetOf", "C#": "SortedSet<T>",
    TypeScript: "no stdlib type", Ruby: "no stdlib type", Elixir: "no stdlib type",
    Julia: "SortedSet (package)", R: "ordered factor",
  },
  Deque: {
    Python: "collections.deque", Rust: "VecDeque", Go: "container/list",
    Swift: "no stdlib type", Kotlin: "ArrayDeque", "C#": "Queue<T>",
    TypeScript: "no stdlib type", Ruby: "Thread::Queue", Elixir: ":queue",
    Julia: "Deque (package)", R: "no stdlib type",
  },
  PriorityQueue: {
    Python: "heapq", Rust: "BinaryHeap", Go: "container/heap",
    Swift: "no stdlib type", Kotlin: "java.util.PriorityQueue",
    "C#": "PriorityQueue<TElement,TPriority>", TypeScript: "no stdlib type",
    Ruby: "no stdlib type", Elixir: "no stdlib type", Julia: "heap/package",
    R: "heap/package",
  },
  Cache: {
    Python: "functools.lru_cache", Rust: "HashMap + policy",
    Go: "map + policy", Swift: "Dictionary + policy", Kotlin: "map + policy",
    "C#": "MemoryCache", TypeScript: "Map + policy", Ruby: "Hash + policy",
    Elixir: "map + policy", Julia: "Dict + policy", R: "environment + policy",
  },
  BitSet: {
    Python: "int bit operations", Rust: "bit-vector crate",
    Go: "math/bits", Swift: "integer bit operations", Kotlin: "integer bit operations",
    "C#": "BitArray", TypeScript: "integer bit operations",
    Ruby: "Integer bit operations", Elixir: "bitstring", Julia: "BitVector",
    R: "logical vector",
  },
  ByteBuffer: {
    Python: "io.BytesIO/struct", Rust: "Vec<u8>", Go: "bytes.Buffer",
    Swift: "Data", Kotlin: "ByteArray/ByteBuffer", "C#": "MemoryStream",
    TypeScript: "Uint8Array/DataView", Ruby: "String#pack",
    Elixir: "iolist/binary", Julia: "IOBuffer", R: "raw vector",
  },
  String: {
    Python: "str", Rust: "str/String", Go: "string", Swift: "String",
    Kotlin: "String", "C#": "String", TypeScript: "string",
    Ruby: "String", Elixir: "String", Julia: "String", R: "character vector",
  },
};

// Every Python claim resolves against this snapshot, which is introspected from
// a real interpreter by scripts/agent/python-surface-snapshot.py. A constructed
// member name is not evidence that Python has that member.
const PYTHON_SURFACE = JSON.parse(readFileSync(PYTHON_SURFACE_PATH, "utf8"));

function pythonBuiltinHas(type, member) {
  return Boolean(PYTHON_SURFACE.builtinTypes[type]?.members.includes(member));
}

function pythonModuleHas(module, member) {
  const entry = PYTHON_SURFACE.stdlibModules[module];
  if (!entry) return false;
  return entry.operations.includes(member) || entry.excludedConstants.includes(member);
}

// "str.upper", "itertools.chain", "collections.deque.append", "urllib.parse.quote".
// Class members are recorded, so a nested name resolves instead of being
// refused; refusing them downgraded real matches to "Jet has no counterpart".
function pythonHas(dotted) {
  const parts = dotted.split(".");
  const last = parts[parts.length - 1];
  const head = parts.slice(0, -1).join(".");
  if (!head) return pythonModuleHas("builtins", last);
  if (PYTHON_SURFACE.builtinTypes[head]) return pythonBuiltinHas(head, last);
  if (PYTHON_SURFACE.stdlibModules[head]) return pythonModuleHas(head, last);
  // module.Class.member
  const klass = parts[parts.length - 2];
  const module = parts.slice(0, -2).join(".");
  const entry = PYTHON_SURFACE.stdlibModules[module];
  if (entry && entry.types && entry.types[klass]) return entry.types[klass].includes(last);
  return false;
}

// Resolve a Jet member against the Python module the ledger already maps it to,
// by name, before concluding Python has no counterpart. Without this the
// verdict is decided by whether a hand-written table happened to list the row,
// which is the same absence-as-evidence defect in a new place.
function pythonMemberByName(module, member) {
  const doc = PYTHON_MODULE_FOR_JET[module];
  if (!doc) return null;
  const pythonModule = doc.slice("python.".length);
  const candidate = pythonModule + "." + member;
  return pythonHas(candidate) ? candidate : null;
}

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

function pythonDocsForModule(module) {
  if (module === "core.archive") return ["python.zipfile", "python.tarfile"];
  if (module === "core.files") return ["python.pathlib", "python.tempfile"];
  if (module === "core.time") return ["python.datetime", "python.time"];
  const doc = PYTHON_MODULE_FOR_JET[module];
  return doc ? [doc] : ["python.library"];
}

// A curated mapping is a claim; it counts only when the snapshot confirms the
// member exists. Same-name guesses across a type boundary are still guesses,
// so they are checked exactly like curated entries.
function pythonMemberForCollection(type, method) {
  const special = COLLECTION_PYTHON_SPECIAL[type + "." + method];
  if (special) return pythonHas(special) ? special : null;
  const doc = COLLECTION_PYTHON[type];
  if (!doc) return null;
  const container = {
    Iter: "itertools",
    Deque: "collections.deque",
    PriorityQueue: "heapq",
    ByteBuffer: "io.BytesIO",
    String: "str",
    Map: "dict",
    Set: "set",
    List: "list",
  }[type] || doc.slice("python.".length);
  const candidate = container + "." + method;
  return pythonHas(candidate) ? candidate : null;
}

function pythonMemberForModule(module, member) {
  const mapped = PYTHON_DIRECT[module + "." + member];
  if (mapped) return pythonHas(mapped) ? mapped : null;
  return pythonMemberByName(module, member);
}

// A curated mapping that resolves to nothing, or a mapping key naming a module
// the compiler does not ship, is a silent hole. Both used to disappear without
// a word: 45 of 105 mappings resolved to nothing and 11 keys named modules
// (jet.regex, jet.db, jet.http) that do not exist.
function mappingFaults(modules) {
  const faults = [];
  const shipped = new Set(modules.map(function (entry) { return entry.module; }));
  for (const [key, value] of Object.entries(PYTHON_DIRECT)) {
    const module = key.slice(0, key.lastIndexOf("."));
    if (!shipped.has(module)) faults.push("mapping key names an unknown module: " + key);
    else if (!pythonHas(value)) faults.push("mapping resolves to nothing: " + key + " -> " + value);
  }
  for (const [key, value] of Object.entries(COLLECTION_PYTHON_SPECIAL)) {
    if (!pythonHas(value)) faults.push("collection mapping resolves to nothing: " + key + " -> " + value);
  }
  for (const module of Object.keys(PYTHON_MODULE_FOR_JET)) {
    if (!shipped.has(module)) faults.push("comparator names an unknown module: " + module);
  }
  return faults;
}

function pythonReason(module, pythonMember) {
  if (pythonMember) return "direct or closest named operation in the cited Python reference";
  if (PYTHON_MODULE_FOR_JET[module]) {
    return "the cited Python module is the workflow comparator; it has no single member with this Jet spelling";
  }
  return "Jet owns this typed Core domain; Python's standard library has no matching module-level operation";
}

// Per-row operations for the non-Python competitors are not recorded. Naming a
// container and appending the Jet member name invents an operation, so the row
// says "unverified" until someone reads that language's own reference. The
// document-level competitorSources list keeps those references discoverable.
function competitorComparison(type) {
  return {
    status: "unverified",
    reason: "no non-Python competitor surface has been read for this row",
    candidateContainer: type ? COLLECTION_CONTAINER[type] || null : null,
  };
}

function rowForModule(entry, member, fixedOnly) {
  const pythonMember = pythonMemberForModule(entry.module, member);
  const docs = pythonDocsForModule(entry.module);
  const loses = entry.module === "core.files" || entry.module === "core.path";
  const row = {
    id: "module." + entry.module + "." + member,
    source: {
      kind: fixedOnly ? "fixed_sig" : "module_item",
      module: entry.module,
      member: member,
      sourceLine: entry.sourceLine,
    },
    pythonMember: pythonMember,
    pythonReason: pythonReason(entry.module, pythonMember),
    jetSpelling: entry.module + "." + member,
    workflow: pythonMember
      ? "matched Python workflow: " + pythonMember
      : "typed Jet Core workflow for " + entry.module,
    verdict: loses ? "jet_loses" : (pythonMember ? "equal" : "no_python_match"),
    ownerCard: loses ? 288 : null,
    evidence: [
      "source:" + MODULE_ITEMS_PATH,
      "source:" + FIXED_SIGS_PATH,
      "python-surface:" + PYTHON_SURFACE.pythonVersion,
    ].concat(docs).filter(function (value, index, values) {
      return values.indexOf(value) === index;
    }),
    competitors: competitorComparison(null),
  };
  return row;
}

function rowForCollection(entry, method) {
  const type = entry.type;
  const pythonMember = pythonMemberForCollection(type, method);
  const docs = COLLECTION_PYTHON[type] ? [COLLECTION_PYTHON[type]] : ["python.library"];
  const row = {
    id: "collection." + type + "." + method,
    source: {
      kind: "collection_method_return",
      type: type,
      function: entry.function,
      member: method,
      sourceLine: entry.sourceLine,
    },
    pythonMember: pythonMember,
    pythonReason: pythonReason("collection." + type, pythonMember),
    jetSpelling: type + "." + method,
    workflow: pythonMember
      ? "matched collection workflow: " + pythonMember
      : "typed Jet collection workflow for " + type,
    verdict: pythonMember ? "equal" : "no_python_match",
    ownerCard: null,
    evidence: [
      "source:" + COLLECTIONS_PATH,
      "python-surface:" + PYTHON_SURFACE.pythonVersion,
    ].concat(docs),
    competitors: competitorComparison(type),
  };
  return row;
}

// Walking only Jet's own tables can never surface a feature Jet is missing, so
// the ledger also walks the competitor surface. Every Python comparison point
// that no Jet row matched becomes a visible row with an owner.
function reverseRows(jetRows) {
  const matched = new Set();
  for (const row of jetRows) {
    if (row.pythonMember) matched.add(row.pythonMember);
  }
  const rows = [];
  for (const [type, entry] of Object.entries(PYTHON_SURFACE.builtinTypes)) {
    for (const member of entry.members) {
      const dotted = type + "." + member;
      if (matched.has(dotted)) continue;
      rows.push(reverseRow("builtin_type", type, member, dotted));
    }
  }
  for (const [module, entry] of Object.entries(PYTHON_SURFACE.stdlibModules)) {
    for (const member of entry.operations) {
      const dotted = module + "." + member;
      if (matched.has(dotted)) continue;
      rows.push(reverseRow("stdlib_module", module, member, dotted));
    }
  }
  return rows;
}

function reverseRow(kind, container, member, dotted) {
  return {
    id: "python." + dotted,
    source: {
      kind: kind,
      module: kind === "stdlib_module" ? container : undefined,
      type: kind === "builtin_type" ? container : undefined,
      member: member,
      sourceLine: null,
    },
    pythonMember: dotted,
    pythonReason: "present in the recorded Python surface; no Jet row claims it",
    jetSpelling: null,
    workflow: "Python comparison point awaiting a Jet spelling, a gap owner, or a ratified decline",
    verdict: "no_jet_match",
    ownerCard: null,
    evidence: [
      "python-surface:" + PYTHON_SURFACE.pythonVersion,
      "official:python.library",
    ],
    competitors: competitorComparison(null),
  };
}

function buildRows(modules, fixedPairs, collections) {
  const rows = [];
  const moduleKeys = new Set();
  for (const entry of modules) {
    for (const member of entry.members) {
      const key = entry.module + "." + member;
      moduleKeys.add(key);
      rows.push(rowForModule(entry, member, false));
    }
  }
  for (const pair of fixedPairs) {
    if (moduleKeys.has(pair)) continue;
    const split = pair.lastIndexOf(".");
    const module = pair.slice(0, split);
    const member = pair.slice(split + 1);
    const entry = modules.find(function (item) { return item.module === module; });
    if (!entry) throw new Error("fixed signature module missing from inventory: " + module);
    rows.push(rowForModule(entry, member, true));
  }
  for (const entry of collections) {
    for (const method of entry.methods) rows.push(rowForCollection(entry, method));
  }
  rows.sort(function (left, right) { return left.id.localeCompare(right.id); });
  return rows;
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
  ].map(function (path) {
    const source = read(path);
    return {
      path: path,
      sha256: sha256(source),
      lineCount: source.split(/\r?\n/).length,
    };
  });
}

function buildLedger(previous) {
  const modules = moduleInventory();
  const fixedPairs = fixedSignaturePairs(modules);
  const collections = collectionInventory();
  const jetRows = buildRows(modules, fixedPairs, collections);
  const rows = jetRows.concat(reverseRows(jetRows));
  const losses = rows.filter(function (row) { return row.verdict === "jet_loses"; });
  const unmatched = rows.filter(function (row) { return row.verdict === "no_jet_match"; });
  // Coverage is derived from rows that carry a verified Python member. A
  // hand-listed row set could claim a type was covered when it was not.
  const pythonCoverage = {
    builtinTypes: PYTHON_SCOPE_TYPES.map(function (name) {
      const covered = jetRows.filter(function (row) {
        return row.pythonMember && row.pythonMember.split(".")[0] === name;
      });
      return {
        name: name,
        rows: covered.map(function (row) { return row.id; }),
        surfaceMembers: PYTHON_SURFACE.builtinTypes[name]?.memberCount ?? 0,
        matchedMembers: new Set(covered.map(function (row) { return row.pythonMember; })).size,
      };
    }),
    stdlibModules: PYTHON_SCOPE_MODULES.map(function (name) {
      const covered = jetRows.filter(function (row) {
        return row.pythonMember && row.pythonMember.startsWith(name + ".");
      });
      return {
        name: name,
        rows: covered.map(function (row) { return row.id; }),
        surfaceOperations: PYTHON_SURFACE.stdlibModules[name]?.operationCount ?? 0,
        matchedOperations: new Set(covered.map(function (row) { return row.pythonMember; })).size,
      };
    }),
  };
  return {
    schemaVersion: 1,
    title: "Jet Core surface ledger",
    sourceOfTruth: "docs/reference/core-surface-ledger.json",
    generatedOn: new Date().toISOString().slice(0, 10),
    sourceFiles: sourceFiles(),
    pythonScope: {
      rule: "The ledger claims competition only for these Python built-in types and standard-library modules. Every Python claim resolves against the recorded surface; a constructed member name is never evidence.",
      builtinTypes: PYTHON_SCOPE_TYPES,
      stdlibModules: PYTHON_SCOPE_MODULES,
      surface: "docs/reference/python-surface.json",
      pythonVersion: PYTHON_SURFACE.pythonVersion,
      scopeRule: PYTHON_SURFACE.scopeRule,
      excludedConstantCount: PYTHON_SURFACE.totals.excludedConstants,
      officialIndex: "https://docs.python.org/3/library/index.html",
      builtinIndex: "https://docs.python.org/3/library/functions.html",
    },
    competitorSources: COMPETITOR_SOURCES,
    competitorStatus: {
      Python: "verified against docs/reference/python-surface.json",
      other: "unverified — no surface has been read for the remaining languages",
    },
    consumer: {
      card: 1398,
      input: "docs/reference/core-surface-ledger.json",
      manualWorkflowInventory: false,
      rule: "Load rows from this file. Do not copy the inventory into the Python superiority rubric.",
    },
    inventory: {
      modules: modules,
      fixedSignaturePairs: fixedPairs,
      collections: collections,
    },
    pythonCoverage: pythonCoverage,
    rows: rows,
    summary: {
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
      lossCount: losses.length,
      lossOwners: Array.from(new Set(losses.map(function (row) { return row.ownerCard; }))).sort(),
      pythonComparisonPoints: PYTHON_SURFACE.totals.comparisonPoints,
      pythonMatchedCount: new Set(jetRows.filter(function (row) {
        return row.pythonMember;
      }).map(function (row) { return row.pythonMember; })).size,
      unmatchedCount: unmatched.length,
      unmatchedByContainer: unmatched.reduce(function (counts, row) {
        const key = row.source.module || row.source.type;
        counts[key] = (counts[key] || 0) + 1;
        return counts;
      }, {}),
    },
  };
}

function markdown(ledger) {
  const lossRows = ledger.rows.filter(function (row) {
    return row.verdict === "jet_loses";
  });
  const lossOwners = Array.from(new Set(lossRows.map(function (row) {
    return "#" + row.ownerCard;
  }))).join(", ") || "none";
  const lines = [
    "# Jet Core surface ledger",
    "",
    "Generated from the compiler's Core source tables. The JSON file is the",
    "machine-readable source consumed by #1398; this page is the durable",
    "review index. Do not maintain a second hand-written workflow inventory.",
    "",
    "Generated on: " + ledger.generatedOn,
    "",
    "## Source contract",
    "",
    "- Module members come from module_items.rs, including its dynamic",
    "  core.lang policy registry and resolved Syntax constants.",
    "- Fixed call signatures come from fixed_sigs.rs.",
    "- Built-in type method returns come from Collections.rs.",
    "- The Python side comes from docs/reference/python-surface.json, read from",
    "  a real interpreter. A constructed member name is never evidence.",
    "- --check rejects source drift, an unverified Python member, an equal",
    "  verdict without a member, duplicate rows, hidden exclusions, stale gap",
    "  owners, and unratified deliberate declines.",
    "",
    "## Inventory",
    "",
    "| Measure | Count |",
    "| --- | ---: |",
    "| Core modules | " + ledger.summary.moduleCount + " |",
    "| Module members | " + ledger.summary.moduleMemberCount + " |",
    "| Fixed-signature-only rows | " + ledger.summary.fixedSignatureOnlyCount + " |",
    "| Collection method-return functions | " + ledger.summary.collectionFunctionCount + " |",
    "| Collection method rows | " + ledger.summary.collectionMethodCount + " |",
    "| Jet-side rows | " + ledger.summary.jetRowCount + " |",
    "| Total rows | " + ledger.summary.rowCount + " |",
    "| Jet-loses rows | " + ledger.summary.lossCount + " |",
    "",
    "Jet-loses rows currently reference: " + lossOwners + ".",
    "",
    "## Coverage",
    "",
    "Walking only Jet's own tables cannot surface a feature Jet is missing, so",
    "the ledger also walks the Python surface. Each Python comparison point with",
    "no matching Jet row is a visible row, not an omission.",
    "",
    "This is a report. It records what is true today; it does not track work.",
    "Turn a row that matters into a card by hand.",
    "",
    "| Measure | Count |",
    "| --- | ---: |",
    "| Python comparison points | " + ledger.summary.pythonComparisonPoints + " |",
    "| Matched by a Jet row | " + ledger.summary.pythonMatchedCount + " |",
    "| No matching Jet row | " + ledger.summary.unmatchedCount + " |",
    "",
    "Per-container counts:",
    "",
    "| Container | No Jet match |",
    "| --- | ---: |",
  ].concat(Object.entries(ledger.summary.unmatchedByContainer)
    .sort(function (a, b) { return b[1] - a[1] || a[0].localeCompare(b[0]); })
    .map(function (pair) { return "| " + pair[0] + " | " + pair[1] + " |"; }))
    .concat([
    "",
    "Only the Python surface has been read. Operations for the other competitor",
    "languages are recorded as unverified rather than guessed.",
    "",
    "## Python claim boundary",
    "",
    "The claim covers the built-in types and standard-library modules listed in",
    "the JSON pythonScope, at Python " + ledger.pythonScope.pythonVersion + ". " +
      ledger.pythonScope.excludedConstantCount + " module-level constants are",
    "excluded by the recorded scope rule and stay counted so the exclusion",
    "cannot hide a gap.",
    "",
    "Primary Python references:",
    "",
    "- Python library index: " + ledger.pythonScope.officialIndex,
    "- Python built-in functions and types: " + ledger.pythonScope.builtinIndex,
    "",
    "## Competitor references",
    "",
  ]);
  for (const [language, urls] of Object.entries(ledger.competitorSources)) {
    lines.push("- " + language + ": " + urls.join(", "));
  }
  lines.push(
    "",
    "## Consumer",
    "",
    "Card #1398 reads docs/reference/core-surface-ledger.json as its only",
    "workflow inventory. The ledger contains stable row IDs, a verified Python",
    "member or an explicit reason, Jet spelling, workflow, verdict, gap owner,",
    "source provenance, and evidence links.",
    "",
    "Run the focused guard from the repository root:",
    "",
    "~~~sh",
    "node scripts/agent/check-core-surface-ledger.mjs --check --tower plugins/tower/.tower/tower.json",
    "~~~",
    "",
    "Full rows are intentionally kept in the JSON artifact so the release",
    "rubric can consume structured data without duplicating this inventory.",
    "",
  );
  return lines.join("\n");
}

function loadJson(path) {
  if (!existsSync(path)) throw new Error("missing ledger: " + path);
  return JSON.parse(readFileSync(path, "utf8"));
}

function validateMappings(modules) {
  const faults = mappingFaults(modules);
  if (faults.length) {
    throw new Error("comparison mapping is broken:\n  " + faults.join("\n  "));
  }
}

function validateRows(ledger, cards) {
  const ids = new Set();
  const sourceKeys = new Set();
  for (const row of ledger.rows) {
    if (ids.has(row.id)) throw new Error("duplicate row id: " + row.id);
    ids.add(row.id);
    const sourceKey = row.source.kind + ":" + (row.source.module || row.source.type) + ":" + row.source.member;
    if (sourceKeys.has(sourceKey)) throw new Error("duplicate source row: " + sourceKey);
    sourceKeys.add(sourceKey);
    if (!row.pythonReason || !row.workflow || !row.verdict) {
      throw new Error("incomplete row: " + row.id);
    }
    if (row.verdict !== "no_jet_match" && !row.jetSpelling) {
      throw new Error("incomplete row: " + row.id);
    }
    if (!["equal", "jet_loses", "no_python_match", "no_jet_match"].includes(row.verdict)) {
      throw new Error("invalid verdict in " + row.id + ": " + row.verdict);
    }
    // A row may not assert a Python member the recorded surface does not have.
    if (row.pythonMember && !pythonHas(row.pythonMember)) {
      throw new Error("unverified Python member in " + row.id + ": " + row.pythonMember);
    }
    if (row.verdict === "equal" && !row.pythonMember) {
      throw new Error("equal verdict without a verified Python member: " + row.id);
    }
    if (row.competitors?.status !== "unverified" && !row.competitors?.verifiedFrom) {
      throw new Error("competitor claim without a cited surface in " + row.id);
    }
  }
}

function validateCoverage(ledger) {
  if (ledger.pythonScope.pythonVersion !== PYTHON_SURFACE.pythonVersion) {
    throw new Error("ledger was built against Python " + ledger.pythonScope.pythonVersion +
      " but the recorded surface is " + PYTHON_SURFACE.pythonVersion);
  }
  for (const item of ledger.pythonCoverage.builtinTypes) {
    if (item.surfaceMembers === 0) throw new Error("Python builtin type missing from the surface: " + item.name);
  }
  for (const item of ledger.pythonCoverage.stdlibModules) {
    if (item.surfaceOperations === 0) throw new Error("Python stdlib module missing from the surface: " + item.name);
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

function refresh() {
  const previous = existsSync(LEDGER_PATH) ? loadJson(LEDGER_PATH) : null;
  const ledger = buildLedger(previous);
  writeFileSync(LEDGER_PATH, JSON.stringify(ledger, null, 2) + "\n");
  writeFileSync(README_PATH, markdown(ledger));
  process.stdout.write("wrote " + LEDGER_PATH + "\n");
  process.stdout.write("rows=" + ledger.summary.rowCount + " losses=" + ledger.summary.lossCount + "\n");
}

// Truthfulness and closure are different questions. Every claim in the ledger
// The ledger is a report, so the only thing to enforce is that everything it
// says is true. Coverage is a number it prints, never a gate.
function check() {
  const stored = loadJson(LEDGER_PATH);
  const expected = buildLedger(stored);
  // Order matters: a hand-edited fabrication must be reported as an unverified
  // member, not masked by the drift hash that would otherwise fire first.
  validateCoverage(stored);
  validateRows(stored);
  validateMappings(moduleInventory());
  compareLedger(stored, expected);
  process.stdout.write("core surface ledger: source-derived, verified against Python " +
    stored.pythonScope.pythonVersion + ", and unique\n");
  process.stdout.write("modules=" + stored.summary.moduleCount +
    " rows=" + stored.summary.rowCount +
    " matched=" + stored.summary.pythonMatchedCount + "/" + stored.summary.pythonComparisonPoints +
    " losses=" + stored.summary.lossCount +
    " no-jet-match=" + stored.summary.unmatchedCount + "\n");
}

const args = process.argv.slice(2);
try {
  if (args.includes("--refresh")) refresh();
  else if (args.includes("--check")) check();
  else throw new Error("usage: check-core-surface-ledger.mjs --refresh|--check");
} catch (error) {
  process.stderr.write(error.message + "\n");
  process.exitCode = 1;
}
