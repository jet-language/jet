#!/usr/bin/env node
// Card #298: offline, checksum-verified Unicode 17.0.0 UCD table generator.
// No network at build/compile time — this script is run by hand (or by a
// maintainer script) against a local UCD snapshot dir. The allocation-free Jet
// runtime lookup functions and the JUCD Core text assets are committed together.
// No external packages (I6): plain Node and Jet.
//
// Usage:
//   node Tools/agent/gen-unicode-tables.mjs [--write | --check] [ucd-dir]
//
// <ucd-dir> must contain (fetched from https://www.unicode.org/Public/17.0.0/ucd/):
//   UnicodeData.txt, CaseFolding.txt, SpecialCasing.txt, PropList.txt,
//   CompositionExclusions.txt,
//   DerivedNormalizationProps.txt, EastAsianWidth.txt, emoji/emoji-data.txt,
//   auxiliary/GraphemeBreakProperty.txt, auxiliary/WordBreakProperty.txt,
//   auxiliary/SentenceBreakProperty.txt, DerivedCoreProperties.txt (GB9c's
//   Indic_Conjunct_Break, added card #298 segmentation slice)
//
// Emits these paths:
//   Compiler/JetRuntime/Source/UnicodeTables.jet (fixed-word runtime lookups)
//   Compiler/JetRuntime/Source/UnicodeNormalizeTables.jet (NFC-only lookups)
//   Compiler/JetRuntime/Source/UnicodeText_<n>.jet (native text-method lookups)
//   Core/Text/... (JUCD schema-1 assets, names and segmentation tables)
//
// Pinned Unicode release: 17.0.0. Pinned sha256 of each input file (verified
// before generating — mismatch aborts):
const PINNED_SHA256 = {
  "UnicodeData.txt": "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c",
  "CaseFolding.txt": "ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183",
  "SpecialCasing.txt": "efc25faf19de21b92c1194c111c932e03d2a5eaf18194e33f1156e96de4c9588",
  "CompositionExclusions.txt": "2f239196ef3b5b61db5cc476e9bd80f534d15aa1b74e1be1dea5d042a344c85f",
  "EastAsianWidth.txt": "ea7ce50f3444a050333448dffef1cadd9325af55cbb764b4a2280faf52170a33",
  "DerivedNormalizationProps.txt": "71fd6a206a2c0cdd41feb6b7f656aa31091db45e9cedc926985d718397f9e488",
  "emoji/emoji-data.txt": "2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b",
  "auxiliary/GraphemeBreakProperty.txt": "d6b51d1d2ae5c33b451b7ed994b48f1f4dc62b2272a5831e7fd418514a6bae89",
  "auxiliary/WordBreakProperty.txt": "72274cac1e6b919507db35655c3e175aa27274668a1ece95c28d2069f2ad9852",
  "auxiliary/SentenceBreakProperty.txt": "871c0c985ad95125e25b302414065a10839d068970bceb383ecec138f22a0a18",
  // GB9c (Indic conjunct clusters — Unicode 15+): Indic_Conjunct_Break.
  "DerivedCoreProperties.txt": "24c7fed1195c482faaefd5c1e7eb821c5ee1fb6de07ecdbaa64b56a99da22c08",
  "PropList.txt": "130dcddcaadaf071008bdfce1e7743e04fdfbc910886f017d9f9ac931d8c64dd",
};

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";
import assert from "node:assert/strict";

const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const ucdDir = args.find(arg => !arg.startsWith("--")) ?? "tests/data/unicode/ucd";

function readChecked(rel) {
  const p = path.join(ucdDir, rel);
  const buf = readFileSync(p);
  const got = createHash("sha256").update(buf).digest("hex");
  const want = PINNED_SHA256[rel];
  if (!want) throw new Error(`no pinned checksum recorded for ${rel}`);
  if (got !== want) {
    throw new Error(`checksum mismatch for ${rel}: got ${got}, want ${want}`);
  }
  return buf.toString("utf8");
}

function stripComment(line) {
  const i = line.indexOf("#");
  return i === -1 ? line : line.slice(0, i);
}

// ---- UnicodeData.txt ------------------------------------------------------
// fields: cp;name;gc;ccc;bidi;decomp;...
const unicodeData = readChecked("UnicodeData.txt");
const ccc = []; // [cp, ccc] nonzero only, sorted (file is cp-sorted already)
const canonDecomp = new Map(); // cp -> [u32...] (no <tag>)
const compatDecomp = new Map(); // cp -> [u32...] (any decomposition, tag or not)
const simpleLower = new Map(); // cp -> [u32...], overridden by SpecialCasing
const simpleUpper = new Map(); // cp -> [u32...], overridden by SpecialCasing
const simpleTitle = new Map(); // cp -> [u32...], overridden by SpecialCasing
const unicodeNames = new Map(); // cp -> Unicode name; controls use their legacy names
const generalCategory = []; // [start,end,gcTagIndex]
const letterRanges = []; // General_Category L*
const numericRanges = []; // General_Category Nd/Nl/No
const GC_TAGS = ["Cc", "Cf", "Mn", "Me", "Mc", "Zs", "Zl", "Zp", "Other"];
function gcTag(gc) {
  const i = GC_TAGS.indexOf(gc);
  return i === -1 ? GC_TAGS.length - 1 : i;
}

let pendingRange = null;
for (const raw of unicodeData.split("\n")) {
  if (!raw) continue;
  const f = raw.split(";");
  if (f.length < 14) continue;
  const cp = parseInt(f[0], 16);
  const name = f[1];
  const gc = f[2];
  const unicodeName = name === "<control>" ? f[10].trim() : name;
  if (unicodeName && !unicodeName.startsWith("<") && !name.endsWith(", First>") && !name.endsWith(", Last>")) {
    unicodeNames.set(cp, unicodeName);
  }
  if (name.endsWith(", First>")) {
    pendingRange = [cp, gc];
    continue;
  }
  if (name.endsWith(", Last>")) {
    if (!pendingRange || pendingRange[1] !== gc) throw new Error(`malformed UnicodeData range at ${raw}`);
    const [start] = pendingRange;
    generalCategory.push([start, cp, gcTag(gc)]);
    if (gc.startsWith("L")) letterRanges.push([start, cp]);
    if (["Nd", "Nl", "No"].includes(gc)) numericRanges.push([start, cp]);
    pendingRange = null;
    continue;
  }
  const cccVal = parseInt(f[3], 10) || 0;
  const decomp = f[5].trim();
  if (cccVal !== 0) ccc.push([cp, cccVal]);
  generalCategory.push([cp, cp, gcTag(gc)]);
  if (gc.startsWith("L")) letterRanges.push([cp, cp]);
  if (["Nd", "Nl", "No"].includes(gc)) numericRanges.push([cp, cp]);
  if (f[12]) simpleUpper.set(cp, [parseInt(f[12], 16)]);
  if (f[13]) simpleLower.set(cp, [parseInt(f[13], 16)]);
  if (f[14]) simpleTitle.set(cp, [parseInt(f[14], 16)]);
  if (decomp) {
    const tagged = decomp.startsWith("<");
    let rest = decomp;
    if (tagged) rest = decomp.slice(decomp.indexOf(">") + 1).trim();
    const cps = rest.split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16));
    compatDecomp.set(cp, cps);
    if (!tagged) canonDecomp.set(cp, cps);
  }
}
if (pendingRange) throw new Error("unterminated UnicodeData First/Last range");

// Unconditional rows are locale-free full mappings. Conditional rows are
// locale/context rules; v1 applies the one language-neutral condition,
// Final_Sigma, in Text.rs/TextLite.rs and intentionally excludes tr/az/lt.
const specialCasing = readChecked("SpecialCasing.txt");
for (const raw of specialCasing.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const f = line.split(";").map((x) => x.trim());
  if (f.length < 5 || f[4] !== "") continue;
  const cp = parseInt(f[0], 16);
  simpleLower.set(cp, f[1].split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16)));
  simpleTitle.set(cp, f[2].split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16)));
  simpleUpper.set(cp, f[3].split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16)));
}

// ---- CompositionExclusions / DerivedNormalizationProps ---------------------
// Full_Composition_Exclusion (DerivedNormalizationProps.txt) is the complete
// authoritative exclusion set (primary exclusions + singleton + non-starter
// decomposables); it supersedes CompositionExclusions.txt alone.
readChecked("CompositionExclusions.txt");
const dnp = readChecked("DerivedNormalizationProps.txt");
const fullCompositionExclusion = []; // [start,end]
for (const raw of dnp.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const parts = line.split(";").map((x) => x.trim());
  if (parts.length < 2) continue;
  if (parts[1] !== "Full_Composition_Exclusion") continue;
  const range = parts[0];
  if (range.includes("..")) {
    const [a, b] = range.split("..");
    fullCompositionExclusion.push([parseInt(a, 16), parseInt(b, 16)]);
  } else {
    const cp = parseInt(range, 16);
    fullCompositionExclusion.push([cp, cp]);
  }
}
function inExclusion(cp) {
  // linear scan is fine at generation time (one-shot, offline)
  for (const [a, b] of fullCompositionExclusion) if (cp >= a && cp <= b) return true;
  return false;
}

// ---- CaseFolding.txt --------------------------------------------------------
// status C (common) + F (full) define default case folding; S (simple) and T
// (Turkic) are excluded (locale-free default fold per D-TEXTUNICODE1).
const caseFolding = readChecked("CaseFolding.txt");
const caseFold = new Map(); // cp -> [u32...]
const simpleCaseFold = new Map(); // cp -> one u32 (C+S)
for (const raw of caseFolding.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const f = line.split(";").map((x) => x.trim());
  if (f.length < 3) continue;
  const status = f[1];
  const cp = parseInt(f[0], 16);
  const mapping = f[2].split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16));
  if (status === "C" || status === "F") caseFold.set(cp, mapping);
  if ((status === "C" || status === "S") && mapping.length === 1) simpleCaseFold.set(cp, mapping[0]);
}

// ---- EastAsianWidth.txt ------------------------------------------------------
// tag: 0 = narrow group (N/Na/H), 1 = Ambiguous, 2 = wide group (W/F)
const eaw = readChecked("EastAsianWidth.txt");
const eawRanges = [];
for (const raw of eaw.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const f = line.split(";").map((x) => x.trim());
  if (f.length < 2) continue;
  const range = f[0];
  const prop = f[1];
  let a, b;
  if (range.includes("..")) {
    [a, b] = range.split("..").map((x) => parseInt(x, 16));
  } else {
    a = b = parseInt(range, 16);
  }
  let tag;
  if (prop === "A") tag = 1;
  else if (prop === "W" || prop === "F") tag = 2;
  else tag = 0; // N, Na, H
  eawRanges.push([a, b, tag]);
}

// ---- emoji/emoji-data.txt ----------------------------------------------------
const emojiData = readChecked("emoji/emoji-data.txt");
const extPictographic = [];
const emojiPresentation = [];
const emoji = [];
for (const raw of emojiData.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const f = line.split(";").map((x) => x.trim());
  if (f.length < 2) continue;
  const range = f[0];
  const prop = f[1];
  let a, b;
  if (range.includes("..")) [a, b] = range.split("..").map((x) => parseInt(x, 16));
  else a = b = parseInt(range, 16);
  if (prop === "Extended_Pictographic") extPictographic.push([a, b]);
  if (prop === "Emoji_Presentation") emojiPresentation.push([a, b]);
  if (prop === "Emoji") emoji.push([a, b]);
}

// ---- auxiliary/*BreakProperty.txt -------------------------------------------
function parseBreakProperty(text) {
  const out = []; // [start,end,tagString]
  for (const raw of text.split("\n")) {
    const line = stripComment(raw).trim();
    if (!line) continue;
    const f = line.split(";").map((x) => x.trim());
    if (f.length < 2) continue;
    const range = f[0];
    const prop = f[1];
    let a, b;
    if (range.includes("..")) [a, b] = range.split("..").map((x) => parseInt(x, 16));
    else a = b = parseInt(range, 16);
    out.push([a, b, prop]);
  }
  return out;
}
const graphemeProp = parseBreakProperty(readChecked("auxiliary/GraphemeBreakProperty.txt"));
const wordProp = parseBreakProperty(readChecked("auxiliary/WordBreakProperty.txt"));
const sentenceProp = parseBreakProperty(readChecked("auxiliary/SentenceBreakProperty.txt"));
const derivedCoreProperties = readChecked("DerivedCoreProperties.txt");
const derivedCoreProp = parseBreakProperty(derivedCoreProperties);
const alphabeticRanges = derivedCoreProp.filter(([, , p]) => p === "Alphabetic").map(([a, b]) => [a, b]);
const casedRanges = derivedCoreProp.filter(([, , p]) => p === "Cased").map(([a, b]) => [a, b]);
const caseIgnorableRanges = derivedCoreProp.filter(([, , p]) => p === "Case_Ignorable").map(([a, b]) => [a, b]);
const lowercaseRanges = derivedCoreProp.filter(([, , p]) => p === "Lowercase").map(([a, b]) => [a, b]);
const uppercaseRanges = derivedCoreProp.filter(([, , p]) => p === "Uppercase").map(([a, b]) => [a, b]);
const defaultIgnorableRanges = derivedCoreProp.filter(([, , p]) => p === "Default_Ignorable_Code_Point").map(([a, b]) => [a, b]);
const whiteSpaceRanges = parseBreakProperty(readChecked("PropList.txt"))
  .filter(([, , p]) => p === "White_Space")
  .map(([a, b]) => [a, b]);

// ---- DerivedCoreProperties.txt: Indic_Conjunct_Break (GB9c) -----------------
// `<range> ; InCB; <Value> # comment` lines only; everything else in the file
// (Alphabetic, ID_Start, …) is irrelevant here and skipped.
function parseInCB(text) {
  const out = []; // [start,end,tagString]
  for (const raw of text.split("\n")) {
    const line = stripComment(raw).trim();
    if (!line) continue;
    const f = line.split(";").map((x) => x.trim());
    if (f.length < 3 || f[1] !== "InCB") continue;
    const range = f[0];
    const prop = f[2];
    let a, b;
    if (range.includes("..")) [a, b] = range.split("..").map((x) => parseInt(x, 16));
    else a = b = parseInt(range, 16);
    out.push([a, b, prop]);
  }
  return out;
}
const incbProp = parseInCB(derivedCoreProperties);

// ---- derive: canonical composition pairs ------------------------------------
// pair (c1,c2) -> composed, only from canonical (untagged) 2-codepoint
// decompositions whose composed cp is not in Full_Composition_Exclusion.
const composePairs = []; // [c1,c2,composed]
for (const [cp, seq] of canonDecomp) {
  if (seq.length !== 2) continue;
  if (inExclusion(cp)) continue;
  composePairs.push([seq[0], seq[1], cp]);
}
composePairs.sort((a, b) => (a[0] - b[0]) || (a[1] - b[1]));

// ---- RLE range-merge helpers -------------------------------------------------
function mergeRangesSameTag(pairs) {
  // pairs: [[cp, tag], ...] sorted by cp ascending, contiguous same-tag merge
  const out = [];
  for (const [cp, tag] of pairs) {
    const last = out[out.length - 1];
    if (last && last[1] + 1 === cp && last[2] === tag) {
      last[1] = cp;
    } else {
      out.push([cp, cp, tag]);
    }
  }
  return out;
}
function mergeTaggedRanges(ranges) {
  // ranges: [[a,b,tag],...] already individual UCD ranges; merge adjacent
  // ranges sharing the same tag (post-sort).
  const sorted = [...ranges].sort((x, y) => x[0] - y[0]);
  const out = [];
  for (const [a, b, tag] of sorted) {
    const last = out[out.length - 1];
    if (last && last[1] + 1 === a && last[2] === tag) {
      last[1] = b;
    } else {
      out.push([a, b, tag]);
    }
  }
  return out;
}
function mergePairRanges(ranges) {
  const sorted = [...ranges].sort((x, y) => x[0] - y[0]);
  const out = [];
  for (const [a, b] of sorted) {
    const last = out[out.length - 1];
    if (last && last[1] + 1 === a) last[1] = b;
    else out.push([a, b]);
  }
  return out;
}

const cccRanges = mergeRangesSameTag(ccc);
const gcRanges = mergeTaggedRanges(generalCategory);
const eawMerged = mergeTaggedRanges(eawRanges);

// ---- break-property enum tag encodings --------------------------------------
const GRAPHEME_TAGS = [
  "Other", "CR", "LF", "Control", "Extend", "ZWJ", "Regional_Indicator",
  "Prepend", "SpacingMark", "L", "V", "T", "LV", "LVT",
];
const WORD_TAGS = [
  "Other", "CR", "LF", "Newline", "Extend", "ZWJ", "Regional_Indicator",
  "Format", "Katakana", "Hebrew_Letter", "ALetter", "Single_Quote",
  "Double_Quote", "MidNumLet", "MidLetter", "MidNum", "Numeric",
  "ExtendNumLet", "WSegSpace",
];
const SENTENCE_TAGS = [
  "Other", "CR", "LF", "Extend", "Sep", "Format", "Sp", "Lower", "Upper",
  "OLetter", "Numeric", "ATerm", "SContinue", "STerm", "Close", "SContinue",
];
function encodeBreakRanges(ranges, tags) {
  const withTag = ranges
    .filter(([, , p]) => tags.includes(p))
    .map(([a, b, p]) => [a, b, tags.indexOf(p)]);
  return mergeTaggedRanges(withTag);
}
const graphemeRanges = encodeBreakRanges(graphemeProp, GRAPHEME_TAGS);
const wordRanges = encodeBreakRanges(wordProp, WORD_TAGS);
const sentenceRanges = encodeBreakRanges(sentenceProp, SENTENCE_TAGS);
// GB9c: 0=None(unlisted default) 1=Linker 2=Consonant 3=Extend
const INCB_TAGS = ["None", "Linker", "Consonant", "Extend"];
const incbRanges = encodeBreakRanges(incbProp, INCB_TAGS);

// decomposition pool: flatten canon+compat into one u32 pool with (cp,start,len,is_canon)
const pool = [];
const decompIndex = []; // [cp, start, len, isCanonU8]
const allDecompCps = new Set([...canonDecomp.keys(), ...compatDecomp.keys()]);
for (const cp of [...allDecompCps].sort((a, b) => a - b)) {
  const compat = compatDecomp.get(cp);
  const isCanon = canonDecomp.has(cp) ? 1 : 0;
  const start = pool.length;
  pool.push(...compat);
  decompIndex.push([cp, start, compat.length, isCanon]);
}

const foldPool = [];
const foldIndex = []; // [cp, start, len]
for (const cp of [...caseFold.keys()].sort((a, b) => a - b)) {
  const seq = caseFold.get(cp);
  const start = foldPool.length;
  foldPool.push(...seq);
  foldIndex.push([cp, start, seq.length]);
}

function mappingPool(map) {
  const pool = [];
  const index = [];
  for (const cp of [...map.keys()].sort((a, b) => a - b)) {
    const seq = map.get(cp);
    const start = pool.length;
    pool.push(...seq);
    index.push([cp, start, seq.length]);
  }
  return { pool, index };
}
const lower = mappingPool(simpleLower);
const upper = mappingPool(simpleUpper);
const title = mappingPool(simpleTitle);
const unicodeNameIndex = [...unicodeNames.entries()].sort((a, b) => a[0] - b[0]);
// Core text names scalars from the UNICODE_NAME_INDEX asset emitted below
// (core.text `unicode_name`); no Jet source decision tree is generated.

const HEADER_COMMENT = `// GENERATED FILE — do not hand-edit.
// Source: Tools/agent/gen-unicode-tables.mjs against pinned Unicode 17.0.0 UCD.
// Regenerate: node Tools/agent/gen-unicode-tables.mjs <ucd-dir-with-checksummed-files>
// Every runtime lookup and Core text asset is emitted from this one run.
`;
// ---- emit Jet runtime lookups -----------------------------------------------
// The runtime cannot use Core lists/Int indexing to implement Core itself.
// Balanced static decisions are the immutable tables: no heap, constructor,
// hidden exact-Int calls, or second global-data mechanism. Coalesce adjacent
// one-scalar mappings with the same delta before emitting the search tree.
// Full mappings pack up to three 21-bit scalars in a positive I64; the first
// scalar is always present, and zero in later lanes means no further scalar.
function jetMappingRows(map) {
  const rows = [];
  for (const [cp, seq] of [...map].sort((a, b) => a[0] - b[0])) {
    if (seq.length < 1 || seq.length > 3 || seq.some((s) => s <= 0 || s > 0x10ffff)) {
      throw new Error(`mapping cannot fit the Jet scalar lanes: ${cp}`);
    }
    const delta = seq.length === 1 ? seq[0] - cp : null;
    const last = rows.at(-1);
    if (delta !== null && last && last[1] + 1 === cp && last[2] === delta) {
      last[1] = cp;
    } else {
      const packed = seq.reduce((bits, s, lane) => bits | (BigInt(s) << BigInt(lane * 21)), 0n);
      rows.push([cp, cp, delta, packed]);
    }
  }
  return rows;
}
function jetLookupTree(rows, missing, mapped, indent = "    ") {
  if (!rows.length) return `${indent}return ${missing}\n`;
  const mid = Math.floor(rows.length / 2);
  const row = rows[mid];
  function arm(guard, children) {
    if (!children.length) return `${indent}    ${guard} -> return ${missing}\n`;
    return `${indent}    ${guard} -> {\n`
      + jetLookupTree(children, missing, mapped, indent + "        ")
      + `${indent}    }\n`;
  }
  return `${indent}if {\n`
    + arm(`(cp < I64{${row[0]}})`, rows.slice(0, mid))
    + arm(`(cp > I64{${row[1]}})`, rows.slice(mid + 1))
    + `${indent}    else -> return ${mapped(row)}\n`
    + `${indent}}\n`;
}
function jetMapping(name, map) {
  const rows = jetMappingRows(map);
  return `fn unicode_${name}_mapping(cp: I64) -> I64 Never! {\n`
    + jetLookupTree(rows, "cp", (row) => row[2] === null
      ? `I64{${row[3]}}`
      : row[2] === 0 ? "cp" : `wrapping(cp + I64{${row[2]}})`)
    + "}\n";
}
function jetProperty(name, ranges) {
  return `fn unicode_is_${name}(cp: I64) -> Bool Never! {\n`
    + jetLookupTree(mergePairRanges(ranges), "false", () => "true")
    + "}\n";
}
const jetTablesOut = HEADER_COMMENT.trimEnd() + "\n"
  + "// Static balanced tables: I64 only, no allocation or Core indexing.\n"
  + "// Mapping results contain three 21-bit scalar lanes (low lane first).\n"
  + jetMapping("lower", simpleLower) + "\n"
  + jetMapping("upper", simpleUpper) + "\n"
  + jetProperty("white_space", whiteSpaceRanges) + "\n"
  + jetProperty("cased", casedRanges) + "\n"
  + jetProperty("case_ignorable", caseIgnorableRanges);
if ([...canonDecomp.values()].some((seq) => seq.length > 2)) {
  throw new Error("canonical decomposition exceeds the NFC two-lane contract");
}
const jetNormalizeHeader = HEADER_COMMENT.trimEnd() + "\n"
  + Object.entries(PINNED_SHA256).map(([file, hash]) => `// SHA256 ${file}: ${hash}\n`).join("")
  + "// NFC canonical-only tables; static I64 decisions, no allocation/Core indexing.\n"
  + "// Canonical mappings pack two 21-bit scalar lanes (low lane first).\n";
const jetNormalizeChunks = [];
// Bound each lexer/parser input as well as each decision tree. 128 rows keep
// the generated functions small without changing the balanced search policy.
function jetNormalizeLookup(name, rows, missing, mapped) {
  const dispatch = [];
  for (let start = 0; start < rows.length; start += 128) {
    const chunk = rows.slice(start, start + 128);
    const index = dispatch.length;
    const fn = `unicode_${name}_${index}`;
    const text = jetNormalizeHeader + `fn ${fn}(cp: I64) -> I64 Never! {\n`
      + jetLookupTree(chunk, missing, mapped) + "}\n";
    if (text.length > jetTablesOut.length) throw new Error(`${fn} exceeds existing UnicodeTables.jet size`);
    jetNormalizeChunks.push([`Compiler/JetRuntime/Source/UnicodeNormalize${name.split("_").map(p => p[0].toUpperCase() + p.slice(1)).join("")}${index}.jet`, text]);
    dispatch.push([chunk[0][0], chunk.at(-1)[1], fn]);
  }
  return `fn unicode_${name}(cp: I64) -> I64 Never! {\n`
    + jetLookupTree(dispatch, missing, (row) => `${row[2]}(cp)`) + "}\n";
}
const jetNormalizeTablesOut = jetNormalizeHeader
  + jetNormalizeLookup("canonical_mapping", jetMappingRows(canonDecomp), "cp", (row) => row[2] === null
    ? `I64{${row[3]}}` : row[2] === 0 ? "cp" : `wrapping(cp + I64{${row[2]}})`) + "\n"
  + jetNormalizeLookup("normal_ccc", cccRanges, "I64{0}", (row) => `I64{${row[2]}}`) + "\n"
  + jetNormalizeLookup("normal_compose_key", composePairs.map(([a, b, c]) => {
    const key = (BigInt(a) << 21n) | BigInt(b);
    return [key, key, c];
  }), "I64{-1}", (row) => `I64{${row[2]}}`) + "\n"
  + "fn unicode_normal_compose(a: I64, b: I64) -> I64 Never! {\n"
  + "    unicode_normal_compose_key((a << I64{21}).bit_or(b))\n}\n";

// Native text-method lookups (title/capitalize/casefold, istitle, padding
// width and its UAX #29 grapheme views) from the same parsed rows; a tagged
// lookup returns its row's tag and an unlisted scalar the given default.
// Lookups are packed in order into UnicodeText_<n>.jet units, each within the
// existing UnicodeTables.jet size (one lookup alone must also fit).
function jetTagged(name, ranges, missing) {
  return `fn unicode_${name}(cp: I64) -> I64 Never! {\n`
    + jetLookupTree(ranges.filter((row) => row[2] !== missing), `I64{${missing}}`, (row) => `I64{${row[2]}}`)
    + "}\n";
}
const jetTextHeader = HEADER_COMMENT.trimEnd() + "\n"
  + "// Native text-method lookups; static I64 decisions, no allocation/Core indexing.\n"
  + "// Mapping results contain three 21-bit scalar lanes (low lane first).\n";
const jetTextLookups = [
  jetMapping("title", simpleTitle),
  jetMapping("fold", caseFold),
  jetProperty("lowercase", lowercaseRanges),
  jetProperty("uppercase", uppercaseRanges),
  // General_Category Cc (gc tag 0): jet_text_is_control.
  jetProperty("control", gcRanges.filter((row) => row[2] === 0)),
  // jet_text_is_zero_width_scalar: gc Mn/Me (tags 2, 3) or Default_Ignorable_Code_Point.
  jetProperty("zero_width", [...gcRanges.filter((row) => row[2] === 2 || row[2] === 3), ...defaultIgnorableRanges]),
  // jet_text_eaw_class == 2: East_Asian_Width W/F.
  jetProperty("east_asian_wide", eawMerged.filter((row) => row[2] === 2)),
  jetProperty("emoji", emoji),
  jetProperty("emoji_presentation", emojiPresentation),
  jetProperty("extended_pictographic", extPictographic),
  jetTagged("incb_class", incbRanges, 0),
  jetTagged("grapheme_class", graphemeRanges, 0),
];
const jetTextUnits = [];
for (const lookup of jetTextLookups) {
  const last = jetTextUnits.at(-1);
  if (last && last[1].length + 1 + lookup.length <= jetTablesOut.length) {
    last[1] += "\n" + lookup;
    continue;
  }
  const text = jetTextHeader + lookup;
  if (text.length > jetTablesOut.length) throw new Error(`text lookup exceeds existing UnicodeTables.jet size:\n${lookup.slice(0, 80)}`);
  jetTextUnits.push([`Compiler/JetRuntime/Source/UnicodeText${jetTextUnits.length}.jet`, text]);
}

const outPaths = [
  ["Compiler/JetRuntime/Source/UnicodeTables.jet", jetTablesOut],
  ["Compiler/JetRuntime/Source/UnicodeNormalizeTables.jet", jetNormalizeTablesOut],
  ...jetNormalizeChunks,
  ...jetTextUnits,
];
// Reused JUCD schema-1 encoding from 7c42badbb. These five private Core data
// tables use the same parsed/pinned ranges as the runtime lookups above.
function le(value, width) {
  if (!Number.isSafeInteger(value) || value < 0 || value >= 2 ** (width * 8))
    throw new Error(`asset field overflow: ${value}/${width}`);
  return Array.from({ length: width }, (_, i) => Math.floor(value / 2 ** (i * 8)) & 255);
}
function segmentationAsset(symbol, rows, tagged) {
  const widths = tagged ? [3, 3, 1] : [3, 3];
  const stride = tagged ? 7 : 6;
  const records = rows.flatMap(row => row.flatMap((field, i) => le(field, widths[i])));
  const size = 40 + records.length;
  const bytes = Buffer.from([
    74, 85, 67, 68, ...le(1, 2), 0, 0, 17, 0, 0, 0,
    ...[tagged ? 3 : 2, rows.length, stride, 40, size, 0, size].flatMap(n => le(n, 4)),
    ...records,
  ]);
  for (let row = 0; row < rows.length; row++) {
    let at = 40 + row * stride;
    for (let field = 0; field < widths.length; field++) {
      if (bytes.readUIntLE(at, widths[field]) !== rows[row][field])
        throw new Error(`asset round trip: ${symbol}/${row}/${field}`);
      at += widths[field];
    }
  }
  const literal = Array.from(bytes, b => `\\x${b.toString(16).padStart(2, "0").toUpperCase()}`).join("");
  return `// ${rows.length} JUCD records, ${stride} bytes each.\n${symbol} :: [U8]{"${literal}"}\n`;
}
const segmentationTables = [
  ["UNICODE_WORD_BREAK", wordRanges, true],
  ["UNICODE_SENTENCE_BREAK", sentenceRanges, true],
  ["UNICODE_EXTENDED_PICTOGRAPHIC", extPictographic, false],
  ["UNICODE_ALPHABETIC", mergePairRanges(alphabeticRanges), false],
  ["UNICODE_NUMERIC", mergePairRanges(numericRanges), false],
];
outPaths.push(["Core/Text/SegmentationTables.jet",
  HEADER_COMMENT + "// Compiler-private data part; JUCD schema 1, Unicode 17.0.0, little-endian.\n"
  + segmentationTables.map(([symbol, rows, tagged]) => segmentationAsset(symbol, rows, tagged)).join("\n")]);
for (const [rel, text] of outPaths) {
  const full = path.join(process.cwd(), rel);
  if (checkOnly) {
    const current = readFileSync(full, "utf8");
    if (current !== text) throw new Error(`${rel} is stale; regenerate from pinned Unicode 17.0.0`);
    console.log(`ok ${rel} (${text.length} bytes)`);
  } else {
    writeFileSync(full, text);
    console.log(`wrote ${rel} (${text.length} bytes)`);
  }
}

// Shared immutable assets: explicit little-endian fields, never native tuples.
// Header (40 bytes): JUCD, schema:u16, reserved:u16, version:3*u8, reserved:u8,
// kind:u32, count:u32, stride:u32, records_offset:u32, pool_offset:u32,
// pool_bytes:u32, total_bytes:u32. Offsets are byte offsets from the asset start.
// Kinds: 1 scalar pool; 2 ranges; 3 tagged ranges; 4 mapping index;
// 5 decomposition index; 6 composition; 7 simple fold; 8 UTF-8 names.
const assetRoot = "Core/Text/Runtime/Assets";
function bytesLiteral(bytes) {
  return `[U8]{"${Array.from(bytes, b => `\\x${b.toString(16).padStart(2, "0").toUpperCase()}`).join("")}"}`;
}
const assets = [];
function asset(symbol, kind, rows, widths, namePool = []) {
  // The name asset decodes back to the parsed UCD names before it is encoded.
  if (kind === 8) {
    const decoded = rows.map(([cp, offset, length]) => [cp, Buffer.from(namePool.slice(offset, offset + length)).toString("utf8")]);
    assert.deepEqual(decoded, unicodeNameIndex, `name pool drift: ${symbol}`);
  }
  const stride = widths.reduce((a, b) => a + b, 0);
  const records = rows.flatMap(row => row.flatMap((field, i) => le(field, widths[i])));
  const poolOffset = 40 + records.length;
  const size = poolOffset + namePool.length;
  const bytes = Buffer.from([
    74, 85, 67, 68, ...le(1, 2), 0, 0, 17, 0, 0, 0,
    ...[kind, rows.length, stride, 40, poolOffset, namePool.length, size].flatMap(n => le(n, 4)),
    ...records, ...namePool,
  ]);
  // Decode all records offline, not at startup or per runtime lookup.
  if (bytes.length !== size || bytes.readUInt32LE(36) !== size) throw new Error("asset extent");
  for (let row = 0; row < rows.length; row++) {
    let at = 40 + row * stride;
    for (let field = 0; field < widths.length; field++) {
      if (bytes.readUIntLE(at, widths[field]) !== rows[row][field]) throw new Error("asset round trip");
      at += widths[field];
    }
  }
  const module = symbol.toLowerCase();
  const source = `${HEADER_COMMENT}// JUCD schema 1; little-endian; ${rows.length} records of ${stride} bytes.\n${symbol} :: ${bytesLiteral(bytes)}\n`;
  assets.push({symbol, module, kind, count: rows.length, stride, bytes: size,
    sha256: createHash("sha256").update(bytes).digest("hex"), source});
}
asset("UNICODE_DECOMP_POOL", 1, pool.map(n => [n]), [3]);
asset("UNICODE_DECOMP_INDEX", 5, decompIndex, [3, 2, 1, 1]);
asset("UNICODE_CCC", 3, cccRanges, [3, 3, 1]);
asset("UNICODE_COMPOSE_PAIRS", 6, composePairs, [3, 3, 3]);
for (const [name, mapping] of [["FOLD", {pool: foldPool, index: foldIndex}],
  ["LOWER", lower], ["UPPER", upper], ["TITLE", title]]) {
  asset(`UNICODE_${name}_POOL`, 1, mapping.pool.map(n => [n]), [3]);
  asset(`UNICODE_${name}_INDEX`, 4, mapping.index, [3, 4, 4]);
  for (const [cp, start, length] of mapping.index) {
    if (start + length > mapping.pool.length || length === 0) throw new Error(`mapping extent: ${cp}`);
  }
}
asset("UNICODE_SIMPLE_FOLD", 7, [...simpleCaseFold.entries()].sort((a,b) => a[0]-b[0]), [3,3]);
for (const [name, rows] of [
  ["WHITE_SPACE", mergePairRanges(whiteSpaceRanges)], ["CASED", mergePairRanges(casedRanges)],
  ["CASE_IGNORABLE", mergePairRanges(caseIgnorableRanges)], ["LETTER", mergePairRanges(letterRanges)],
  ["LOWERCASE", mergePairRanges(lowercaseRanges)], ["UPPERCASE", mergePairRanges(uppercaseRanges)],
  ["NUMERIC", mergePairRanges(numericRanges)], ["ALPHABETIC", mergePairRanges(alphabeticRanges)],
  ["DEFAULT_IGNORABLE", mergePairRanges(defaultIgnorableRanges)],
  ["COMPOSITION_EXCLUSIONS", fullCompositionExclusion], ["EXTENDED_PICTOGRAPHIC", extPictographic],
  ["EMOJI_PRESENTATION", emojiPresentation], ["EMOJI", emoji],
]) asset(`UNICODE_${name}`, 2, rows, [3,3]);
for (const [name, rows] of [
  ["GENERAL_CATEGORY", gcRanges], ["EAST_ASIAN_WIDTH", eawMerged],
  ["GRAPHEME_BREAK", graphemeRanges], ["WORD_BREAK", wordRanges],
  ["SENTENCE_BREAK", sentenceRanges], ["INCB", incbRanges],
]) asset(`UNICODE_${name}`, 3, rows, [3,3,1]);
const namesPool = [];
const namesIndex = unicodeNameIndex.map(([cp, name]) => {
  const bytes = [...Buffer.from(name, "utf8")];
  const row = [cp, namesPool.length, bytes.length];
  namesPool.push(...bytes);
  return row;
});
asset("UNICODE_NAME_INDEX", 8, namesIndex, [3,4,4], namesPool);
if (!checkOnly) mkdirSync(assetRoot, {recursive: true});
// Core source files are PascalCase; the module names inside them stay snake_case.
const pascalFile = (module) => module.split("_").map((part) => part[0].toUpperCase() + part.slice(1)).join("");
const coreOutputs = assets.map(a => [`${assetRoot}/${pascalFile(a.module)}.jet`, a.source]);
coreOutputs.push([`${assetRoot}/Assets.jet`,
  `${HEADER_COMMENT}${assets.map(a => `module ${a.module}`).join("\n")}\n`]);
coreOutputs.push([`${assetRoot}/manifest.json`, JSON.stringify({
  schema: 1, unicode: "17.0.0", byteOrder: "little", inputs: PINNED_SHA256,
  assets: assets.map(({source, ...facts}) => facts),
}, null, 2) + "\n"]);
const stringModules = assets.filter(a => /^UNICODE_(LOWER|UPPER|WHITE_SPACE|CASED|CASE_IGNORABLE)/.test(a.symbol));
function declarations(list) {
  return list.map(a => `use core.text.runtime.assets.${a.module} as text_${a.module}`).join("\n");
}
// String asset modules are attached to their canonical kernel, not normalization.
const stringPath = "Core/Text/Runtime/KernelUnicodeString.jet";
const stringSource = readFileSync(stringPath, "utf8");
const begin = "// BEGIN GENERATED STRING ASSETS";
const end = "// END GENERATED STRING ASSETS";
const start = stringSource.indexOf(begin);
const stop = stringSource.indexOf(end, start);
if (start < 0 || stop < start) throw new Error("string asset markers missing");
coreOutputs.push([stringPath, stringSource.slice(0, start) + `${begin}\n${declarations(stringModules)}\n${end}` + stringSource.slice(stop + end.length)]);
for (const [rel, text] of coreOutputs) {
  if (checkOnly) {
    if (readFileSync(rel, "utf8") !== text) throw new Error(`${rel} is stale`);
  } else writeFileSync(rel, text);
}
console.log(`${checkOnly ? "checked" : "generated"} ${assets.length} JUCD schema-1 Unicode 17.0.0 assets; all records round-trip`);
console.log(checkOnly ? "Unicode tables are byte-identical." : "done.");
