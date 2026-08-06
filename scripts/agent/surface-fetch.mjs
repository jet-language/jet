#!/usr/bin/env node
/*
 * Emit the Swift, Kotlin, C#, Julia, and R comparison surfaces for the Core
 * surface ledger.
 *
 * These five languages have no runtime this repository can introspect, so each
 * surface is read from that language's own official machine-readable
 * documentation. Every fetched document is recorded with its URL and a sha256
 * of the exact bytes parsed, so a claim can be rechecked without trusting this
 * script's memory.
 *
 * Regenerate one language:
 *   node scripts/agent/surface-fetch.mjs swift \
 *       > docs/reference/surfaces/swift-surface.json
 */

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const CACHE = process.env.SURFACE_CACHE || "/tmp/jet-surface-cache";
mkdirSync(CACHE, { recursive: true });

const fetched = [];

async function get(url) {
  const key = join(CACHE, createHash("sha256").update(url).digest("hex").slice(0, 32));
  let body;
  if (existsSync(key)) {
    body = readFileSync(key, "utf8");
  } else {
    const response = await fetch(url, { headers: { "user-agent": "jet-core-surface-ledger" } });
    if (!response.ok) throw new Error("fetch failed " + response.status + ": " + url);
    body = await response.text();
    writeFileSync(key, body);
  }
  fetched.push({ url: url, sha256: createHash("sha256").update(body).digest("hex"), bytes: body.length });
  return body;
}

function emit(surface) {
  const containers = surface.containers;
  const present = Object.values(containers).filter((c) => c.present);
  process.stdout.write(JSON.stringify({
    language: surface.language,
    sourceKind: surface.sourceKind,
    runtime: surface.runtime,
    scopeRule: surface.scopeRule,
    scopeLimit: surface.scopeLimit,
    officialReferences: surface.officialReferences,
    fetchedDocuments: fetched.sort((a, b) => a.url.localeCompare(b.url)),
    containers: containers,
    totals: {
      containers: Object.keys(containers).length,
      presentContainers: present.length,
      operations: present.reduce((count, c) => count + c.operations.length, 0),
    },
  }, null, 2) + "\n");
}

function absentContainers(map) {
  const out = {};
  for (const [name, reason] of Object.entries(map)) {
    out[name] = { present: false, reason: reason, operations: [] };
  }
  return out;
}

function finish(containers) {
  for (const [name, entry] of Object.entries(containers)) {
    if (entry.present && entry.operations.length === 0) {
      throw new Error("no operations parsed for container " + name);
    }
  }
  return containers;
}

// ---------------------------------------------------------------------------
// Swift — Apple's documentation JSON, the same data that renders the Swift
// standard-library reference.

const SWIFT = {
  List: ["array"],
  Iter: ["sequence", "iteratorprotocol"],
  Map: ["dictionary"],
  Set: ["set"],
  String: ["string", "stringprotocol", "character"],
  "core.math": ["double", "int"],
  "core.time": ["duration"],
  "core.tasks": ["task"],
  "core.regex": ["regex"],
};

const SWIFT_ABSENT = {
  "core.args": "ArgumentParser is a separate package, not the Swift standard library",
  "core.email": "no Swift standard-library email support",
  "core.sync": "concurrency primitives live in Dispatch and Foundation, not the Swift standard library",
  "core.reflect": "Mirror gives read-only structure; Swift ships no reflection module",
  "core.mime": "no Swift standard-library MIME database",
  "core.encoding.xml": "XMLParser belongs to Foundation, not the Swift standard library",
  "core.encoding.yaml": "no Swift standard-library YAML codec",
  "core.mem": "MemoryLayout describes layout; Swift ships no memory module",
  "core.term": "no Swift standard-library terminal control",
  "core.web": "no Swift standard-library web framework",
  "core.text": "text handling lives on String and Character, recorded under String",
  SortedSet: "no Swift standard-library ordered set; swift-collections is a separate package",
  Deque: "no Swift standard-library double-ended queue; swift-collections is a separate package",
  PriorityQueue: "no Swift standard-library priority queue; swift-collections is a separate package",
  BitSet: "no Swift standard-library bit set; integers carry bit operations",
  Cache: "no Swift standard-library cache with an eviction policy",
  ByteBuffer: "no Swift standard-library byte buffer; Data belongs to Foundation",
  "core.random": "SystemRandomNumberGenerator seeds randomness, but Swift ships no random module",
  "core.crypto.random": "no Swift standard-library cryptographic random source; CryptoKit is separate",
  "core.crypto": "no Swift standard-library cryptography; CryptoKit is separate",
  "core.encoding.json": "JSONEncoder belongs to Foundation, not the Swift standard library",
  "core.encoding.csv": "no Swift standard-library CSV codec",
  "core.encoding.toml": "no Swift standard-library TOML decoder",
  "core.encoding.base64": "base64 encoding belongs to Foundation Data, not the Swift standard library",
  "core.encoding.base32": "no Swift standard-library base32 codec",
  "core.encoding.hex": "no Swift standard-library hex codec",
  "core.files": "FileManager belongs to Foundation, not the Swift standard library",
  "core.path": "URL path handling belongs to Foundation, not the Swift standard library",
  "core.env": "ProcessInfo belongs to Foundation, not the Swift standard library",
  "core.os": "ProcessInfo belongs to Foundation, not the Swift standard library",
  "core.process": "Process belongs to Foundation, not the Swift standard library",
  "core.net": "networking belongs to Foundation and Network, not the Swift standard library",
  "core.tls": "TLS belongs to Network and Security, not the Swift standard library",
  "core.http": "URLSession belongs to Foundation, not the Swift standard library",
  "core.url": "URL belongs to Foundation, not the Swift standard library",
  "core.uuid": "UUID belongs to Foundation, not the Swift standard library",
  "core.db": "no Swift standard-library database client",
  "core.log": "Logger belongs to OSLog, not the Swift standard library",
  "core.archive": "no Swift standard-library archive or compression codec",
  "core.data": "no Swift standard-library statistics",
  "core.testing": "XCTest and swift-testing are separate from the Swift standard library",
  "core.binary": "no Swift standard-library binary reader; Foundation Data carries it",
  "core.io": "print is a standard-library function, but Swift ships no console module",
  "core.fmt": "String interpolation is syntax; Swift ships no formatting module",
  "core.text.unicode": "Unicode scalar properties exist on Unicode.Scalar, but Swift ships no Unicode database module",
};

async function swift() {
  const containers = {};
  for (const [name, slugs] of Object.entries(SWIFT)) {
    const operations = new Set();
    for (const slug of slugs) {
      const doc = JSON.parse(await get(
        "https://developer.apple.com/tutorials/data/documentation/swift/" + slug + ".json"));
      // topicSections list every documented member of the type by identifier.
      for (const section of doc.topicSections || []) {
        for (const identifier of section.identifiers || []) {
          const tail = identifier.split("/").pop();
          if (!tail) continue;
          // "append(_:)" and "first(where:)" both name the operation `append`
          // and `first`; the Jet ledger compares operation names, not labels.
          const base = tail.split("(")[0];
          if (/^[a-z_][A-Za-z0-9_]*$/.test(base)) operations.add(base);
        }
      }
    }
    containers[name] = { present: true, types: slugs, operations: Array.from(operations).sort() };
  }
  emit({
    language: "Swift",
    sourceKind: "official documentation JSON (developer.apple.com)",
    runtime: "Swift standard library, developer.apple.com as fetched",
    scopeRule: "Members listed in the topic sections of each Swift standard-library type. Argument labels are dropped because the ledger compares operation names. Foundation is not the Swift standard library; workflows it owns are recorded as absent with that reason.",
    officialReferences: [
      "https://developer.apple.com/documentation/swift",
      "https://developer.apple.com/documentation/swift/sequence-and-collection-protocols",
    ],
    containers: finish(Object.assign(containers, absentContainers(SWIFT_ABSENT))),
  });
}

// ---------------------------------------------------------------------------
// Kotlin — the kotlinlang.org standard-library reference. A package index page
// links each declaration in that package, extensions included, which is what a
// Kotlin user can actually call.

const KOTLIN = {
  List: ["kotlin.collections", "kotlin.collections/-list", "kotlin.collections/-mutable-list"],
  Iter: ["kotlin.sequences"],
  Map: ["kotlin.collections/-map", "kotlin.collections/-mutable-map"],
  Set: ["kotlin.collections/-set", "kotlin.collections/-mutable-set"],
  String: ["kotlin.text"],
  "core.math": ["kotlin.math"],
  "core.random": ["kotlin.random"],
  "core.time": ["kotlin.time"],
  "core.io": ["kotlin.io"],
  "core.files": ["kotlin.io.path"],
  "core.sync": ["kotlin.concurrent"],
  "core.uuid": ["kotlin.uuid"],
  "core.encoding.base64": ["kotlin.io.encoding"],
};

const KOTLIN_ABSENT = {
  "core.tasks": "coroutines ship in kotlinx.coroutines, a separate library",
  "core.args": "no Kotlin standard-library argument parser",
  "core.email": "no Kotlin standard-library email support",
  "core.reflect": "kotlin-reflect is a separate artifact from the standard library",
  "core.mime": "no Kotlin standard-library MIME database",
  "core.encoding.xml": "no Kotlin standard-library XML codec",
  "core.encoding.yaml": "no Kotlin standard-library YAML codec",
  "core.mem": "the JVM manages memory; Kotlin ships no memory module",
  "core.term": "no Kotlin standard-library terminal control",
  "core.web": "ktor is a separate library, not the Kotlin standard library",
  "core.text": "text handling lives in the kotlin.text package, recorded under String",
  SortedSet: "sortedSetOf returns a java.util.SortedSet; the Kotlin standard library declares no ordered set of its own",
  Deque: "ArrayDeque is a Kotlin class but the standard library declares no deque package surface of its own",
  PriorityQueue: "no Kotlin standard-library priority queue; java.util.PriorityQueue is a JDK type",
  BitSet: "no Kotlin standard-library bit set; integers carry bit operations",
  Cache: "no Kotlin standard-library cache with an eviction policy",
  ByteBuffer: "ByteArray is a Kotlin type but the standard library declares no buffer package surface",
  "core.crypto.random": "no Kotlin standard-library cryptographic random source",
  "core.crypto": "no Kotlin standard-library cryptography",
  "core.encoding.json": "kotlinx.serialization is a separate library, not the Kotlin standard library",
  "core.encoding.csv": "no Kotlin standard-library CSV codec",
  "core.encoding.toml": "no Kotlin standard-library TOML decoder",
  "core.encoding.base32": "no Kotlin standard-library base32 codec",
  "core.encoding.hex": "no Kotlin standard-library hex codec",
  "core.regex": "Regex is a kotlin.text class; the standard library declares no separate regex package",
  "core.path": "kotlin.io.path covers paths; no separate path package exists",
  "core.env": "no Kotlin standard-library environment module; System is a JDK type",
  "core.os": "no Kotlin standard-library operating-system module",
  "core.process": "no Kotlin standard-library process module; ProcessBuilder is a JDK type",
  "core.net": "no Kotlin standard-library networking",
  "core.tls": "no Kotlin standard-library TLS client",
  "core.http": "ktor is a separate library, not the Kotlin standard library",
  "core.url": "no Kotlin standard-library URL parser",
  "core.db": "no Kotlin standard-library database client",
  "core.log": "no Kotlin standard-library logging facade",
  "core.archive": "no Kotlin standard-library archive or compression codec",
  "core.data": "no Kotlin standard-library statistics",
  "core.testing": "kotlin.test is a separate artifact from the Kotlin standard library",
  "core.binary": "no Kotlin standard-library binary reader",
  "core.fmt": "no Kotlin standard-library formatting module beyond kotlin.text",
  "core.text.unicode": "no Kotlin standard-library Unicode property database",
};

async function kotlin() {
  const containers = {};
  const pages = new Map();
  for (const [name, packages] of Object.entries(KOTLIN)) {
    const operations = new Set();
    for (const pkg of packages) {
      if (!pages.has(pkg)) {
        pages.set(pkg, await get("https://kotlinlang.org/api/core/kotlin-stdlib/" + pkg + "/"));
      }
      // A declaration in this package is linked by a sibling relative page.
      // Links that climb out of the package, or point at the index itself,
      // name something declared elsewhere.
      // The index renders a declaration's own name inside a `token function`
      // span. Reading the surrounding links instead would collect the type
      // parameters and parameter names that make up the rest of the signature.
      for (const match of pages.get(pkg).matchAll(/<span class="token function">([A-Za-z][A-Za-z0-9_]*)<\/span>/g)) {
        operations.add(match[1]);
      }
    }
    containers[name] = { present: true, packages: packages, operations: Array.from(operations).sort() };
  }
  emit({
    language: "Kotlin",
    sourceKind: "official API reference (kotlinlang.org)",
    runtime: "kotlin-stdlib, kotlinlang.org as fetched",
    scopeRule: "Declarations linked from each kotlin-stdlib package index, extension functions included, because an extension is callable exactly like a member. JDK types reachable from Kotlin are not the Kotlin standard library; workflows only the JDK answers are recorded as absent with that reason.",
    officialReferences: ["https://kotlinlang.org/api/core/kotlin-stdlib/"],
    containers: finish(Object.assign(containers, absentContainers(KOTLIN_ABSENT))),
  });
}

// ---------------------------------------------------------------------------
// C# — Microsoft's own dotnet-api-docs XML, which is the source the .NET API
// reference is rendered from.

const CSHARP = {
  List: ["System.Collections.Generic/List`1"],
  Iter: ["System.Linq/Enumerable"],
  Map: ["System.Collections.Generic/Dictionary`2"],
  Set: ["System.Collections.Generic/HashSet`1"],
  SortedSet: ["System.Collections.Generic/SortedSet`1"],
  Deque: ["System.Collections.Generic/Queue`1"],
  PriorityQueue: ["System.Collections.Generic/PriorityQueue`2"],
  BitSet: ["System.Collections/BitArray"],
  String: ["System/String"],
  ByteBuffer: ["System.IO/MemoryStream"],
  "core.math": ["System/Math"],
  "core.random": ["System/Random"],
  "core.crypto.random": ["System.Security.Cryptography/RandomNumberGenerator"],
  "core.crypto": ["System.Security.Cryptography/SHA256"],
  "core.time": ["System/DateTime", "System/TimeSpan"],
  "core.encoding.json": ["System.Text.Json/JsonSerializer"],
  "core.encoding.base64": ["System/Convert"],
  "core.regex": ["System.Text.RegularExpressions/Regex"],
  "core.files": ["System.IO/File", "System.IO/Directory"],
  "core.path": ["System.IO/Path"],
  "core.os": ["System/Environment"],
  "core.process": ["System.Diagnostics/Process"],
  "core.net": ["System.Net.Sockets/Socket"],
  "core.tls": ["System.Net.Security/SslStream"],
  "core.http": ["System.Net.Http/HttpClient"],
  "core.url": ["System/Uri"],
  "core.uuid": ["System/Guid"],
  "core.db": ["System.Data.Common/DbConnection"],
  "core.tasks": ["System.Threading.Tasks/Task"],
  "core.sync": ["System.Threading/Monitor"],
  "core.email": ["System.Net.Mail/MailMessage"],
  "core.reflect": ["System.Reflection/Assembly"],
  "core.mime": ["System.Net.Mime/ContentType"],
  "core.encoding.xml": ["System.Xml/XmlDocument"],
  "core.mem": ["System/GC"],
  "core.archive": ["System.IO.Compression/ZipFile"],
  "core.binary": ["System.IO/BinaryReader"],
  "core.io": ["System/Console"],
  "core.text.unicode": ["System.Globalization/CharUnicodeInfo"],
};

const CSHARP_ABSENT = {
  "core.args": "System.CommandLine ships separately from the base class library",
  "core.encoding.yaml": "no .NET base-class-library YAML codec",
  "core.term": "console control lives on System.Console, recorded under core.io",
  "core.web": "ASP.NET Core ships separately from the base class library",
  "core.env": "environment access lives on System.Environment, recorded under core.os",
  "core.fmt": "formatting lives on System.String, recorded under String",
  "core.text": "text handling lives on System.String, recorded under String",
  Cache: "MemoryCache ships in a separate NuGet package, not the base class library",
  "core.encoding.csv": "no .NET base-class-library CSV codec",
  "core.encoding.toml": "no .NET base-class-library TOML decoder",
  "core.encoding.base32": "no .NET base-class-library base32 codec",
  "core.encoding.hex": "Convert.ToHexString exists, but .NET ships no hex codec type of its own",
  "core.log": "Microsoft.Extensions.Logging ships separately from the base class library",
  "core.data": "no .NET base-class-library statistics",
  "core.testing": "xUnit, NUnit, and MSTest all ship separately from the base class library",
};

async function csharp() {
  const containers = {};
  const types = new Map();
  for (const [name, paths] of Object.entries(CSHARP)) {
    const operations = new Set();
    for (const path of paths) {
      if (!types.has(path)) {
        types.set(path, await get("https://raw.githubusercontent.com/dotnet/dotnet-api-docs/main/xml/" +
          path.replace(/`/g, "%60") + ".xml"));
      }
      // A constructor is not a named operation, and an explicit interface
      // implementation is spelled with its interface prefix; neither is
      // something a user calls by that name.
      for (const match of types.get(path).matchAll(/<Member MemberName="([^"]+)"/g)) {
        const member = match[1];
        if (member.startsWith(".") || member.includes("op_")) continue;
        const base = member.split("<")[0];
        if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(base)) operations.add(base);
      }
    }
    containers[name] = { present: true, types: paths, operations: Array.from(operations).sort() };
  }
  emit({
    language: "C#",
    sourceKind: "official API documentation source (github.com/dotnet/dotnet-api-docs)",
    runtime: "dotnet-api-docs main branch as fetched",
    scopeRule: "Named members of the base-class-library types that hold each workflow. Constructors and operator overloads are excluded because a user does not call them by name. Types that ship in a separate NuGet package are not the base class library; workflows only they answer are recorded as absent with that reason.",
    officialReferences: [
      "https://learn.microsoft.com/en-us/dotnet/api/",
      "https://learn.microsoft.com/en-us/dotnet/standard/linq/",
    ],
    containers: finish(Object.assign(containers, absentContainers(CSHARP_ABSENT))),
  });
}

// ---------------------------------------------------------------------------
// Julia — the Documenter search index behind docs.julialang.org, which records
// every documented binding with the manual page that documents it.

const JULIA = {
  List: ["base/arrays"],
  Iter: ["base/iterators"],
  Map: ["base/collections"],
  String: ["base/strings"],
  "core.math": ["base/math", "base/numbers"],
  "core.random": ["stdlib/Random"],
  "core.data": ["stdlib/Statistics"],
  "core.time": ["stdlib/Dates"],
  "core.files": ["base/file"],
  "core.io": ["base/io-network"],
  "core.os": ["base/base"],
  "core.tasks": ["base/parallel"],
  "core.sync": ["base/multi-threading"],
  "core.crypto": ["stdlib/SHA"],
  "core.uuid": ["stdlib/UUIDs"],
  "core.encoding.base64": ["stdlib/Base64"],
  "core.testing": ["stdlib/Test"],
  "core.log": ["stdlib/Logging"],
  "core.db": ["stdlib/LibGit2"],
  "core.archive": ["stdlib/Tar"],
};

const JULIA_ABSENT = {
  "core.args": "no Julia standard-library argument parser; ArgParse.jl is a package",
  "core.email": "no Julia standard-library email support",
  "core.reflect": "reflection is documented on the base manual page, recorded under core.os",
  "core.mime": "no Julia standard-library MIME database beyond display types",
  "core.encoding.xml": "no Julia standard-library XML codec",
  "core.encoding.yaml": "no Julia standard-library YAML codec",
  "core.mem": "the runtime manages memory; Julia ships no memory module",
  "core.term": "no Julia standard-library terminal control",
  "core.web": "no Julia standard-library web framework",
  "core.env": "environment access is documented on the base manual page, recorded under core.os",
  Set: "Set is documented on the collections manual page, recorded under Map",
  "core.path": "path handling is documented on the file manual page, recorded under core.files",
  "core.net": "networking is documented on the io-network manual page, recorded under core.io",
  "core.binary": "binary reading is documented on the io-network manual page, recorded under core.io",
  "core.process": "process control is documented on the base manual page, recorded under core.os",
  "core.text": "text handling is documented on the strings manual page, recorded under String",
  "core.regex": "regular expressions are documented on the strings manual page, recorded under String",
  SortedSet: "no Julia standard-library ordered set; DataStructures.jl is a package",
  Deque: "no Julia standard-library double-ended queue; DataStructures.jl is a package",
  PriorityQueue: "no Julia standard-library priority queue; DataStructures.jl is a package",
  BitSet: "BitSet exists in Base, but the manual documents it inside collections rather than as its own surface",
  Cache: "no Julia standard-library cache with an eviction policy",
  ByteBuffer: "IOBuffer exists in Base, but the manual documents it inside io-network rather than as its own surface",
  "core.crypto.random": "Random.RandomDevice seeds from the system, but Julia ships no cryptographic random module",
  "core.encoding.json": "no Julia standard-library JSON codec; JSON.jl is a package",
  "core.encoding.csv": "no Julia standard-library CSV codec; CSV.jl is a package",
  "core.encoding.toml": "TOML ships in the standard library, but the manual documents it outside the fetched pages",
  "core.encoding.base32": "no Julia standard-library base32 codec",
  "core.encoding.hex": "bytes2hex exists in Base, but Julia ships no hex codec module",
  "core.tls": "no Julia standard-library TLS client; MbedTLS.jl is a package",
  "core.http": "no Julia standard-library HTTP client; HTTP.jl is a package",
  "core.url": "no Julia standard-library URL parser; URIs.jl is a package",
  "core.fmt": "Printf ships in the standard library, but the manual documents it outside the fetched pages",
  "core.text.unicode": "no Julia standard-library Unicode property database beyond Base.Unicode",
};

async function julia() {
  const body = await get("https://docs.julialang.org/en/v1/search_index.js");
  const index = JSON.parse(body.replace(/^\s*var documenterSearchIndex\s*=\s*/, "").replace(/;?\s*$/, ""));
  // A section is prose, not an operation. A constant or type is a value or a
  // shape, not something a user calls.
  const callable = new Set(["function", "method", "macro"]);
  const byPage = new Map();
  for (const doc of index.docs) {
    if (!callable.has(doc.category)) continue;
    const page = doc.location.split("#")[0].replace(/\/$/, "");
    const name = doc.title.replace(/^@/, "").split("(")[0];
    const bare = name.split(".").pop();
    if (!/^[A-Za-z_][A-Za-z0-9_!]*$/.test(bare)) continue;
    if (!byPage.has(page)) byPage.set(page, new Set());
    byPage.get(page).add(bare);
  }
  const containers = {};
  for (const [name, pages] of Object.entries(JULIA)) {
    const operations = new Set();
    for (const page of pages) {
      const found = byPage.get(page);
      if (!found) throw new Error("manual page absent from the Julia search index: " + page);
      for (const key of found) operations.add(key);
    }
    containers[name] = { present: true, manualPages: pages, operations: Array.from(operations).sort() };
  }
  emit({
    language: "Julia",
    sourceKind: "official documentation search index (docs.julialang.org)",
    runtime: "Julia v1 manual as fetched",
    scopeRule: "Documented functions, methods, and macros, grouped by the manual page that documents them. Sections, constants, and types are excluded because they are prose, values, or shapes rather than operations. Module qualifiers are dropped because the ledger compares operation names.",
    officialReferences: [
      "https://docs.julialang.org/en/v1/base/collections/",
      "https://docs.julialang.org/en/v1/",
    ],
    containers: finish(Object.assign(containers, absentContainers(JULIA_ABSENT))),
  });
}

// ---------------------------------------------------------------------------
// R — the R manual's per-package function index. The card adds R for the data
// and math surface, so that is the whole claim recorded here.

// R's manual indexes a whole package at once, so a match means "package base
// documents a function of this name", not "base's math surface documents it".
// Claiming a per-container R surface off a package-level index would credit R
// with operations its index cannot attribute, so the claim is held to the data
// and math surface the card adds R for, and every other container is recorded
// as outside the claim.
const R_PACKAGES = {
  "core.math": ["base"],
  "core.data": ["stats"],
};

const R_OUT_OF_CLAIM =
  "outside the R claim: the ledger records R for the data and math surface only, " +
  "and R's package-level function index cannot attribute a name to this container";

const R_ABSENT = {
  "core.random": R_OUT_OF_CLAIM,

  Iter: "no R base iterator protocol; the iterators package is separate",
  Set: "no R base set type; union and intersect operate on vectors",
  SortedSet: "no R base ordered set",
  Deque: "no R base double-ended queue",
  PriorityQueue: "no R base priority queue",
  BitSet: "no R base bit set; logical vectors carry the workflow",
  Cache: "no R base cache with an eviction policy",
  ByteBuffer: "raw vectors carry bytes, but R base ships no buffer type",
  List: R_OUT_OF_CLAIM,
  Map: R_OUT_OF_CLAIM,
  String: R_OUT_OF_CLAIM,
  "core.archive": R_OUT_OF_CLAIM,
  "core.binary": R_OUT_OF_CLAIM,
  "core.crypto": R_OUT_OF_CLAIM,
  "core.crypto.random": R_OUT_OF_CLAIM,
  "core.db": R_OUT_OF_CLAIM,
  "core.encoding.base32": R_OUT_OF_CLAIM,
  "core.encoding.base64": R_OUT_OF_CLAIM,
  "core.encoding.csv": R_OUT_OF_CLAIM,
  "core.encoding.hex": R_OUT_OF_CLAIM,
  "core.encoding.json": R_OUT_OF_CLAIM,
  "core.encoding.toml": R_OUT_OF_CLAIM,
  "core.env": R_OUT_OF_CLAIM,
  "core.files": R_OUT_OF_CLAIM,
  "core.fmt": R_OUT_OF_CLAIM,
  "core.http": R_OUT_OF_CLAIM,
  "core.io": R_OUT_OF_CLAIM,
  "core.log": R_OUT_OF_CLAIM,
  "core.net": R_OUT_OF_CLAIM,
  "core.os": R_OUT_OF_CLAIM,
  "core.path": R_OUT_OF_CLAIM,
  "core.process": R_OUT_OF_CLAIM,
  "core.regex": R_OUT_OF_CLAIM,
  "core.tasks": R_OUT_OF_CLAIM,
  "core.testing": R_OUT_OF_CLAIM,
  "core.text": R_OUT_OF_CLAIM,
  "core.text.unicode": R_OUT_OF_CLAIM,
  "core.time": R_OUT_OF_CLAIM,
  "core.tls": R_OUT_OF_CLAIM,
  "core.url": R_OUT_OF_CLAIM,
  "core.uuid": R_OUT_OF_CLAIM,
  "core.args": R_OUT_OF_CLAIM,
  "core.email": R_OUT_OF_CLAIM,
  "core.sync": R_OUT_OF_CLAIM,
  "core.reflect": R_OUT_OF_CLAIM,
  "core.mime": R_OUT_OF_CLAIM,
  "core.encoding.xml": R_OUT_OF_CLAIM,
  "core.encoding.yaml": R_OUT_OF_CLAIM,
  "core.mem": R_OUT_OF_CLAIM,
  "core.term": R_OUT_OF_CLAIM,
  "core.web": R_OUT_OF_CLAIM,
};

async function r() {
  const containers = {};
  const packages = new Map();
  for (const [name, names] of Object.entries(R_PACKAGES)) {
    const operations = new Set();
    for (const pkg of names) {
      if (!packages.has(pkg)) {
        packages.set(pkg, await get(
          "https://stat.ethz.ch/R-manual/R-devel/library/" + pkg + "/html/00Index.html"));
      }
      for (const match of packages.get(pkg).matchAll(/<a href="[^"]+\.html">([^<]+)<\/a>/g)) {
        const entry = match[1].trim();
        if (/^[A-Za-z.][A-Za-z0-9._]*$/.test(entry)) operations.add(entry);
      }
    }
    // The index answers "package base documents a function of this name". It
    // cannot answer "this function belongs to the math surface", so this
    // container may confirm a Jet match but may not mint a per-operation gap.
    containers[name] = {
      present: true,
      attribution: "package",
      packages: names,
      operations: Array.from(operations).sort(),
    };
  }
  emit({
    language: "R",
    sourceKind: "official R manual package index (stat.ethz.ch R-devel)",
    runtime: "R-devel manual as fetched",
    scopeRule: "Documented entries in the function index of each R package that holds the workflow. Entries whose names are not plain identifiers are excluded because they are operators or syntax rather than named operations.",
    scopeLimit: "R is recorded for the data and math surface only, which is the whole claim the ledger makes for it. Containers outside that claim are recorded as absent with a reason, never scored as an R win.",
    officialReferences: [
      "https://stat.ethz.ch/R-manual/R-devel/library/base/html/00Index.html",
      "https://stat.ethz.ch/R-manual/R-devel/library/stats/html/00Index.html",
    ],
    containers: finish(Object.assign(containers, absentContainers(R_ABSENT))),
  });
}

const LANGUAGES = { swift, kotlin, csharp, julia, r };
const which = process.argv[2];
if (!LANGUAGES[which]) {
  throw new Error("usage: surface-fetch.mjs " + Object.keys(LANGUAGES).join("|"));
}
await LANGUAGES[which]();
