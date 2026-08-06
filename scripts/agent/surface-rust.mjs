#!/usr/bin/env node
/*
 * Emit the Rust comparison surface for the Core surface ledger.
 *
 * The primary record is the standard-library source itself, so every "Rust has
 * this" claim is checkable against the shipped code rather than from memory.
 * Only `#[stable]` items count: an unstable API is not something Rust ships to
 * a user on the stable channel, so counting it would overstate Rust's surface.
 *
 * Regenerate:
 *   nix build --no-link --print-out-paths nixpkgs#rustPlatform.rustcSrc
 *   node scripts/agent/surface-rust.mjs <that-path>/library <rust version> \
 *       > docs/reference/surfaces/rust-surface.json
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

const library = process.argv[2];
const version = process.argv[3];
if (!library || !version) throw new Error("usage: surface-rust.mjs <library-dir> <version>");

// Canonical Jet-facing container names map to the standard-library files that
// define the same workflow. Scoping by file keeps the attribution honest: a
// file's inherent impls are that container's operations. A container Rust does
// not ship is recorded as absent, never omitted, so the gap stays countable.
const CONTAINERS = {
  List: ["alloc/src/vec/mod.rs", "core/src/slice/mod.rs"],
  Iter: ["core/src/iter/traits/iterator.rs"],
  Map: ["std/src/collections/hash/map.rs", "alloc/src/collections/btree/map.rs"],
  Set: ["std/src/collections/hash/set.rs"],
  SortedSet: ["alloc/src/collections/btree/set.rs"],
  Deque: ["alloc/src/collections/vec_deque/mod.rs"],
  PriorityQueue: ["alloc/src/collections/binary_heap/mod.rs"],
  String: ["alloc/src/string.rs", "core/src/str/mod.rs", "alloc/src/str.rs"],
  ByteBuffer: ["std/src/io/cursor.rs"],
  "core.math": ["core/src/num/f64.rs", "core/src/num/int_macros.rs"],
  "core.time": ["core/src/time.rs", "std/src/time.rs"],
  "core.files": ["std/src/fs.rs"],
  "core.path": ["std/src/path.rs"],
  "core.env": ["std/src/env.rs"],
  "core.os": ["std/src/env.rs"],
  "core.process": ["std/src/process.rs"],
  "core.net": ["std/src/net/tcp.rs", "std/src/net/udp.rs"],
  "core.io": ["std/src/io/mod.rs"],
  "core.binary": ["std/src/io/mod.rs", "core/src/num/int_macros.rs"],
  "core.tasks": ["std/src/thread/mod.rs", "std/src/sync/mpsc.rs"],
  "core.fmt": ["core/src/fmt/mod.rs"],
  "core.text": ["core/src/str/mod.rs", "alloc/src/str.rs", "core/src/char/methods.rs"],
};

const ABSENT = {
  BitSet: "no Rust standard-library bit set; integers carry bit operations",
  Cache: "no Rust standard-library cache with an eviction policy",
  "core.random": "no Rust standard-library random number generator",
  "core.crypto.random": "no Rust standard-library cryptographic random source",
  "core.crypto": "no Rust standard-library cryptography",
  "core.encoding.json": "no Rust standard-library JSON codec",
  "core.encoding.csv": "no Rust standard-library CSV codec",
  "core.encoding.toml": "no Rust standard-library TOML decoder",
  "core.encoding.base64": "no Rust standard-library base64 codec",
  "core.encoding.base32": "no Rust standard-library base32 codec",
  "core.encoding.hex": "no Rust standard-library hex codec",
  "core.regex": "no Rust standard-library regular-expression engine",
  "core.url": "no Rust standard-library URL parser",
  "core.tls": "no Rust standard-library TLS client",
  "core.http": "no Rust standard-library HTTP client or server",
  "core.uuid": "no Rust standard-library UUID generator",
  "core.db": "no Rust standard-library database client",
  "core.log": "no Rust standard-library logging facade",
  "core.archive": "no Rust standard-library archive or compression codec",
  "core.data": "no Rust standard-library statistics",
  "core.testing": "the test harness is built in, but Rust ships no assertion or fixture library",
  "core.text.unicode": "no Rust standard-library Unicode property database",
};

// A `pub fn` counts only when the attributes attached to it mark it stable.
// `#[unstable]` and `rustc_const_unstable` items are not shipped to stable
// users. They stay counted in unstableOperations so the exclusion is visible.
function scan(text) {
  const stable = new Set();
  const unstable = new Set();
  const lines = text.split("\n");
  let attributes = [];
  for (const raw of lines) {
    const line = raw.trim();
    if (line.startsWith("#[") || line.startsWith("#![")) {
      attributes.push(line);
      continue;
    }
    if (line.startsWith("//") || line.length === 0) continue;
    // `pub` is optional because a trait's own methods carry no visibility of
    // their own; Iterator's surface lives entirely inside `pub trait Iterator`.
    // Requiring an attached `#[stable(...)]` keeps private helpers out.
    const fn = /^(?:pub )?(?:const )?(?:async )?(?:unsafe )?(?:extern "[^"]*" )?fn ([a-z_][A-Za-z0-9_]*)/.exec(line);
    if (fn) {
      const joined = attributes.join(" ");
      const isUnstable = joined.includes("#[unstable(") || joined.includes("rustc_const_unstable");
      const isStable = joined.includes("#[stable(");
      if (isStable && !isUnstable) stable.add(fn[1]);
      else if (isUnstable) unstable.add(fn[1]);
    }
    attributes = [];
  }
  for (const name of stable) unstable.delete(name);
  return { stable, unstable };
}

const containers = {};
let operationCount = 0;
let unstableCount = 0;
for (const [name, files] of Object.entries(CONTAINERS)) {
  const stable = new Set();
  const unstable = new Set();
  for (const file of files) {
    const found = scan(readFileSync(join(library, file), "utf8"));
    for (const key of found.stable) stable.add(key);
    for (const key of found.unstable) unstable.add(key);
  }
  for (const key of stable) unstable.delete(key);
  if (stable.size === 0) throw new Error("no stable operations found for container " + name);
  containers[name] = {
    present: true,
    files: files,
    operations: Array.from(stable).sort(),
    unstableOperations: Array.from(unstable).sort(),
  };
  operationCount += stable.size;
  unstableCount += unstable.size;
}
for (const [name, reason] of Object.entries(ABSENT)) {
  containers[name] = { present: false, reason: reason, operations: [], unstableOperations: [] };
}

process.stdout.write(JSON.stringify({
  language: "Rust",
  sourceKind: "standard-library source (rust-src component)",
  runtime: version,
  scopeRule: "Public functions defined in the standard-library files that hold each workflow, counted only when their own attributes mark them #[stable]. Unstable items are not shipped on the stable channel; they stay counted in unstableOperations so the exclusion cannot hide a gap.",
  officialReferences: [
    "https://doc.rust-lang.org/std/",
    "https://doc.rust-lang.org/std/collections/",
    "https://doc.rust-lang.org/std/iter/",
  ],
  containers: containers,
  totals: {
    containers: Object.keys(containers).length,
    presentContainers: Object.values(containers).filter((c) => c.present).length,
    operations: operationCount,
    unstableOperations: unstableCount,
  },
}, null, 2) + "\n");
