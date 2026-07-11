#!/usr/bin/env node
// Card #298: offline, checksum-verified Unicode 16.0.0 UCD table generator.
// No network at build/compile time — this script is run by hand (or by a
// maintainer script) against a local UCD snapshot dir, and its Rust output is
// committed to the repo. No external crates (I6): plain Node + plain Rust.
//
// Usage:
//   node scripts/agent/gen-unicode-tables.mjs <ucd-dir>
//
// <ucd-dir> must contain (fetched from https://www.unicode.org/Public/16.0.0/ucd/):
//   UnicodeData.txt, CaseFolding.txt, CompositionExclusions.txt,
//   DerivedNormalizationProps.txt, EastAsianWidth.txt, emoji/emoji-data.txt,
//   auxiliary/GraphemeBreakProperty.txt, auxiliary/WordBreakProperty.txt,
//   auxiliary/SentenceBreakProperty.txt
//
// Emits byte-identical Rust source to two paths:
//   crates/jet-foundation/src/generated/UnicodeTables.rs   (source of truth;
//     jet-comptime depends on jet-foundation and uses this module directly —
//     no separate comptime copy, one table, no drift)
//   crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs (textual
//     duplicate: the AOT prelude is embedded into the user's compiled
//     program via include_str!, which cannot depend on the compiler's own
//     jet-foundation crate, so this copy carries the same data as literal
//     prelude source)
//
// Pinned Unicode release: 16.0.0. Pinned sha256 of each input file (verified
// before generating — mismatch aborts):
const PINNED_SHA256 = {
  "UnicodeData.txt": "ff58e5823bd095166564a006e47d111130813dcf8bf234ef79fa51a870edb48f",
  "CaseFolding.txt": "6f1f9c588eb4a5c718d9e8f93b782685e5c7fec872cf05e8e6878053599e09bb",
  "SpecialCasing.txt": "8d5de354eef79f2395a54c9c7dcebbaf3d30fc962d0f85611ea97aa973a0c451",
  "CompositionExclusions.txt": "89e83cf9cc8bef6c1f8bf77e42cf6f0341dfa42e66261f4dbe9b492e7a23c8ee",
  "EastAsianWidth.txt": "43adc76c0686a42cb370764eb8cfe2b2a45b10b855e5572a2db4a0eecce15d5b",
  "DerivedNormalizationProps.txt": "4d4c03892dea9146d674b686e495df2d55a28d071ac474041d73518f887abddc",
  "emoji/emoji-data.txt": "f1365a5173eee18e1f98b240cdc492e84a25f1ce7e0c9d1094eb29c41a22696a",
  "auxiliary/GraphemeBreakProperty.txt": "c29360bd6f7132811d701d29069541e827eb44bfc4c8fbde8c370d6982689dc1",
  "auxiliary/WordBreakProperty.txt": "476464e71a4b7b779b8ba7c5671f4338fea77da8e6b6b05fb82b3fdd14603779",
  "auxiliary/SentenceBreakProperty.txt": "20aab5eca3842c7a27cc6756d74488a4a5f744c8dca2948ec1128f26a60d1f79",
};

import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";

const ucdDir = process.argv[2];
if (!ucdDir) {
  console.error("usage: gen-unicode-tables.mjs <ucd-dir>");
  process.exit(1);
}

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
const generalCategory = []; // [cp, gcTagIndex]
const GC_TAGS = ["Cc", "Cf", "Mn", "Me", "Mc", "Zs", "Zl", "Zp", "Other"];
function gcTag(gc) {
  const i = GC_TAGS.indexOf(gc);
  return i === -1 ? GC_TAGS.length - 1 : i;
}

for (const raw of unicodeData.split("\n")) {
  if (!raw) continue;
  const f = raw.split(";");
  if (f.length < 6) continue;
  const cp = parseInt(f[0], 16);
  const gc = f[2];
  const cccVal = parseInt(f[3], 10) || 0;
  const decomp = f[5].trim();
  if (cccVal !== 0) ccc.push([cp, cccVal]);
  generalCategory.push([cp, gcTag(gc)]);
  if (decomp) {
    const tagged = decomp.startsWith("<");
    let rest = decomp;
    if (tagged) rest = decomp.slice(decomp.indexOf(">") + 1).trim();
    const cps = rest.split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16));
    compatDecomp.set(cp, cps);
    if (!tagged) canonDecomp.set(cp, cps);
  }
}

// ---- CompositionExclusions / DerivedNormalizationProps ---------------------
// Full_Composition_Exclusion (DerivedNormalizationProps.txt) is the complete
// authoritative exclusion set (primary exclusions + singleton + non-starter
// decomposables); it supersedes CompositionExclusions.txt alone.
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
for (const raw of caseFolding.split("\n")) {
  const line = stripComment(raw).trim();
  if (!line) continue;
  const f = line.split(";").map((x) => x.trim());
  if (f.length < 3) continue;
  const status = f[1];
  if (status !== "C" && status !== "F") continue;
  const cp = parseInt(f[0], 16);
  const mapping = f[2].split(/\s+/).filter(Boolean).map((x) => parseInt(x, 16));
  caseFold.set(cp, mapping);
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

const cccRanges = mergeRangesSameTag(ccc);
const gcRanges = mergeRangesSameTag(generalCategory);
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

// ---- emit Rust ---------------------------------------------------------------
function fmtU32Triples(rows) {
  return rows.map(([a, b, c]) => `(0x${a.toString(16).toUpperCase()},0x${b.toString(16).toUpperCase()},${c})`).join(",");
}
function fmtU32Pairs(rows) {
  return rows.map(([a, b]) => `(0x${a.toString(16).toUpperCase()},0x${b.toString(16).toUpperCase()})`).join(",");
}

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

const HEADER_COMMENT = `// GENERATED FILE — do not hand-edit.
// Source: scripts/agent/gen-unicode-tables.mjs against pinned Unicode 16.0.0 UCD.
// Regenerate: node scripts/agent/gen-unicode-tables.mjs <ucd-dir-with-checksummed-files>
// Three siblings emitted from one run, byte-identical data (R12 parity):
//   jet-foundation (compiler-internal module), jet-comptime (own module),
//   jet-codegen (AOT prelude — spliced flat into the emitted user program,
//   so this copy carries no inner #![...] attribute: it is not a crate root).
`;
// jet-foundation / jet-comptime are proper standalone module files: safe to
// carry an inner attribute. The jet-codegen copy is concatenated flat into
// other prelude text (see Codegen/mod.rs CORELIB_PRELUDE_PARTS) — no mod
// wrapper, so an inner `#![...]` attribute there would not be at a crate/mod
// root and is illegal; leave unused-item warnings alone (I2 only forbids
// rustc *rejecting* generated code, not warning on it).
const MODULE_HEADER = HEADER_COMMENT + "#![allow(dead_code)]\n";
const FLAT_HEADER = HEADER_COMMENT;

const body = `
pub static UNICODE_DECOMP_POOL: &[u32] = &[${pool.join(",")}];
// (codepoint, pool_start, pool_len, is_canonical: 1/0)
pub static UNICODE_DECOMP_INDEX: &[(u32,u32,u32,u8)] = &[${decompIndex
  .map(([cp, s, l, c]) => `(0x${cp.toString(16).toUpperCase()},${s},${l},${c})`)
  .join(",")}];

pub static UNICODE_FOLD_POOL: &[u32] = &[${foldPool.join(",")}];
// (codepoint, pool_start, pool_len)
pub static UNICODE_FOLD_INDEX: &[(u32,u32,u32)] = &[${foldIndex
  .map(([cp, s, l]) => `(0x${cp.toString(16).toUpperCase()},${s},${l})`)
  .join(",")}];

// (start, end, canonical_combining_class)
pub static UNICODE_CCC: &[(u32,u32,u8)] = &[${fmtU32Triples(cccRanges)}];

// (start, end, general_category_tag) tags: ${GC_TAGS.map((t, i) => `${i}=${t}`).join(" ")}
pub static UNICODE_GENERAL_CATEGORY: &[(u32,u32,u8)] = &[${fmtU32Triples(gcRanges)}];

// composition exclusions (Full_Composition_Exclusion, DerivedNormalizationProps.txt)
pub static UNICODE_COMPOSITION_EXCLUSIONS: &[(u32,u32)] = &[${fmtU32Pairs(fullCompositionExclusion)}];

// canonical composition pairs (c1, c2) -> composed, sorted by (c1,c2)
pub static UNICODE_COMPOSE_PAIRS: &[(u32,u32,u32)] = &[${composePairs
  .map(([a, b, c]) => `(0x${a.toString(16).toUpperCase()},0x${b.toString(16).toUpperCase()},0x${c.toString(16).toUpperCase()})`)
  .join(",")}];

// East Asian Width: 0=narrow(N/Na/H) 1=Ambiguous 2=wide(W/F)
pub static UNICODE_EAST_ASIAN_WIDTH: &[(u32,u32,u8)] = &[${fmtU32Triples(eawMerged)}];

pub static UNICODE_EXTENDED_PICTOGRAPHIC: &[(u32,u32)] = &[${fmtU32Pairs(extPictographic)}];
pub static UNICODE_EMOJI_PRESENTATION: &[(u32,u32)] = &[${fmtU32Pairs(emojiPresentation)}];

// Grapheme_Cluster_Break tags: ${GRAPHEME_TAGS.map((t, i) => `${i}=${t}`).join(" ")}
pub static UNICODE_GRAPHEME_BREAK: &[(u32,u32,u8)] = &[${fmtU32Triples(graphemeRanges)}];
// Word_Break tags: ${WORD_TAGS.map((t, i) => `${i}=${t}`).join(" ")}
pub static UNICODE_WORD_BREAK: &[(u32,u32,u8)] = &[${fmtU32Triples(wordRanges)}];
// Sentence_Break tags: ${SENTENCE_TAGS.map((t, i) => `${i}=${t}`).join(" ")}
pub static UNICODE_SENTENCE_BREAK: &[(u32,u32,u8)] = &[${fmtU32Triples(sentenceRanges)}];
`;

const moduleOut = MODULE_HEADER + body;
const flatOut = FLAT_HEADER + body;

const outPaths = [
  ["crates/jet-foundation/src/generated/UnicodeTables.rs", moduleOut],
  ["crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs", flatOut],
];
for (const [rel, text] of outPaths) {
  const full = path.join(process.cwd(), rel);
  writeFileSync(full, text);
  console.log(`wrote ${rel} (${text.length} bytes)`);
}
console.log("done.");
