#!/usr/bin/env node
/**
 * Heuristic corpus scan: where inherent `impl Type` blocks live vs type defs.
 * Not a full Rust parser — good enough for trends across real crates.
 */

import fs from "node:fs";
import path from "node:path";

const SKIP_DIRS = new Set([
  "target",
  ".git",
  "tests",
  "benches",
  "examples",
  "testdata",
  "test_data",
  "ui",
]);

function walkRs(root) {
  const out = [];
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entries) {
      if (e.name.startsWith(".")) continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) {
        if (!SKIP_DIRS.has(e.name)) stack.push(p);
      } else if (e.isFile() && e.name.endsWith(".rs")) {
        out.push(p);
      }
    }
  }
  return out;
}

function stripNoise(src) {
  // Replace comments and string/char/raw-string contents with spaces (keep newlines).
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const n1 = src[i + 1];

    if (c === "/" && n1 === "/") {
      out += "  ";
      i += 2;
      while (i < n && src[i] !== "\n") {
        out += " ";
        i++;
      }
      continue;
    }
    if (c === "/" && n1 === "*") {
      out += "  ";
      i += 2;
      while (i < n && !(src[i] === "*" && src[i + 1] === "/")) {
        out += src[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < n) {
        out += "  ";
        i += 2;
      }
      continue;
    }

    // raw strings r#"..."# / r"..."
    if (c === "r" && (n1 === "#" || n1 === '"')) {
      let j = i + 1;
      let hash = 0;
      while (src[j] === "#") {
        hash++;
        j++;
      }
      if (src[j] === '"') {
        out += " ".repeat(j - i + 1);
        i = j + 1;
        const close = '"' + "#".repeat(hash);
        while (i < n && !src.startsWith(close, i)) {
          out += src[i] === "\n" ? "\n" : " ";
          i++;
        }
        if (i < n) {
          out += " ".repeat(close.length);
          i += close.length;
        }
        continue;
      }
    }

    if (c === '"' || c === "'") {
      const q = c;
      out += " ";
      i++;
      while (i < n) {
        if (src[i] === "\\") {
          out += "  ";
          i += 2;
          continue;
        }
        if (src[i] === q) {
          out += " ";
          i++;
          break;
        }
        out += src[i] === "\n" ? "\n" : " ";
        i++;
      }
      continue;
    }

    out += c;
    i++;
  }
  return out;
}

function lineStartsOf(src) {
  const s = [0];
  for (let i = 0; i < src.length; i++) if (src[i] === "\n") s.push(i + 1);
  return s;
}

function lineAt(off, starts) {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (starts[mid] <= off) lo = mid + 1;
    else hi = mid - 1;
  }
  return hi + 1;
}

function findMatchingBrace(src, openIdx) {
  let d = 1;
  for (let i = openIdx + 1; i < src.length; i++) {
    if (src[i] === "{") d++;
    else if (src[i] === "}") {
      d--;
      if (d === 0) return i;
    }
  }
  return -1;
}

function skipWs(src, i) {
  while (i < src.length && /\s/.test(src[i])) i++;
  return i;
}

function readIdent(src, i) {
  i = skipWs(src, i);
  if (i >= src.length || !/[A-Za-z_]/.test(src[i])) return [null, i];
  const a = i;
  i++;
  while (i < src.length && /[A-Za-z0-9_]/.test(src[i])) i++;
  return [src.slice(a, i), i];
}

function skipBalanced(src, i, open, close) {
  if (src[i] !== open) return i;
  let d = 1;
  i++;
  while (i < src.length && d > 0) {
    if (src[i] === open) d++;
    else if (src[i] === close) d--;
    i++;
  }
  return i;
}

function skipGenerics(src, i) {
  i = skipWs(src, i);
  return src[i] === "<" ? skipBalanced(src, i, "<", ">") : i;
}

function skipToItemEnd(src, i) {
  let angle = 0;
  let paren = 0;
  let bracket = 0;
  while (i < src.length) {
    const c = src[i];
    if (c === "<") angle++;
    else if (c === ">" && angle) angle--;
    else if (c === "(") paren++;
    else if (c === ")" && paren) paren--;
    else if (c === "[") bracket++;
    else if (c === "]" && bracket) bracket--;
    else if (!angle && !paren && !bracket && (c === "{" || c === ";")) return i;
    i++;
  }
  return i;
}

function readPath(src, i) {
  const parts = [];
  while (true) {
    const [id, ni] = readIdent(src, i);
    if (!id) return [null, i];
    parts.push(id);
    i = skipGenerics(src, ni);
    i = skipWs(src, i);
    if (src.startsWith("::", i)) {
      i += 2;
      continue;
    }
    break;
  }
  return [parts, i];
}

function skipVisibilityAndQualifiers(src, i) {
  while (true) {
    const [kw, ni] = readIdent(src, i);
    if (
      kw === "pub" ||
      kw === "unsafe" ||
      kw === "const" ||
      kw === "default" ||
      kw === "auto" ||
      kw === "async"
    ) {
      i = ni;
      i = skipWs(src, i);
      if (kw === "pub" && src[i] === "(") i = skipBalanced(src, i, "(", ")");
      continue;
    }
    break;
  }
  return i;
}

function scanFile(abs, rel) {
  const raw = fs.readFileSync(abs, "utf8");
  const src = stripNoise(raw);
  const starts = lineStartsOf(src);
  const types = [];
  const inherent = [];
  const traitImpls = [];

  // Scan at every brace depth — type/impl items can appear inside modules.
  // We do not try to recover nested type names inside functions.
  let i = 0;
  const n = src.length;
  let depth = 0;
  // Track if we are inside fn/trait body roughly via depth from their braces —
  // Accept false positives inside fns (rare for `struct Foo` + `impl Foo`).

  while (i < n) {
    if (/\s/.test(src[i])) {
      i++;
      continue;
    }

    // attributes
    if (src.startsWith("#[", i) || src.startsWith("#![", i)) {
      const open = src.indexOf("[", i);
      i = skipBalanced(src, open, "[", "]");
      continue;
    }

    const itemStart = i;
    i = skipVisibilityAndQualifiers(src, i);
    const [kw, afterKw] = readIdent(src, i);
    if (!kw) {
      if (src[i] === "{") {
        depth++;
        i++;
      } else if (src[i] === "}") {
        depth = Math.max(0, depth - 1);
        i++;
      } else i++;
      continue;
    }

    if (kw === "struct" || kw === "enum" || kw === "union") {
      const [name, afterName] = readIdent(src, afterKw);
      if (name && /^[A-Z]/.test(name)) {
        let k = skipGenerics(src, afterName);
        k = skipToItemEnd(src, k);
        const line = lineAt(itemStart, starts);
        let endLine = line;
        if (src[k] === "{") {
          const close = findMatchingBrace(src, k);
          if (close >= 0) {
            endLine = lineAt(close, starts);
            types.push({ name, kind: kw, line, endLine, file: rel });
            i = close + 1;
            continue;
          }
        } else if (src[k] === ";") {
          endLine = lineAt(k, starts);
          types.push({ name, kind: kw, line, endLine, file: rel });
          i = k + 1;
          continue;
        }
      }
      i = afterKw;
      continue;
    }

    if (kw === "impl") {
      let k = skipGenerics(src, afterKw);
      k = skipWs(src, k);
      let [maybeConst, k2] = readIdent(src, k);
      if (maybeConst === "const") k = k2;

      const [path1, after1] = readPath(src, k);
      if (path1) {
        let p = skipWs(src, after1);
        const [maybeFor] = readIdent(src, p);
        let target = null;
        let isTrait = false;
        if (maybeFor === "for") {
          isTrait = true;
          const [path2, after2] = readPath(src, p + 3);
          if (path2) {
            target = path2[path2.length - 1];
            p = after2;
          }
        } else {
          target = path1[path1.length - 1];
          p = after1;
        }
        if (target && /^[A-Z_]/.test(target)) {
          p = skipToItemEnd(src, p);
          if (src[p] === "{") {
            const close = findMatchingBrace(src, p);
            if (close >= 0) {
              const line = lineAt(itemStart, starts);
              const endLine = lineAt(close, starts);
              if (isTrait) {
                traitImpls.push({
                  name: target,
                  trait: path1.join("::"),
                  line,
                  file: rel,
                });
              } else {
                inherent.push({ name: target, line, endLine, file: rel });
              }
              i = close + 1;
              continue;
            }
          }
        }
      }
      i = afterKw;
      continue;
    }

    // Consume other items with blocks so braces stay aligned.
    if (
      kw === "fn" ||
      kw === "trait" ||
      kw === "mod" ||
      kw === "macro_rules" ||
      kw === "extern"
    ) {
      let k = skipToItemEnd(src, afterKw);
      if (kw === "mod" && src[skipWs(src, afterKw)] === ";") {
        i = skipWs(src, afterKw) + 1;
        continue;
      }
      if (src[k] === "{") {
        const close = findMatchingBrace(src, k);
        i = close >= 0 ? close + 1 : k + 1;
        continue;
      }
      if (src[k] === ";") {
        i = k + 1;
        continue;
      }
    }

    if (kw === "use" || kw === "type" || kw === "const" || kw === "static") {
      let k = skipToItemEnd(src, afterKw);
      if (src[k] === "{") {
        const close = findMatchingBrace(src, k);
        i = close >= 0 ? close + 1 : k + 1;
      } else i = src[k] === ";" ? k + 1 : afterKw;
      continue;
    }

    // fallback: advance one char from original keyword start
    i = itemStart + 1;
  }

  return { types, inherent, traitImpls, lines: starts.length };
}

function analyzeCrate(root, label) {
  const files = walkRs(root);
  const allTypes = [];
  const allInherent = [];
  let totalLines = 0;
  let traitImplBlocks = 0;

  for (const f of files) {
    const rel = path.relative(root, f);
    try {
      const r = scanFile(f, rel);
      allTypes.push(...r.types);
      allInherent.push(...r.inherent);
      traitImplBlocks += r.traitImpls.length;
      totalLines += r.lines;
    } catch {
      /* skip */
    }
  }

  const typesByName = new Map();
  const typeNameCounts = new Map();
  for (const t of allTypes) {
    typeNameCounts.set(t.name, (typeNameCounts.get(t.name) || 0) + 1);
    if (!typesByName.has(t.name)) typesByName.set(t.name, t);
  }

  const implsByName = new Map();
  for (const im of allInherent) {
    if (!implsByName.has(im.name)) implsByName.set(im.name, []);
    implsByName.get(im.name).push(im);
  }

  const ADJACENT_GAP = 8;

  let typesWithImpl = 0;
  let skippedHomonym = 0;
  let sameFileAll = 0;
  let sameFileAdjacent = 0;
  let sameFileNotAdjacent = 0;
  let multiFile = 0;
  let sameDirDiffFile = 0;
  let otherDir = 0;
  let multiBlockAny = 0;
  let multiBlockSameFile = 0;
  let singleBlockSameFile = 0;
  let noDefFound = 0;

  const examples = { multiFile: [], multiBlock: [], distantSameFile: [] };

  for (const [name, impls] of implsByName) {
    const def = typesByName.get(name);
    if (!def) {
      noDefFound++;
      continue;
    }
    // Same simple name in many modules (Sender, Context, …) is not real
    // cross-file impl splitting for one type. Skip those from placement stats.
    if ((typeNameCounts.get(name) || 0) > 1) {
      skippedHomonym++;
      continue;
    }
    typesWithImpl++;
    const fileSet = new Set(impls.map((x) => x.file));
    const allInDefFile = fileSet.size === 1 && fileSet.has(def.file);

    if (impls.length > 1) multiBlockAny++;

    if (allInDefFile) {
      sameFileAll++;
      if (impls.length > 1) {
        multiBlockSameFile++;
        if (examples.multiBlock.length < 6) {
          examples.multiBlock.push({
            name,
            file: def.file,
            blocks: impls.length,
          });
        }
      } else singleBlockSameFile++;

      const first = [...impls].sort((a, b) => a.line - b.line)[0];
      const gap = first.line - def.endLine;
      if (gap >= 0 && gap <= ADJACENT_GAP) sameFileAdjacent++;
      else {
        sameFileNotAdjacent++;
        if (examples.distantSameFile.length < 6) {
          examples.distantSameFile.push({
            name,
            file: def.file,
            defEnd: def.endLine,
            implLine: first.line,
            gap,
          });
        }
      }
    } else {
      multiFile++;
      const defDir = path.dirname(def.file);
      let anyOtherDir = false;
      for (const f of fileSet) {
        if (f === def.file) continue;
        if (path.dirname(f) !== defDir) anyOtherDir = true;
      }
      if (anyOtherDir) otherDir++;
      else sameDirDiffFile++;

      if (examples.multiFile.length < 8) {
        examples.multiFile.push({
          name,
          defFile: def.file,
          implFiles: [...fileSet],
          blocks: impls.length,
        });
      }
    }
  }

  const pct = (a, b) => (b === 0 ? 0 : Math.round((1000 * a) / b) / 10);

  return {
    label,
    files: files.length,
    totalLines,
    typeDefs: allTypes.length,
    inherentBlocks: allInherent.length,
    traitImplBlocks,
    typesWithInherentImpl: typesWithImpl,
    skippedHomonym,
    noDefFoundForImplName: noDefFound,
    metrics: {
      sameFileAll,
      sameFileAdjacent,
      sameFileNotAdjacent,
      multiFile,
      sameDirDiffFile,
      otherDir,
      multiBlockAny,
      multiBlockSameFile,
      singleBlockSameFile,
      pctSameFile: pct(sameFileAll, typesWithImpl),
      pctAdjacentAmongSameFile: pct(sameFileAdjacent, sameFileAll),
      pctMultiFile: pct(multiFile, typesWithImpl),
      pctMultiBlock: pct(multiBlockAny, typesWithImpl),
    },
    examples,
  };
}

const targets = process.argv.slice(2);
if (!targets.length) {
  console.error("Usage: analyze_struct_impl.mjs label=path ...");
  process.exit(1);
}

const results = [];
for (const t of targets) {
  const eq = t.indexOf("=");
  const label = t.slice(0, eq);
  const root = path.resolve(t.slice(eq + 1));
  if (!fs.existsSync(root)) {
    console.error("Missing", root);
    continue;
  }
  results.push(analyzeCrate(root, label));
}
console.log(JSON.stringify(results, null, 2));
