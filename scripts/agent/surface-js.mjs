#!/usr/bin/env node
/*
 * Emit the JavaScript/TypeScript comparison surface for the Core surface ledger.
 *
 * TypeScript adds no runtime members; its `lib.es*.d.ts` files describe exactly
 * these ECMAScript objects. Reading the live engine is therefore the primary
 * record for both languages, and every "TypeScript has this" claim is checkable.
 *
 * Regenerate:
 *   node scripts/agent/surface-js.mjs > docs/reference/surfaces/js-surface.json
 */

// Canonical Jet-facing container names map to the ECMAScript objects that hold
// the same workflow. A container with no ECMAScript counterpart is recorded as
// absent rather than omitted, so the gap stays countable.
const CONTAINERS = {
  List: [Array.prototype, Array],
  Iter: [Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()))],
  Map: [Map.prototype, Map],
  Set: [Set.prototype, Set],
  String: [String.prototype, String],
  ByteBuffer: [Uint8Array.prototype, Uint8Array, DataView.prototype],
  "core.math": [Math],
  "core.encoding.json": [JSON],
  "core.time": [Date.prototype, Date],
  "core.regex": [RegExp.prototype],
  "core.tasks": [Promise.prototype, Promise],
  "core.url": [URL.prototype, URL],
  "core.text": [String.prototype, Intl],
};

const ABSENT = {
  SortedSet: "no ECMAScript built-in ordered set",
  Deque: "no ECMAScript built-in double-ended queue",
  PriorityQueue: "no ECMAScript built-in priority queue",
  BitSet: "no ECMAScript built-in bit set; integers carry bit operations",
  Cache: "no ECMAScript built-in cache with an eviction policy",
};

// Operations only. `Symbol.*` keys and non-callable data properties are
// configuration, not workflow, and are counted in excluded so the exclusion
// cannot hide a gap.
function members(target) {
  const operations = [];
  const excluded = [];
  for (const key of Object.getOwnPropertyNames(target)) {
    if (key === "constructor") continue;
    const descriptor = Object.getOwnPropertyDescriptor(target, key);
    const callable = typeof descriptor.value === "function" || typeof descriptor.get === "function";
    (callable ? operations : excluded).push(key);
  }
  return { operations, excluded };
}

const containers = {};
let operationCount = 0;
let excludedCount = 0;
for (const [name, targets] of Object.entries(CONTAINERS)) {
  const operations = new Set();
  const excluded = new Set();
  for (const target of targets) {
    const found = members(target);
    for (const key of found.operations) operations.add(key);
    for (const key of found.excluded) excluded.add(key);
  }
  for (const key of operations) excluded.delete(key);
  containers[name] = {
    present: true,
    operations: Array.from(operations).sort(),
    excludedProperties: Array.from(excluded).sort(),
  };
  operationCount += operations.size;
  excludedCount += excluded.size;
}
for (const [name, reason] of Object.entries(ABSENT)) {
  containers[name] = { present: false, reason: reason, operations: [], excludedProperties: [] };
}

process.stdout.write(JSON.stringify({
  language: "TypeScript",
  alsoCovers: ["JavaScript"],
  sourceKind: "runtime introspection",
  runtime: "node " + process.versions.node + " (V8 " + process.versions.v8 + ")",
  scopeRule: "Callable own properties of the ECMAScript objects that hold each workflow. Non-callable data properties are configuration, not operations; they stay counted in excludedProperties so the exclusion cannot hide a gap.",
  officialReferences: [
    "https://www.typescriptlang.org/tsconfig/lib.html",
    "https://tc39.es/ecma262/",
    "https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects",
  ],
  containers: containers,
  totals: {
    containers: Object.keys(containers).length,
    presentContainers: Object.values(containers).filter((c) => c.present).length,
    operations: operationCount,
    excludedProperties: excludedCount,
  },
}, null, 2) + "\n");
