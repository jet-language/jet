#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const BEGIN = "<!-- unsafe-ratchet:begin -->";
const END = "<!-- unsafe-ratchet:end -->";
const DATA_BEGIN = "<!-- unsafe-ratchet:data";
const DATA_END = "-->";
const SKIP_DIRECTORIES = new Set([
  ".agent-worktrees",
  ".cache",
  ".claude",
  ".git",
  ".tmp",
  "build",
  "node_modules",
  "result",
  "target",
]);

function usage() {
  return [
    "usage: check-unsafe-ratchet.mjs [--update] [--root PATH] [--baseline PATH]",
    "",
    "Scan user-written Jet unsafe regions and compare them with the committed baseline.",
    "A lower count updates the baseline. Use --update to approve a higher count.",
  ].join("\n");
}

function parseArgs(argv) {
  const options = { root: REPO_ROOT, baseline: join(REPO_ROOT, "docs/spec/safety.md"), update: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--update") {
      options.update = true;
    } else if (arg === "--root" || arg === "--baseline") {
      const value = argv[++index];
      if (!value) throw new Error(`${arg} needs a path`);
      options[arg.slice(2)] = resolve(value);
    } else if (arg === "--help") {
      console.log(usage());
      process.exit(0);
    } else {
      throw new Error(`unknown option ${arg}\n\n${usage()}`);
    }
  }
  return options;
}

function toPosix(path) {
  return path.split("\\").join("/");
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function isGeneratedFfi(relativePath) {
  const parts = relativePath.split("/");
  return parts.some((part, index) => part === ".jet" && parts[index + 1] === "bindings");
}

function jetFiles(root) {
  const files = [];
  function visit(directory) {
    const entries = readdirSync(directory, { withFileTypes: true }).sort((left, right) => compareText(left.name, right.name));
    for (const entry of entries) {
      if (entry.isSymbolicLink()) continue;
      const path = join(directory, entry.name);
      const relativePath = toPosix(relative(root, path));
      if (entry.isDirectory()) {
        if (SKIP_DIRECTORIES.has(entry.name) || isGeneratedFfi(`${relativePath}/placeholder`)) continue;
        visit(path);
      } else if (entry.isFile() && entry.name.endsWith(".jet") && !isGeneratedFfi(relativePath)) {
        files.push(path);
      }
    }
  }
  visit(root);
  return files;
}

function manifestName(path, pattern) {
  if (!existsSync(path)) return null;
  const match = readFileSync(path, "utf8").match(pattern);
  return match?.[1] ?? null;
}

function packageName(root, file) {
  const relativePath = toPosix(relative(root, file));
  const parts = relativePath.split("/");
  if (parts[0] === "crates" && parts[1]) {
    return manifestName(join(root, "crates", parts[1], "Cargo.toml"), /^name\s*=\s*"([^"]+)"/m) ?? `crate-${parts[1]}`;
  }
  if (parts[0] === "corelib") {
    let directory = dirname(file);
    while (directory.startsWith(root) && directory !== dirname(root)) {
      for (const manifest of ["package.jet", "pkg.jet"]) {
        const name = manifestName(join(directory, manifest), /^name\s*:\s*"([^"]+)"/m);
        if (name) return name;
      }
      directory = dirname(directory);
    }
    return "corelib";
  }
  if (["docs", "examples", "tests"].includes(parts[0])) return parts[0];
  return manifestName(join(root, "package.jet"), /^name\s*:\s*"([^"]+)"/m)
    ?? manifestName(join(root, "pkg.jet"), /^name\s*:\s*"([^"]+)"/m)
    ?? manifestName(join(root, "Cargo.toml"), /^name\s*=\s*"([^"]+)"/m)
    ?? parts[0]
    ?? "root";
}

function lineStarts(text) {
  const starts = [0];
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "\n") starts.push(index + 1);
  }
  return starts;
}

function location(starts, index) {
  let low = 0;
  let high = starts.length;
  while (low + 1 < high) {
    const middle = Math.floor((low + high) / 2);
    if (starts[middle] <= index) low = middle;
    else high = middle;
  }
  return { line: low + 1, column: index - starts[low] + 1 };
}

function skipQuoted(text, start, delimiter) {
  if (delimiter === "\"\"\"") {
    const end = text.indexOf(delimiter, start + delimiter.length);
    return end < 0 ? text.length : end + delimiter.length;
  }
  for (let index = start + delimiter.length; index < text.length; index += 1) {
    if (text[index] === "\\") {
      index += 1;
    } else if (text.startsWith(delimiter, index)) {
      return index + delimiter.length;
    }
  }
  return text.length;
}

function skipComment(text, start) {
  if (text.startsWith("//", start)) {
    const end = text.indexOf("\n", start + 2);
    return end < 0 ? text.length : end + 1;
  }
  let depth = 1;
  for (let index = start + 2; index < text.length; index += 1) {
    if (text.startsWith("/*", index)) {
      depth += 1;
      index += 1;
    } else if (text.startsWith("*/", index)) {
      depth -= 1;
      index += 1;
      if (depth === 0) return index + 1;
    }
  }
  return text.length;
}

function isIdentifierChar(char) {
  return Boolean(char) && /[A-Za-z0-9_]/.test(char);
}

function skipSpace(text, start) {
  let index = start;
  while (/\s/.test(text[index] ?? "")) index += 1;
  return index;
}

function decodeJetString(text) {
  let value = "";
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (char === "\\") {
      const escaped = text[++index];
      if (escaped === "n") value += "\n";
      else if (escaped === "t") value += "\t";
      else if (escaped === '"') value += '"';
      else if (escaped === "\\") value += "\\";
      else return null;
    } else if (char === "{" && text[index + 1] === "{") {
      value += "{";
      index += 1;
    } else if (char === "}" && text[index + 1] === "}") {
      value += "}";
      index += 1;
    } else if (char === "{" || char === "}") {
      return null;
    } else {
      value += char;
    }
  }
  return value;
}

function quotedStringAt(text, start) {
  if (text[start] !== '"' || text.startsWith('"""', start)) return null;
  const end = skipQuoted(text, start, '"');
  if (end <= start || text[end - 1] !== '"') return null;
  const reason = decodeJetString(text.slice(start + 1, end - 1));
  return reason === null ? null : { end, reason };
}

function unsafeAt(text, start) {
  if (text[start] !== "#") return null;
  let index = skipSpace(text, start + 1);
  if (!text.startsWith("Unsafe", index) || isIdentifierChar(text[index + "Unsafe".length])) return null;
  index = skipSpace(text, index + "Unsafe".length);
  if (text[index] !== "(") return null;
  const literal = quotedStringAt(text, skipSpace(text, index + 1));
  return literal ? { end: literal.end, reason: literal.reason } : null;
}

function skipBalanced(text, start, open, close) {
  let depth = 0;
  for (let index = start; index < text.length; index += 1) {
    if (text.startsWith("//", index) || text.startsWith("/*", index)) {
      index = skipComment(text, index) - 1;
      continue;
    }
    if (text.startsWith("\"\"\"", index)) {
      index = skipQuoted(text, index, "\"\"\"") - 1;
      continue;
    }
    if (text[index] === '"' || text[index] === "`" || text[index] === "'") {
      index = skipQuoted(text, index, text[index]) - 1;
      continue;
    }
    if (text[index] === open) depth += 1;
    else if (text[index] === close) {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return text.length;
}

function markerGroupAt(text, start) {
  if (!text.startsWith("#[", start)) return null;
  const markers = [];
  let index = start + 2;
  while (index < text.length) {
    index = skipSpace(text, index);
    if (text[index] === "]") return { end: index + 1, markers };
    let negated = false;
    if (text[index] === "!") {
      negated = true;
      index = skipSpace(text, index + 1);
    }
    if (!/[A-Za-z_]/.test(text[index] ?? "")) return null;
    const nameStart = index;
    index += 1;
    while (isIdentifierChar(text[index])) index += 1;
    const name = text.slice(nameStart, index);
    index = skipSpace(text, index);
    let reason = null;
    if (text[index] === "(") {
      if (name === "Unsafe") {
        const literal = quotedStringAt(text, skipSpace(text, index + 1));
        reason = literal?.reason ?? null;
      }
      index = skipBalanced(text, index, "(", ")");
    }
    markers.push({ name, negated, reason });
    index = skipSpace(text, index);
    if (text[index] === ",") {
      index += 1;
      continue;
    }
    if (text[index] === "]") return { end: index + 1, markers };
    return null;
  }
  return null;
}

function scanSource(text, relativePath, owner) {
  const starts = lineStarts(text);
  const regions = [];
  for (let index = 0; index < text.length;) {
    if (text.startsWith("//", index) || text.startsWith("/*", index)) {
      index = skipComment(text, index);
      continue;
    }
    if (text.startsWith("\"\"\"", index)) {
      index = skipQuoted(text, index, "\"\"\"");
      continue;
    }
    if (text[index] === '"') {
      index = skipQuoted(text, index, '"');
      continue;
    }
    if (text[index] === "`") {
      index = skipQuoted(text, index, "`");
      continue;
    }
    if (text[index] === "'") {
      index = skipQuoted(text, index, "'");
      continue;
    }
    if (text.startsWith("#[", index)) {
      const group = markerGroupAt(text, index);
      if (group) {
        for (const marker of group.markers) {
          if (marker.name !== "Unsafe" || marker.negated || marker.reason === null) continue;
          const position = location(starts, index);
          regions.push({ package: owner, file: relativePath, line: position.line, column: position.column, reason: marker.reason });
        }
        index = group.end;
        continue;
      }
    }
    if (text[index] === "#") {
      const found = unsafeAt(text, index);
      if (found) {
        const position = location(starts, index);
        regions.push({ package: owner, file: relativePath, line: position.line, column: position.column, reason: found.reason });
        index = found.end;
        continue;
      }
    }
    index += 1;
  }
  return regions;
}

function scan(root) {
  const regions = jetFiles(root).flatMap((file) => {
    const relativePath = toPosix(relative(root, file));
    return scanSource(readFileSync(file, "utf8"), relativePath, packageName(root, file));
  });
  regions.sort((left, right) => compareText(left.package, right.package) || compareText(left.file, right.file) || left.line - right.line || left.column - right.column || compareText(left.reason, right.reason));
  const counts = {};
  for (const region of regions) counts[region.package] = (counts[region.package] ?? 0) + 1;
  return {
    schema: 1,
    total: regions.length,
    counts: Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right))),
    regions,
  };
}

function validateData(data) {
  if (!data || data.schema !== 1 || !Number.isInteger(data.total) || !data.counts || !Array.isArray(data.regions)) {
    throw new Error("unsafe baseline has invalid data");
  }
  if (data.total !== data.regions.length) throw new Error("unsafe baseline total does not match its region rows");
  const counts = {};
  for (const region of data.regions) {
    if (!region || typeof region.package !== "string" || typeof region.file !== "string" || !Number.isInteger(region.line) || !Number.isInteger(region.column) || typeof region.reason !== "string") {
      throw new Error("unsafe baseline has an invalid region row");
    }
    counts[region.package] = (counts[region.package] ?? 0) + 1;
  }
  const normalizedCounts = Object.fromEntries(Object.entries(counts).sort(([left], [right]) => compareText(left, right)));
  if (JSON.stringify(normalizedCounts) !== JSON.stringify(data.counts)) throw new Error("unsafe baseline counts do not match its region rows");
}

function markdownCell(value) {
  return value.replaceAll("&", "&amp;").replaceAll("|", "&#124;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

function renderSection(data) {
  const counts = Object.entries(data.counts);
  const countRows = counts.length === 0 ? "| (none) | 0 |" : counts.map(([name, count]) => `| ${markdownCell(name)} | ${count} |`).join("\n");
  const regionRows = data.regions.length === 0
    ? "| (none) | (none) | - | (none) |"
    : data.regions.map((region) => `| ${markdownCell(region.package)} | ${markdownCell(region.file)} | ${region.line}:${region.column} | ${markdownCell(JSON.stringify(region.reason))} |`).join("\n");
  return [
    `${DATA_BEGIN}\n${JSON.stringify(data, null, 2)}\n${DATA_END}`,
    "",
    "### Counts",
    "",
    "| crate/package | regions |",
    "| --- | ---: |",
    countRows,
    `| **total** | **${data.total}** |`,
    "",
    "### Regions",
    "",
    "| crate/package | file | line | reason |",
    "| --- | --- | ---: | --- |",
    regionRows,
  ].join("\n");
}

function sectionBounds(text) {
  const begin = text.indexOf(BEGIN);
  const end = text.indexOf(END);
  if (begin < 0 || end < 0 || end < begin) return null;
  return { begin, end };
}

function baselineDocument(text, data) {
  const section = renderSection(data);
  const bounds = sectionBounds(text);
  if (!bounds) {
    return `${text.trimEnd()}\n\n${BEGIN}\n${section}\n${END}\n`;
  }
  return `${text.slice(0, bounds.begin)}${BEGIN}\n${section}\n${END}${text.slice(bounds.end + END.length)}`;
}

function readBaseline(path) {
  if (!existsSync(path)) throw new Error(`missing baseline ${path}`);
  const text = readFileSync(path, "utf8");
  const bounds = sectionBounds(text);
  if (!bounds) throw new Error(`baseline ${path} has no unsafe-ratchet section`);
  const body = text.slice(bounds.begin + BEGIN.length, bounds.end).replace(/^\n/, "").replace(/\n$/, "");
  const match = body.match(/^<!-- unsafe-ratchet:data\n([\s\S]*?)\n-->[\s\S]*$/);
  if (!match) throw new Error(`baseline ${path} has no unsafe-ratchet data`);
  let data;
  try {
    data = JSON.parse(match[1]);
  } catch (error) {
    throw new Error(`baseline ${path} has invalid JSON: ${error.message}`);
  }
  validateData(data);
  if (body !== renderSection(data)) throw new Error(`baseline ${path} has stale generated rows; run the ratchet with --update`);
  return { text, data };
}

function regionKey(region) {
  return JSON.stringify([region.package, region.file, region.reason]);
}

function growth(current, baseline) {
  const packages = [...new Set([...Object.keys(current.counts), ...Object.keys(baseline.counts)])].sort(compareText);
  const increases = packages
    .map((name) => ({ name, before: baseline.counts[name] ?? 0, after: current.counts[name] ?? 0 }))
    .filter((row) => row.after > row.before);
  if (increases.length === 0) return null;
  const remaining = new Map();
  for (const region of baseline.regions) remaining.set(regionKey(region), (remaining.get(regionKey(region)) ?? 0) + 1);
  const newRegions = [];
  for (const region of current.regions) {
    const key = regionKey(region);
    const count = remaining.get(key) ?? 0;
    if (count > 0) remaining.set(key, count - 1);
    else if ((current.counts[region.package] ?? 0) > (baseline.counts[region.package] ?? 0)) newRegions.push(region);
  }
  return { increases, newRegions };
}

function formatGrowth(details) {
  const lines = ["unsafe-region baseline grew:", ...details.increases.map((row) => `  ${row.name}: ${row.before} -> ${row.after}`), "new regions:"];
  if (details.newRegions.length === 0) lines.push("  (baseline rows changed without enough named rows to identify the additions)");
  else for (const region of details.newRegions) lines.push(`  - ${region.package} ${region.file}:${region.line}:${region.column} reason ${JSON.stringify(region.reason)}`);
  lines.push("update the committed baseline in the same change:", "  node scripts/agent/check-unsafe-ratchet.mjs --update");
  return lines.join("\n");
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const current = scan(options.root);
  let baseline;
  try {
    baseline = readBaseline(options.baseline);
  } catch (error) {
    if (!options.update) throw error;
    const text = existsSync(options.baseline) ? readFileSync(options.baseline, "utf8") : "# Safety\n";
    writeFileSync(options.baseline, baselineDocument(text, current));
    console.log(`unsafe ratchet: initialized baseline with ${current.total} regions`);
    return;
  }

  if (options.update) {
    writeFileSync(options.baseline, baselineDocument(baseline.text, current));
    console.log(`unsafe ratchet: baseline updated to ${current.total} regions`);
    return;
  }

  const details = growth(current, baseline.data);
  if (details) {
    console.error(formatGrowth(details));
    process.exitCode = 1;
    return;
  }

  if (current.total < baseline.data.total) {
    writeFileSync(options.baseline, baselineDocument(baseline.text, current));
    console.log(`unsafe ratchet: baseline decreased to ${current.total} regions`);
  } else {
    console.log(`unsafe ratchet: ${current.total} regions; baseline unchanged`);
  }
}

try {
  main();
} catch (error) {
  console.error(`unsafe ratchet: ${error.message}`);
  process.exitCode = 1;
}
