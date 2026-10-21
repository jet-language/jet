#!/usr/bin/env node
// Lost-work audit: finds commits whose result silently dropped earlier work.
//
// Signals, over every commit reachable from --rev:
//   merge-drop      a merge file whose content differs from a clean 3-way
//                   combination of its parents: lines one side added since the
//                   merge base are missing (lost), or lines one side deleted came
//                   back (resurrected). Measured on trimmed, non-punctuation line
//                   multisets, so a clean merge scores 0 whatever the conflict
//                   markers looked like.
//   revert-like     a non-merge commit that changes more than --min-lines lines
//                   and leaves a file closer to an OLDER version of that path
//                   than to the version it replaced (re-adding a deleted path
//                   compares against the last version before the deletion), and
//                   the replaced version's newer lines are gone.
//   resurrection    any commit that sets a path to the exact blob it had in an
//                   older ancestor while a newer version existed in between.
//   symbol-drop     declarations (Jet `fn`/types/`#Test("...")`, Rust `fn`/types,
//                   JS `function`/`test(...)`) whose count over the commit's
//                   changed files fell below what the parents supply: for merges
//                   the clean 3-way expectation, for other commits the parent
//                   (tests only, unless the file is also revert-like). Moves
//                   inside one commit cancel out.
//
// Each sweep finding records whether the --rev tip still lacks it: the share of
// the lost lines present in the tip's version of the path (case- and
// separator-insensitive path match, which follows the PascalCase Core move) and
// whether the tip defines each dropped symbol anywhere.
//
// Usage:
//   lost-work-audit.mjs --check [--baseline PATH]
//   lost-work-audit.mjs --sweep [--rev REV] [--since REV] [--min-lines N] [--out DIR]
//                       [--history FILE] [--jobs N] [--shard-heap-mb MB]
//                       [--write-baseline] [--baseline PATH]
// --check verifies the committed baseline in seconds, from git objects only: its
// pinned revision, a known disposition for every reviewed finding, and every
// evidence assertion the review recorded (an object's blob or absence, a
// rev-list count, a tree's file count, all at full SHAs). It reports every
// failure in one pass and passes until the baseline is deliberately re-pinned.
// --sweep runs the full history sweep (minutes) and fails (exit 1) listing every
// commit finding whose key has no disposition in the baseline; --write-baseline
// adds those keys as `unreviewed`, which --check rejects until they are reviewed.
// --since limits the analysed commits to REV..--rev (history is still read in
// full for ancestry). --history FILE caches the history scan (one `git log --raw
// --numstat` pass, a few minutes on the full repo): read when present, written
// otherwise. --jobs N splits the commits over N shard processes (default 1).

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DEFAULT_BASELINE = join(REPO_ROOT, "Docs/audits/lost-work-2026-10-21.json");
const FULL_SHA = /^[0-9a-f]{40}$/;
// Reviewed outcomes a baseline finding may carry; `unreviewed` (what --write-baseline adds) is not one.
const DISPOSITIONS = ["restored", "lost", "candidate", "census", "superseded", "intended", "false-positive"];
const ZERO = "0000000000000000000000000000000000000000";
const MAX_BLOB = 2 * 1024 * 1024;
const OLDER_VERSIONS = 8;

function usage() {
  return [
    "usage: lost-work-audit.mjs --check [--baseline PATH]",
    "       lost-work-audit.mjs --sweep [--rev REV] [--since REV] [--min-lines N] [--out DIR]",
    "                           [--history FILE] [--jobs N] [--shard-heap-mb MB]",
    "                           [--write-baseline] [--baseline PATH]",
  ].join("\n");
}

function parseArgs(argv) {
  const opts = {
    rev: "HEAD",
    since: null,
    minLines: 200,
    mergeMin: 3,
    revertMin: 5,
    out: null,
    check: false,
    sweep: false,
    writeBaseline: false,
    baseline: DEFAULT_BASELINE,
    history: null,
    jobs: 1,
    shardHeapMb: 1024,
    shard: null,
    part: null,
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new Error(`${arg} needs a value\n${usage()}`);
      return argv[++i];
    };
    if (arg === "--rev") opts.rev = value();
    else if (arg === "--since") opts.since = value();
    else if (arg === "--min-lines") opts.minLines = Number(value());
    else if (arg === "--merge-min") opts.mergeMin = Number(value());
    else if (arg === "--revert-min") opts.revertMin = Number(value());
    else if (arg === "--out") opts.out = resolve(value());
    else if (arg === "--check") opts.check = true;
    else if (arg === "--sweep") opts.sweep = true;
    else if (arg === "--write-baseline") opts.writeBaseline = true;
    else if (arg === "--baseline") opts.baseline = resolve(value());
    else if (arg === "--history") opts.history = resolve(value());
    else if (arg === "--jobs") opts.jobs = Number(value());
    else if (arg === "--shard-heap-mb") opts.shardHeapMb = Number(value());
    else if (arg === "--shard") {
      const [index, of] = value().split("/").map(Number);
      opts.shard = { index, of };
    } else if (arg === "--part") opts.part = resolve(value());
    else if (arg === "--help" || arg === "-h") {
      console.log(usage());
      process.exit(0);
    } else throw new Error(`unknown argument ${arg}\n${usage()}`);
  }
  if (opts.shard) return opts;
  if (opts.check === opts.sweep) throw new Error(`choose --check or --sweep\n${usage()}`);
  if (opts.writeBaseline && !opts.sweep) throw new Error(`--write-baseline needs --sweep\n${usage()}`);
  if (opts.check && argv.some((a) => a.startsWith("--") && !["--check", "--baseline"].includes(a))) {
    throw new Error(`--check takes only --baseline; sweep options need --sweep\n${usage()}`);
  }
  return opts;
}

function git(args, input) {
  return execFileSync("git", args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024 * 1024,
    input,
  });
}

// ---- path classes --------------------------------------------------------

const CODE_EXT = /\.(jet|rs|mjs|cjs|js|ts|sh|py|toml)$/;
const GOLDEN_EXT = /\.(out|stderr|stdout|expected|golden)$/;
const DOC_EXT = /\.(md|html)$/;
const TEXT_GLOBS = ["*.jet", "*.rs", "*.mjs", "*.cjs", "*.js", "*.ts", "*.sh", "*.py", "*.toml", "*.md", "*.html", "*.out", "*.stderr", "*.stdout"];

function pathClass(path) {
  if (path.startsWith("Tools/tower/.tower/")) return null;
  if (CODE_EXT.test(path)) return "code";
  if (GOLDEN_EXT.test(path)) return "golden";
  if (DOC_EXT.test(path)) return "doc";
  return null;
}

function langOf(path) {
  if (path.endsWith(".jet")) return "jet";
  if (path.endsWith(".rs")) return "rust";
  if (/\.(mjs|cjs|js|ts)$/.test(path)) return "js";
  return null;
}

function headKey(path) {
  return path.toLowerCase().replace(/[_-]/g, "");
}

// ---- history -------------------------------------------------------------

function loadHistory(rev, cache) {
  const text =
    cache && existsSync(cache)
      ? readFileSync(cache, "utf8")
      : git([
          "log",
          "--topo-order",
          "--reverse",
          "--format=C %H %P%x09%s",
          "--raw",
          "--numstat",
          "--no-abbrev",
          "--no-renames",
          "--diff-merges=first-parent",
          rev,
        ]);
  if (cache && !existsSync(cache)) writeFileSync(cache, text);
  const commits = [];
  const index = new Map();
  let cur = null;
  for (const line of text.split("\n")) {
    if (line.startsWith("C ")) {
      const tab = line.indexOf("\t");
      const shas = line.slice(2, tab).trim().split(" ");
      cur = {
        i: commits.length,
        sha: shas[0],
        parents: shas.slice(1),
        subject: line.slice(tab + 1),
        changes: [],
        lines: 0,
      };
      index.set(cur.sha, cur.i);
      commits.push(cur);
    } else if (line.startsWith(":")) {
      const tab = line.indexOf("\t");
      const meta = line.slice(1, tab).split(" ");
      cur.changes.push({ path: line.slice(tab + 1), oldBlob: meta[2], newBlob: meta[3], status: meta[4] });
    } else if (line && cur) {
      const m = /^(\d+|-)\t(\d+|-)\t/.exec(line);
      if (m) cur.lines += (m[1] === "-" ? 0 : Number(m[1])) + (m[2] === "-" ? 0 : Number(m[2]));
    }
  }
  for (const c of commits) c.parentIdx = c.parents.map((p) => index.get(p));
  return { commits, index };
}

function ancestorSets(commits) {
  const words = (commits.length + 31) >>> 5;
  const anc = new Array(commits.length);
  for (const c of commits) {
    const set = new Uint32Array(words);
    for (const p of c.parentIdx) {
      if (p === undefined) continue;
      const ps = anc[p];
      for (let w = 0; w < words; w++) set[w] |= ps[w];
      set[p >>> 5] |= 1 << (p & 31);
    }
    anc[c.i] = set;
  }
  return (a, c) => a !== c && (anc[c][a >>> 5] & (1 << (a & 31))) !== 0;
}

function pathEvents(commits) {
  const events = new Map();
  for (const c of commits) {
    for (const ch of c.changes) {
      let list = events.get(ch.path);
      if (!list) events.set(ch.path, (list = []));
      list.push({ ci: c.i, blob: ch.newBlob === ZERO ? null : ch.newBlob });
    }
  }
  return events;
}

// ---- blobs and line multisets --------------------------------------------
// Blobs are read per chunk of paths and released after it (releaseBlobs), so a
// many-thousand-file merge never holds more than one chunk of contents.

const CHUNK = 200;
const MAX_LOST_LINES = 300;
const blobCache = new Map();

function readBlobs(shas) {
  const asked = [...new Set(shas.filter((s) => s && s !== ZERO && !blobCache.has(s)))];
  if (!asked.length) return;
  // Sizes first: generated files (stage-zero emissions, tables) run to tens of
  // megabytes per version and are never compared, so they are never read.
  const sizes = spawnSync("git", ["cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"], {
    cwd: REPO_ROOT,
    input: asked.join("\n") + "\n",
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  if (sizes.status !== 0) throw new Error(`git cat-file --batch-check failed: ${sizes.stderr}`);
  const want = [];
  for (const line of sizes.stdout.split("\n")) {
    const [sha, type, size] = line.split(" ");
    if (!sha) continue;
    if (type === "blob" && Number(size) <= MAX_BLOB) want.push(sha);
    else blobCache.set(sha, null);
  }
  if (!want.length) return;
  const res = spawnSync("git", ["cat-file", "--batch"], {
    cwd: REPO_ROOT,
    input: want.join("\n") + "\n",
    maxBuffer: 2 * 1024 * 1024 * 1024,
  });
  if (res.status !== 0) throw new Error(`git cat-file failed: ${res.stderr}`);
  const out = res.stdout;
  let pos = 0;
  while (pos < out.length) {
    const nl = out.indexOf(10, pos);
    const header = out.toString("utf8", pos, nl).split(" ");
    pos = nl + 1;
    if (header[1] === "missing") {
      blobCache.set(header[0], null);
      continue;
    }
    const size = Number(header[2]);
    const body = out.subarray(pos, pos + size);
    pos += size + 1;
    const probe = body.subarray(0, 8000);
    blobCache.set(header[0], size > MAX_BLOB || probe.includes(0) ? null : body.toString("utf8"));
  }
}

// Lines are slices of whole blob texts; a kept line is copied so a finding never
// pins the blob it came from.
function own(line) {
  return Buffer.from(line, "utf8").toString("utf8");
}

function releaseBlobs() {
  blobCache.clear();
  multisetCache.clear();
}

function chunks(list) {
  const out = [];
  for (let i = 0; i < list.length; i += CHUNK) out.push(list.slice(i, i + CHUNK));
  return out;
}

function blobText(sha) {
  if (!sha || sha === ZERO) return "";
  return blobCache.get(sha) ?? null;
}

const SIGNIFICANT = /[A-Za-z0-9]/;
const multisetCache = new Map();

function lineMultiset(sha) {
  if (!sha || sha === ZERO) return new Map();
  const hit = multisetCache.get(sha);
  if (hit) return hit;
  const text = blobText(sha);
  const ms = new Map();
  if (text !== null) {
    for (const raw of text.split("\n")) {
      const line = raw.trim();
      if (!SIGNIFICANT.test(line)) continue;
      ms.set(line, (ms.get(line) ?? 0) + 1);
    }
  }
  multisetCache.set(sha, ms);
  return ms;
}

function distance(a, b) {
  let d = 0;
  for (const [k, n] of a) d += Math.abs(n - (b.get(k) ?? 0));
  for (const [k, n] of b) if (!a.has(k)) d += n;
  return d;
}

// ---- declarations ----------------------------------------------------------

const DECL = {
  jet: [
    [/^\s*(?:(?:pub|private|public|extern|export|unsafe|async|const|comptime|inline)\s+|#\w+(?:\([^)]*\))?\s+)*fn\s+([A-Za-z_][\w]*)/, "fn"],
    [/^\s*(?:(?:pub|private|public)\s+)*(?:struct|enum|trait|effect|protocol|class|type|choice|union)\s+([A-Z][\w]*)/, "type"],
    [/^\s*#Test\("((?:[^"\\]|\\.)*)"/, "test"],
  ],
  rust: [
    [/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe|extern\s+"[^"]*")\s+)*fn\s+([A-Za-z_]\w*)/, "fn"],
    [/^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union|type)\s+([A-Z]\w*)/, "type"],
  ],
  js: [
    [/^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)/, "fn"],
    [/^\s*(?:export\s+)?class\s+([A-Za-z_$][\w$]*)/, "type"],
    [/^\s*(?:test|it)(?:\.only|\.skip)?\(\s*(["'`])((?:(?!\1)[^\\]|\\.)*)\1/, "test", 2],
  ],
};

function declarations(path, sha) {
  const lang = langOf(path);
  const out = new Map();
  if (!lang || !sha || sha === ZERO) return out;
  const text = blobText(sha);
  if (text === null) return out;
  const rules = DECL[lang];
  const lines = text.split("\n");
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n];
    for (const [re, kind, group = 1] of rules) {
      const m = re.exec(line);
      if (!m) continue;
      let k = kind;
      if (lang === "rust" && kind === "fn") {
        for (let back = n - 1; back >= 0 && back >= n - 4; back--) {
          const prev = lines[back].trim();
          if (/^#\[(tokio::)?test\b/.test(prev)) k = "test";
          if (!prev.startsWith("#[") && !prev.startsWith("//")) break;
        }
      }
      const key = `${k}:${m[group]}`;
      out.set(key, (out.get(key) ?? 0) + 1);
      break;
    }
  }
  return out;
}

function addInto(total, ms, sign) {
  for (const [k, n] of ms) total.set(k, (total.get(k) ?? 0) + sign * n);
}

// ---- merge analysis --------------------------------------------------------

function diffTree(a, b) {
  const map = new Map();
  const text = git(["diff-tree", "-r", "--no-renames", "--no-abbrev", a, b]);
  for (const line of text.split("\n")) {
    if (!line.startsWith(":")) continue;
    const tab = line.indexOf("\t");
    const meta = line.slice(1, tab).split(" ");
    map.set(line.slice(tab + 1), { from: meta[2], to: meta[3] });
  }
  return map;
}

function threeWay(b, x, y, r) {
  // Per line: a clean merge holds x + y - b copies.
  const keys = new Set([...b.keys(), ...x.keys(), ...y.keys(), ...r.keys()]);
  const lost = { ours: 0, theirs: 0 };
  const samples = { ours: [], theirs: [], resurrected: [] };
  const lostLines = [];
  let resurrected = 0;
  for (const k of keys) {
    const bn = b.get(k) ?? 0;
    const xn = x.get(k) ?? 0;
    const yn = y.get(k) ?? 0;
    const rn = r.get(k) ?? 0;
    const expected = Math.max(0, xn + yn - bn);
    if (rn < expected) {
      const deficit = expected - rn;
      const fromOurs = Math.min(deficit, Math.max(0, xn - bn));
      const fromTheirs = Math.min(deficit - fromOurs, Math.max(0, yn - bn));
      lost.ours += fromOurs;
      lost.theirs += fromTheirs;
      if (fromOurs && samples.ours.length < 4) samples.ours.push(own(k));
      if (fromTheirs && samples.theirs.length < 4) samples.theirs.push(own(k));
      if (fromOurs + fromTheirs && lostLines.length < MAX_LOST_LINES) lostLines.push(own(k));
    } else if (rn > expected) {
      const back = Math.min(rn - expected, Math.max(0, bn - Math.min(xn, yn)));
      resurrected += back;
      if (back && samples.resurrected.length < 4) samples.resurrected.push(own(k));
    }
  }
  return { lost, resurrected, samples, lostLines };
}

function analyseMerge(c, commits, opts, findings) {
  const [p1, p2] = c.parents;
  if (c.parents.length > 2) {
    findings.push({ kind: "octopus-merge", sha: c.sha, path: "", subject: c.subject, note: `${c.parents.length} parents` });
  }
  let base;
  try {
    base = git(["merge-base", p1, p2]).trim();
  } catch {
    base = null;
  }
  if (!base) return;
  const ours = diffTree(base, p1);
  const theirs = diffTree(base, p2);
  const result = new Map(c.changes.map((ch) => [ch.path, ch.newBlob]));
  const paths = new Set([...ours.keys(), ...theirs.keys(), ...result.keys()]);
  const suspects = [];
  for (const path of paths) {
    if (!pathClass(path)) continue;
    const o = ours.get(path);
    const t = theirs.get(path);
    const bBlob = o?.from ?? t?.from ?? null;
    const xBlob = o ? o.to : bBlob;
    const yBlob = t ? t.to : bBlob;
    const rBlob = result.has(path) ? result.get(path) : xBlob;
    let expected;
    if (xBlob === yBlob) expected = xBlob;
    else if (xBlob === bBlob) expected = yBlob;
    else if (yBlob === bBlob) expected = xBlob;
    else expected = undefined; // both sides changed: needs content
    if (expected !== undefined && rBlob === expected) continue;
    if (bBlob === null) continue; // path unknown to the base and unchanged by either side
    suspects.push({ path, bBlob, xBlob, yBlob, rBlob });
  }
  if (!suspects.length) return;
  const symExpected = new Map();
  const symResult = new Map();
  const perFile = [];
  for (const chunk of chunks(suspects)) {
    readBlobs(chunk.flatMap((s) => [s.bBlob, s.xBlob, s.yBlob, s.rBlob]));
    for (const s of chunk) {
      const ms = [s.bBlob, s.xBlob, s.yBlob, s.rBlob].map(lineMultiset);
      const tw = threeWay(...ms);
      const total = tw.lost.ours + tw.lost.theirs + tw.resurrected;
      const db = declarations(s.path, s.bBlob);
      const dx = declarations(s.path, s.xBlob);
      const dy = declarations(s.path, s.yBlob);
      const dr = declarations(s.path, s.rBlob);
      const keys = new Set([...db.keys(), ...dx.keys(), ...dy.keys()]);
      for (const k of keys) {
        const exp = Math.max(0, (dx.get(k) ?? 0) + (dy.get(k) ?? 0) - (db.get(k) ?? 0));
        symExpected.set(k, (symExpected.get(k) ?? 0) + exp);
      }
      addInto(symResult, dr, 1);
      if (total >= opts.mergeMin) perFile.push({ s, tw, total, dx, dy, dr, db });
    }
    releaseBlobs();
  }
  const droppedSymbols = new Set();
  for (const [k, exp] of symExpected) {
    const got = symResult.get(k) ?? 0;
    if (got < exp) droppedSymbols.add(k);
  }
  for (const f of perFile) {
    const symbols = [...new Set([...f.db.keys(), ...f.dx.keys(), ...f.dy.keys()])].filter((k) => {
      if (!droppedSymbols.has(k)) return false;
      const exp = Math.max(0, (f.dx.get(k) ?? 0) + (f.dy.get(k) ?? 0) - (f.db.get(k) ?? 0));
      return (f.dr.get(k) ?? 0) < exp;
    });
    findings.push({
      kind: "merge-drop",
      sha: c.sha,
      path: f.s.path,
      subject: c.subject,
      base,
      lostOurs: f.tw.lost.ours,
      lostTheirs: f.tw.lost.theirs,
      resurrected: f.tw.resurrected,
      tookSide: f.s.rBlob === f.s.xBlob ? "ours" : f.s.rBlob === f.s.yBlob ? "theirs" : "",
      sourceOurs: `${p1}:${f.s.path}`,
      sourceTheirs: `${p2}:${f.s.path}`,
      symbols: symbols.map(own),
      samples: f.tw.samples,
      lostLines: f.tw.lostLines,
    });
  }
}

// ---- non-merge analysis ----------------------------------------------------

function ancestorEvents(events, path, ci, isAnc) {
  const list = events.get(path) ?? [];
  const out = [];
  for (const e of list) {
    if (e.ci >= ci) break;
    if (isAnc(e.ci, ci)) out.push(e);
  }
  return out;
}

function analyseCommit(c, ctx, opts, findings) {
  const { events, isAnc, commits } = ctx;
  const big = c.parents.length === 1 && c.lines > opts.minLines;
  const plans = [];
  for (const ch of c.changes) {
    const cls = pathClass(ch.path);
    if (!cls || ch.newBlob === ZERO) continue;
    const prior = ancestorEvents(events, ch.path, c.i, isAnc);
    if (!prior.length) continue;
    let p0 = ch.oldBlob === ZERO ? null : ch.oldBlob;
    let p0Intro = null;
    for (let k = prior.length - 1; k >= 0; k--) {
      if (p0 === null && prior[k].blob) p0 = prior[k].blob;
      if (p0 !== null && prior[k].blob === p0) {
        p0Intro = prior[k].ci;
        break;
      }
    }
    if (p0 === null || p0Intro === null || p0 === ch.newBlob) continue;
    const older = [];
    const seen = new Set([p0]);
    for (let k = prior.length - 1; k >= 0 && older.length < OLDER_VERSIONS; k--) {
      const e = prior[k];
      if (!e.blob || seen.has(e.blob) || !isAnc(e.ci, p0Intro)) continue;
      seen.add(e.blob);
      older.push(e);
    }
    const exact = older.find((e) => e.blob === ch.newBlob);
    if (exact) {
      findings.push({
        kind: "resurrection",
        sha: c.sha,
        path: ch.path,
        subject: c.subject,
        restoredFrom: commits[exact.ci].sha,
        replaced: `${commits[p0Intro].sha}:${ch.path}`,
        readded: ch.oldBlob === ZERO,
        p0,
        newBlob: ch.newBlob,
        olderBlob: exact.blob,
      });
      continue;
    }
    if (big && older.length) plans.push({ ch, p0, p0Intro, older, cls });
  }
  if (!big) return;
  // Declarations over the whole commit: moves inside the commit cancel out.
  const codeChanges = c.changes.filter((ch) => langOf(ch.path) && pathClass(ch.path));
  const symBefore = new Map();
  const symAfter = new Map();
  const testHome = new Map();
  for (const chunk of chunks(codeChanges)) {
    readBlobs(chunk.flatMap((ch) => [ch.oldBlob, ch.newBlob]));
    for (const ch of chunk) {
      const before = declarations(ch.path, ch.oldBlob);
      addInto(symBefore, before, 1);
      addInto(symAfter, declarations(ch.path, ch.newBlob), 1);
      for (const k of before.keys()) if (k.startsWith("test:")) testHome.set(k, ch.path);
    }
    releaseBlobs();
  }
  const netDropped = new Set();
  for (const [k, n] of symBefore) if ((symAfter.get(k) ?? 0) < n) netDropped.add(k);
  const revertPaths = new Set();
  for (const chunk of chunks(plans)) {
    readBlobs(chunk.flatMap((p) => [p.p0, p.ch.newBlob, ...p.older.map((e) => e.blob)]));
    for (const p of chunk) {
      const f = revertFinding(c, p, commits, netDropped, opts);
      if (!f) continue;
      revertPaths.add(p.ch.path);
      findings.push(f);
    }
    releaseBlobs();
  }
  const byPath = new Map();
  for (const k of netDropped) {
    if (!k.startsWith("test:")) continue;
    const path = testHome.get(k) ?? "";
    if (revertPaths.has(path)) continue;
    if (!byPath.has(path)) byPath.set(path, []);
    byPath.get(path).push(k);
  }
  for (const [path, list] of byPath) {
    const ch = c.changes.find((x) => x.path === path);
    findings.push({
      kind: "tests-dropped",
      sha: c.sha,
      path,
      subject: c.subject,
      deletedFile: ch?.newBlob === ZERO,
      source: `${c.parents[0]}:${path}`,
      symbols: list.map(own),
    });
  }
}

function revertFinding(c, p, commits, netDropped, opts) {
  const n = lineMultiset(p.ch.newBlob);
  const prev = lineMultiset(p.p0);
  const dPrev = distance(n, prev);
  let best = null;
  for (const e of p.older) {
    const o = lineMultiset(e.blob);
    const d = distance(n, o);
    if (d < dPrev && (!best || d < best.d)) best = { e, o, d };
  }
  if (!best) return null;
  let dropped = 0;
  let restored = 0;
  const lostLines = [];
  for (const [k, pn] of prev) {
    const on = best.o.get(k) ?? 0;
    const nn = n.get(k) ?? 0;
    const gone = Math.min(Math.max(0, pn - on), Math.max(0, pn - nn));
    if (gone) {
      dropped += gone;
      if (lostLines.length < MAX_LOST_LINES) lostLines.push(own(k));
    }
  }
  for (const [k, on] of best.o) {
    const pn = prev.get(k) ?? 0;
    const nn = n.get(k) ?? 0;
    restored += Math.min(Math.max(0, on - pn), Math.max(0, nn - pn));
  }
  if (dropped < opts.revertMin) return null;
  const dp = declarations(p.ch.path, p.p0);
  const dn = declarations(p.ch.path, p.ch.newBlob);
  const symbols = [...dp.keys()].filter((k) => (dn.get(k) ?? 0) < dp.get(k) && netDropped.has(k));
  return {
    kind: "revert-like",
    sha: c.sha,
    path: p.ch.path,
    subject: c.subject,
    readded: p.ch.oldBlob === ZERO,
    closerTo: `${commits[best.e.ci].sha}:${p.ch.path}`,
    replaced: `${commits[p.p0Intro].sha}:${p.ch.path}`,
    distOlder: best.d,
    distPrev: dPrev,
    dropped,
    restored,
    symbols: symbols.map(own),
    samples: { dropped: lostLines.slice(0, 4) },
    lostLines,
  };
}

// ---- HEAD presence -------------------------------------------------------

function headState(rev, findings) {
  const tree = git(["ls-tree", "-r", "--full-tree", rev]);
  const byKey = new Map();
  for (const line of tree.split("\n")) {
    const tab = line.indexOf("\t");
    if (tab < 0) continue;
    const meta = line.slice(0, tab).split(" ");
    if (meta[1] !== "blob") continue;
    byKey.set(headKey(line.slice(tab + 1)), { path: line.slice(tab + 1), blob: meta[2] });
  }
  const anywhere = new Set();
  const all = spawnSync(
    "git",
    ["grep", "-h", "-I", "-e", "[A-Za-z0-9]", rev, "--", ...TEXT_GLOBS, ":!Tools/tower/.tower"],
    { cwd: REPO_ROOT, encoding: "utf8", maxBuffer: 4 * 1024 * 1024 * 1024 },
  ).stdout;
  for (const line of all.split("\n")) anywhere.add(line.trim());
  const withLines = findings.filter((f) => f.lostLines?.length);
  for (const f of findings) {
    const hit = f.path ? byKey.get(headKey(f.path)) : null;
    f.headPath = hit?.path ?? null;
    f._headBlob = hit?.blob ?? null;
  }
  for (const chunk of chunks(withLines)) {
    readBlobs(chunk.map((f) => f._headBlob));
    for (const f of chunk) {
      const ms = f._headBlob ? lineMultiset(f._headBlob) : new Map();
      f.headHasLines = `${f.lostLines.filter((k) => ms.has(k)).length}/${f.lostLines.length}`;
      f.headAnywhere = `${f.lostLines.filter((k) => anywhere.has(k)).length}/${f.lostLines.length}`;
    }
    releaseBlobs();
  }
  for (const f of findings) delete f._headBlob;
  const symbols = [...new Set(findings.flatMap((f) => f.symbols ?? []))];
  const found = new Set();
  const patterns = symbols.map((k) => [k, symbolPattern(k)]);
  for (let i = 0; i < patterns.length; i += 400) {
    const chunk = patterns.slice(i, i + 400);
    const args = ["grep", "-h", "-o", "-E", "-I"];
    for (const [, p] of chunk) args.push("-e", p);
    args.push(rev, "--", "*.jet", "*.rs", "*.mjs", "*.cjs", "*.js", "*.ts");
    const res = spawnSync("git", args, { cwd: REPO_ROOT, encoding: "utf8", maxBuffer: 1024 * 1024 * 1024 });
    const text = res.stdout ?? "";
    for (const [k, p] of chunk) {
      if (new RegExp(p, "m").test(text)) found.add(k);
    }
  }
  for (const f of findings) {
    if (!f.symbols) continue;
    f.symbolsMissingAtHead = f.symbols.filter((k) => !found.has(k));
  }
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function symbolPattern(key) {
  const colon = key.indexOf(":");
  const kind = key.slice(0, colon);
  const name = key.slice(colon + 1);
  if (kind === "test") return `(#Test\\(|test\\(|it\\(|fn )["'\`]?${escapeRe(name)}`;
  if (kind === "type") return `(struct|enum|trait|effect|protocol|class|type|choice|union) ${escapeRe(name)}\\b`;
  return `(fn|function\\*?) ${escapeRe(name)}\\b`;
}

// ---- driver ----------------------------------------------------------------

// Dispositions are per commit and signal: one review covers every file the
// commit's finding names.
function findingKey(f) {
  return `${f.kind}:${f.sha.slice(0, 12)}`;
}

// A commit whose message names the file (its stem) or says it reverts or
// restores is marked `explained`; the audit still lists it for review.
function markExplained(findings) {
  const shas = [...new Set(findings.map((f) => f.sha))];
  const bodies = new Map();
  for (let i = 0; i < shas.length; i += 500) {
    const text = git(["show", "-s", "--format=%x01%H%n%B", ...shas.slice(i, i + 500)]);
    for (const part of text.split("\x01")) {
      if (!part) continue;
      const nl = part.indexOf("\n");
      bodies.set(part.slice(0, nl), part.slice(nl + 1));
    }
  }
  for (const f of findings) {
    const body = bodies.get(f.sha) ?? "";
    const stem = (f.path.split("/").pop() ?? "").replace(/\.[^.]+$/, "");
    const named = stem.length > 2 && body.toLowerCase().includes(stem.toLowerCase());
    f.explained = named || /\b(revert|reverts|reverted|restore|restores|restored|roll(?:s|ed)? back)\b/i.test(body);
  }
}

function analyseScope(opts) {
  const { commits, index } = loadHistory(opts.rev, opts.history);
  const isAnc = ancestorSets(commits);
  const events = pathEvents(commits);
  let scope = commits;
  if (opts.since) {
    const since = index.get(git(["rev-parse", opts.since]).trim());
    if (since === undefined) throw new Error(`--since ${opts.since} is not in ${opts.rev}'s history`);
    scope = commits.filter((c) => c.i !== since && !isAnc(c.i, since));
  }
  const total = scope.length;
  if (opts.shard) scope = scope.filter((_, n) => n % opts.shard.of === opts.shard.index);
  const findings = [];
  const ctx = { events, isAnc, commits };
  let merges = 0;
  let big = 0;
  const label = opts.shard ? ` shard ${opts.shard.index}/${opts.shard.of}` : "";
  for (const [n, c] of scope.entries()) {
    if (n % 100 === 0) {
      const rss = Math.round(process.memoryUsage().rss / 1048576);
      process.stderr.write(`lost-work-audit${label}: ${n}/${scope.length} commits, ${findings.length} findings, rss ${rss} MB\n`);
    }
    if (c.parents.length > 1) {
      merges++;
      analyseMerge(c, commits, opts, findings);
    } else {
      if (c.lines > opts.minLines) big++;
      analyseCommit(c, ctx, opts, findings);
    }
  }
  return { commits: total, merges, bigCommits: big, findings };
}

// --jobs N runs N shard processes over one cached history scan and merges their
// parts; a shard that fails fails the run (its stderr is passed through).
async function runShards(opts) {
  const dir = opts.out ?? join(tmpdir(), `lost-work-audit-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  const history = opts.history ?? join(dir, "history.txt");
  loadHistory(opts.rev, history);
  const passthrough = ["--rev", opts.rev, "--history", history, "--min-lines", String(opts.minLines)];
  passthrough.push("--merge-min", String(opts.mergeMin), "--revert-min", String(opts.revertMin));
  if (opts.since) passthrough.push("--since", opts.since);
  const parts = [];
  const runs = [];
  for (let k = 0; k < opts.jobs; k++) {
    const part = join(dir, `part-${k}.json`);
    parts.push(part);
    const args = [`--max-old-space-size=${opts.shardHeapMb}`, fileURLToPath(import.meta.url), ...passthrough, "--shard", `${k}/${opts.jobs}`, "--part", part];
    runs.push(
      new Promise((done) => {
        const child = spawn(process.execPath, args, { cwd: REPO_ROOT, stdio: ["ignore", "ignore", "inherit"] });
        child.on("exit", (code, signal) => done({ k, code, signal }));
      }),
    );
  }
  const results = await Promise.all(runs);
  const failed = results.filter((r) => r.code !== 0);
  if (failed.length) {
    throw new Error(`shard(s) failed: ${failed.map((r) => `${r.k} (exit ${r.code ?? r.signal})`).join(", ")}`);
  }
  const merged = { commits: 0, merges: 0, bigCommits: 0, findings: [] };
  for (const part of parts) {
    const p = JSON.parse(readFileSync(part, "utf8"));
    merged.commits = p.commits;
    merged.merges += p.merges;
    merged.bigCommits += p.bigCommits;
    for (const f of p.findings) merged.findings.push(f);
  }
  return merged;
}

async function run(opts) {
  const scanned = opts.jobs > 1 ? await runShards(opts) : analyseScope(opts);
  const { findings } = scanned;
  headState(opts.rev, findings);
  markExplained(findings);
  for (const f of findings) {
    f.key = findingKey(f);
    delete f.lostLines;
  }
  return {
    rev: git(["rev-parse", opts.rev]).trim(),
    since: opts.since,
    commits: scanned.commits,
    merges: scanned.merges,
    bigCommits: scanned.bigCommits,
    minLines: opts.minLines,
    findings,
  };
}

function tsvRow(f) {
  const cols = [
    f.kind,
    f.sha.slice(0, 12),
    f.path,
    f.kind === "merge-drop"
      ? `ours=${f.lostOurs} theirs=${f.lostTheirs} back=${f.resurrected} took=${f.tookSide}`
      : f.kind === "revert-like"
        ? `dropped=${f.dropped} restored=${f.restored} dOld=${f.distOlder} dPrev=${f.distPrev}`
        : f.kind === "resurrection"
          ? `from=${f.restoredFrom.slice(0, 12)} readded=${f.readded}`
          : `${f.symbols?.length ?? 0} tests deleted=${f.deletedFile ?? ""}`,
    f.headHasLines ?? "",
    f.headAnywhere ?? "",
    (f.symbolsMissingAtHead ?? []).length + "/" + (f.symbols ?? []).length,
    f.explained ? "explained" : "",
    f.subject.slice(0, 100),
  ];
  return cols.join("\t");
}

// --check: the baseline's pinned revision, dispositions and evidence, from git objects only.
function checkBaseline(path) {
  if (!existsSync(path)) throw new Error(`no baseline at ${path}`);
  const base = JSON.parse(readFileSync(path, "utf8"));
  const problems = [];
  const pinned = (spec) => FULL_SHA.test(spec.slice(0, 40)) && spec[40] === ":";
  if (!FULL_SHA.test(base.rev ?? "")) problems.push(`rev ${base.rev ?? "(none)"} is not a full commit SHA`);
  if (!base.doc || !existsSync(join(REPO_ROOT, base.doc))) problems.push(`doc ${base.doc ?? "(none)"} is not in the tree`);
  const objects = [];
  let assertions = 0;
  if (FULL_SHA.test(base.rev ?? "")) objects.push({ key: "rev", spec: `${base.rev}^{commit}`, want: "commit" });
  const findings = Object.entries(base.findings ?? {});
  for (const [key, f] of findings) {
    if (!DISPOSITIONS.includes(f.disposition)) {
      problems.push(`${key}: disposition ${f.disposition ?? "(none)"} is not one of ${DISPOSITIONS.join(", ")}`);
    }
    for (const e of f.evidence ?? []) {
      assertions++;
      if (e.object !== undefined) {
        if (!pinned(e.object) || (e.blob !== null && !FULL_SHA.test(e.blob ?? ""))) {
          problems.push(`${key}: object assertion needs <full sha>:<path> and a full blob SHA or null: ${JSON.stringify(e)}`);
        } else objects.push({ key, spec: e.object, want: e.blob });
      } else if (e.revList !== undefined) {
        const args = e.revList;
        const ok = Array.isArray(args) && args.length > 0 && args.every((a) => a.startsWith("--") || /^[0-9a-f]{40}(\.\.[0-9a-f]{40})?$/.test(a));
        if (!ok || !Number.isInteger(e.count)) {
          problems.push(`${key}: rev-list assertion needs options and full SHAs only, and a count: ${JSON.stringify(e)}`);
          continue;
        }
        let got;
        try {
          got = Number(git(["rev-list", "--count", ...args]).trim());
        } catch {
          got = "an error";
        }
        if (got !== e.count) problems.push(`${key}: git rev-list --count ${args.join(" ")} is ${got}, baseline says ${e.count}`);
      } else if (e.tree !== undefined) {
        if (!pinned(e.tree) || !Number.isInteger(e.files)) {
          problems.push(`${key}: tree assertion needs <full sha>:<dir> and a file count: ${JSON.stringify(e)}`);
          continue;
        }
        let got;
        try {
          got = git(["ls-tree", "-r", "-z", "--name-only", e.tree]).split("\0").filter(Boolean).length;
        } catch {
          got = "missing";
        }
        if (got !== e.files) problems.push(`${key}: ${e.tree} holds ${got} files, baseline says ${e.files}`);
      } else problems.push(`${key}: unknown evidence assertion ${JSON.stringify(e)}`);
    }
  }
  // An absence assertion only means something when its commit exists: every pinned commit is checked too.
  const commits = new Set([...objects.map((o) => o.spec.slice(0, 40))]);
  for (const [, f] of findings) for (const e of f.evidence ?? []) if (e.tree && pinned(e.tree)) commits.add(e.tree.slice(0, 40));
  commits.delete(base.rev);
  for (const sha of [...commits].sort()) objects.push({ key: "commit", spec: `${sha}^{commit}`, want: "commit" });
  if (objects.length) {
    const lines = git(["cat-file", "--batch-check=%(objectname) %(objecttype)"], objects.map((o) => o.spec).join("\n") + "\n").split("\n");
    objects.forEach((o, n) => {
      const cut = lines[n].lastIndexOf(" ");
      const name = lines[n].slice(0, cut);
      const type = lines[n].slice(cut + 1);
      const got = type === "missing" || type === "ambiguous" ? null : o.want === "commit" ? type : name;
      if (got !== o.want) problems.push(`${o.key}: ${o.spec} is ${got ?? "absent"}, baseline says ${o.want ?? "absent"}`);
    });
  }
  return { rev: base.rev ?? "", findings: findings.length, assertions, problems };
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.shard) {
    writeFileSync(opts.part, JSON.stringify(analyseScope(opts)));
    return;
  }
  const where = opts.baseline.startsWith(REPO_ROOT + "/") ? relative(REPO_ROOT, opts.baseline) : opts.baseline;
  if (opts.check) {
    const { rev, findings, assertions, problems } = checkBaseline(opts.baseline);
    for (const p of problems) console.log(`FAIL\t${p}`);
    if (problems.length) {
      console.log(`lost-work-audit --check: FAIL ${problems.length} problem(s) in ${where}`);
      process.exit(1);
    }
    console.log(`lost-work-audit --check: OK (${rev.slice(0, 12)}; ${findings} findings, ${assertions} evidence assertions in ${where})`);
    return;
  }
  const report = await run(opts);
  const counts = {};
  for (const f of report.findings) counts[f.kind] = (counts[f.kind] ?? 0) + 1;
  if (opts.out) {
    mkdirSync(opts.out, { recursive: true });
    writeFileSync(join(opts.out, "findings.json"), JSON.stringify(report, null, 1) + "\n");
    writeFileSync(
      join(opts.out, "findings.tsv"),
      ["kind\tcommit\tpath\tmeasure\thead_same_path_has_lost_lines\thead_anywhere_has_lost_lines\tsymbols_missing_at_head\tmessage\tsubject", ...report.findings.map(tsvRow)].join("\n") + "\n",
    );
  }
  console.log(
    `lost-work-audit: ${report.commits} commits (${report.merges} merges, ${report.bigCommits} commits > ${report.minLines} lines) at ${report.rev.slice(0, 12)}`,
  );
  console.log(`findings: ${Object.entries(counts).map(([k, n]) => `${k}=${n}`).join(" ") || "none"}`);
  const byKey = new Map();
  for (const f of report.findings) {
    if (!byKey.has(f.key)) byKey.set(f.key, []);
    byKey.get(f.key).push(f);
  }
  const old = existsSync(opts.baseline) ? JSON.parse(readFileSync(opts.baseline, "utf8")) : { findings: {} };
  if (opts.writeBaseline) {
    const next = { ...old.findings };
    for (const [key, list] of byKey) {
      next[key] = old.findings?.[key] ?? { disposition: "unreviewed", files: list.length, subject: list[0].subject, note: "" };
    }
    writeFileSync(opts.baseline, JSON.stringify({ ...old, rev: report.rev, findings: next }, null, 1) + "\n");
    console.log(`baseline: ${Object.keys(next).length} findings in ${where}`);
  }
  const open = [...byKey].filter(([key]) => !DISPOSITIONS.includes(old.findings?.[key]?.disposition));
  for (const [key, list] of open) {
    console.log(`UNREVIEWED\t${key}\t${list.length} file(s)\t${list[0].subject.slice(0, 100)}`);
    for (const f of list.slice(0, 5)) console.log(`\t${tsvRow(f)}`);
  }
  if (open.length) {
    console.log(`lost-work-audit --sweep: FAIL ${open.length} commit finding(s) without a disposition in ${where}`);
    process.exit(1);
  }
  console.log("lost-work-audit --sweep: OK");
}

main().catch((err) => {
  console.error(`lost-work-audit: ${err.message}`);
  process.exit(2);
});
