#!/usr/bin/env node
/*
 * Emit the Go comparison surface for the Core surface ledger.
 *
 * Go freezes its public standard-library API in GOROOT/api/go1*.txt. Those
 * files are the Go project's own record of every exported symbol, so reading
 * them makes every "Go has this" and "Go lacks this" claim checkable.
 *
 * Regenerate:
 *   nix shell nixpkgs#go --command sh -c \
 *     'node scripts/agent/surface-go.mjs $(go env GOROOT) $(go version)' \
 *     > docs/reference/surfaces/go-surface.json
 */

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const goroot = process.argv[2];
const version = process.argv.slice(3).join(" ") || "unknown";
if (!goroot) throw new Error("usage: surface-go.mjs <GOROOT> <go version...>");

// Canonical Jet-facing container names map to the Go packages that hold the
// same workflow. A container Go does not ship is recorded as absent, never
// omitted, so the gap stays countable.
const CONTAINERS = {
  List: ["slices"],
  Iter: ["iter"],
  Map: ["maps"],
  String: ["strings", "strconv"],
  ByteBuffer: ["bytes"],
  Deque: ["container/list"],
  PriorityQueue: ["container/heap"],
  BitSet: ["math/bits"],
  "core.math": ["math", "math/big", "math/cmplx"],
  "core.random": ["math/rand/v2"],
  "core.crypto.random": ["crypto/rand"],
  "core.crypto": ["crypto", "crypto/sha256", "crypto/hmac", "crypto/aes"],
  "core.time": ["time"],
  "core.encoding.json": ["encoding/json"],
  "core.encoding.csv": ["encoding/csv"],
  "core.encoding.base64": ["encoding/base64"],
  "core.encoding.base32": ["encoding/base32"],
  "core.encoding.hex": ["encoding/hex"],
  "core.regex": ["regexp"],
  "core.files": ["os", "io/fs"],
  "core.path": ["path/filepath", "path"],
  "core.env": ["os"],
  "core.process": ["os/exec"],
  "core.net": ["net"],
  "core.tls": ["crypto/tls", "crypto/x509"],
  "core.http": ["net/http"],
  "core.url": ["net/url"],
  "core.db": ["database/sql"],
  "core.tasks": ["sync", "context"],
  "core.testing": ["testing"],
  "core.log": ["log", "log/slog"],
  "core.binary": ["encoding/binary", "io"],
  "core.archive": ["archive/zip", "archive/tar", "compress/gzip", "compress/flate"],
  "core.os": ["os", "runtime"],
  "core.io": ["fmt", "bufio"],
  "core.fmt": ["fmt"],
  "core.text.unicode": ["unicode", "unicode/utf8"],
  "core.text": ["strings", "text/template"],
};

const ABSENT = {
  Set: "no Go standard-library set type; the idiom is map[T]struct{}",
  SortedSet: "no Go standard-library ordered set",
  Cache: "no Go standard-library cache with an eviction policy",
  "core.uuid": "no Go standard-library UUID generator",
  "core.data": "no Go standard-library statistics package",
  "core.encoding.toml": "no Go standard-library TOML decoder",
};

// One line is `pkg <path>[, (<build tag>)], <kind> <name>...`. Operations are
// funcs and methods. Consts, vars, and type declarations are configuration or
// shape, not operations; they stay counted so the exclusion cannot hide a gap.
const byPackage = new Map();
const files = readdirSync(join(goroot, "api")).filter((name) => /^go1(\.\d+)?\.txt$/.test(name));
if (files.length === 0) throw new Error("no api/go1*.txt under " + goroot);
for (const file of files) {
  for (const line of readFileSync(join(goroot, "api", file), "utf8").split("\n")) {
    const match = /^pkg ([^,]+?)(?: \([^)]*\))?, (func|method|type|const|var) (.+)$/.exec(line.trim());
    if (!match) continue;
    const [, pkg, kind, rest] = match;
    if (!byPackage.has(pkg)) byPackage.set(pkg, { operations: new Set(), excluded: new Set() });
    const entry = byPackage.get(pkg);
    if (kind === "func") {
      entry.operations.add(rest.split(/[(\[ ]/)[0]);
    } else if (kind === "method") {
      const method = /^\([^)]*\) ([A-Za-z_][A-Za-z0-9_]*)/.exec(rest);
      if (method) entry.operations.add(method[1]);
    } else {
      entry.excluded.add(rest.split(/[ (\[]/)[0]);
    }
  }
}

const containers = {};
let operationCount = 0;
let excludedCount = 0;
for (const [name, packages] of Object.entries(CONTAINERS)) {
  const operations = new Set();
  const excluded = new Set();
  for (const pkg of packages) {
    const entry = byPackage.get(pkg);
    if (!entry) throw new Error("package absent from the recorded Go API: " + pkg);
    for (const key of entry.operations) operations.add(key);
    for (const key of entry.excluded) excluded.add(key);
  }
  for (const key of operations) excluded.delete(key);
  containers[name] = {
    present: true,
    packages: packages,
    operations: Array.from(operations).sort(),
    excludedDeclarations: Array.from(excluded).sort(),
  };
  operationCount += operations.size;
  excludedCount += excluded.size;
}
for (const [name, reason] of Object.entries(ABSENT)) {
  containers[name] = { present: false, reason: reason, operations: [], excludedDeclarations: [] };
}

process.stdout.write(JSON.stringify({
  language: "Go",
  sourceKind: "official frozen API files (GOROOT/api/go1*.txt)",
  runtime: version,
  scopeRule: "Exported funcs and methods of the packages that hold each workflow, unioned across every released API file. Consts, vars, and type declarations are configuration or shape, not operations; they stay counted in excludedDeclarations so the exclusion cannot hide a gap.",
  officialReferences: ["https://pkg.go.dev/std", "https://go.dev/doc/go1compat"],
  apiFiles: files.sort(),
  containers: containers,
  totals: {
    containers: Object.keys(containers).length,
    presentContainers: Object.values(containers).filter((c) => c.present).length,
    operations: operationCount,
    excludedDeclarations: excludedCount,
  },
}, null, 2) + "\n");
