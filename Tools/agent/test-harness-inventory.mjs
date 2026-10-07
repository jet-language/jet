#!/usr/bin/env node
// U8: repeatable census of Cargo integration targets, not embedded Jet strings.
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

// Keep offsets/newlines so module attributes and function bodies can be read
// from the original text. Rust raw strings, nested comments and lifetimes are
// significant here: many tests embed whole Jet programs containing `jet::`.
export function rustCode(text) {
  let out = '', i = 0;
  const blank = (end) => { out += text.slice(i, end).replace(/[^\n]/gu, ' '); i = end; };
  while (i < text.length) {
    if (text.startsWith('//', i)) { const end = text.indexOf('\n', i); blank(end < 0 ? text.length : end); continue; }
    if (text.startsWith('/*', i)) {
      let depth = 1, end = i + 2;
      while (end < text.length && depth) {
        if (text.startsWith('/*', end)) { depth++; end += 2; }
        else if (text.startsWith('*/', end)) { depth--; end += 2; }
        else end++;
      }
      blank(end); continue;
    }
    const raw = text.slice(i).match(/^(?:b|c)?r(#+)?"/u);
    if (raw && (i === 0 || !/[\w]/u.test(text[i - 1]))) {
      const stop = `"${raw[1] ?? ''}`, end = text.indexOf(stop, i + raw[0].length);
      blank(end < 0 ? text.length : end + stop.length); continue;
    }
    if (text[i] === '"' || (text[i] === "'" && /^'(?:[^'\\\n]|\\(?:.|u\{[^}]*\}|x..))'/u.test(text.slice(i)))) {
      const quote = text[i]; let end = i + 1;
      while (end < text.length) { if (text[end] === '\\') end += 2; else if (text[end++] === quote) break; }
      blank(end); continue;
    }
    out += text[i++];
  }
  return out;
}

function functions(text) {
  const code = rustCode(text), result = new Map();
  let cursor = 0, nesting = 0;
  for (const match of code.matchAll(/\bfn\s+(\w+)\s*(?:<[^{};]*>)?\s*\(/gu)) {
    while (cursor < match.index) {
      if (code[cursor] === '{') nesting++;
      else if (code[cursor] === '}') nesting--;
      cursor++;
    }
    if (nesting !== 0) continue; // Impl methods named new/len are not common imports.
    const start = code.indexOf('{', match.index), semicolon = code.indexOf(';', match.index);
    if (start < 0 || (semicolon >= 0 && semicolon < start)) continue;
    let end = start + 1, depth = 1;
    while (end < code.length && depth) { if (code[end] === '{') depth++; if (code[end] === '}') depth--; end++; }
    result.set(match[1], text.slice(match.index, end));
  }
  return result;
}

function modules(file, seen = new Set()) {
  if (seen.has(file)) return [];
  seen.add(file);
  const text = readFileSync(file, 'utf8'), code = rustCode(text), parts = [text];
  for (const match of code.matchAll(/\bmod\s+(\w+)\s*;/gu)) {
    if (match[1] === 'common') continue; // Follow only the common helpers actually used below.
    const prefix = text.slice(Math.max(0, match.index - 160), match.index);
    const explicit = prefix.match(/#\[path\s*=\s*"([^"]+)"\](?:\s*#\[[^\]]*\])*\s*(?:pub(?:\([^)]*\))?\s*)?$/u)?.[1];
    const stem = file.endsWith('/mod.rs') ? dirname(file) : file.slice(0, -3);
    const candidates = explicit ? [resolve(dirname(file), explicit)] : [join(stem, `${match[1]}.rs`), join(stem, match[1], 'mod.rs'), join(dirname(file), `${match[1]}.rs`), join(dirname(file), match[1], 'mod.rs')];
    const child = candidates.find(existsSync);
    if (!child) throw new Error(`unresolved Rust test module ${match[1]} in ${file}`);
    parts.push(...modules(child, seen));
  }
  return parts;
}

function rustFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((item) => {
    const file = join(directory, item.name);
    return item.isDirectory() ? rustFiles(file) : item.isFile() && item.name.endsWith('.rs') ? [file] : [];
  }).sort();
}

export function inventory(root = ROOT) {
  const common = functions(readFileSync(join(root, 'tests/common/mod.rs'), 'utf8'));
  const crates = new Set(['jet', ...[...readFileSync(join(root, 'Cargo.toml'), 'utf8').matchAll(/^([a-z][a-z0-9-]*)\s*=\s*\{/gmu)].map((m) => m[1]), 'rustls-pemfile', 'wat']);
  const rows = [];
  const owners = new Map();
  const addOwner = (file, target) => {
    if (!owners.has(file)) owners.set(file, new Set());
    owners.get(file).add(target);
  };
  for (const name of readdirSync(join(root, 'tests')).filter((name) => name.endsWith('.rs'))) {
    const entry = join(root, 'tests', name), seen = new Set();
    const parts = modules(entry, seen);
    if (/\bmod\s+common\s*;/u.test(parts.map(rustCode).join('\n'))) modules(join(root, 'tests/common/mod.rs'), seen);
    for (const file of seen) addOwner(file, name);
    for (const file of seen) {
      const source = readFileSync(file, 'utf8'), code = rustCode(source);
      for (const match of source.matchAll(/\binclude_(?:str|bytes)!\(\s*"([^"]+)"\s*\)/gu)) {
        if (!code.slice(match.index).startsWith('include_')) continue;
        const fixture = resolve(dirname(file), match[1]);
        if (fixture.endsWith('.rs') && existsSync(fixture)) addOwner(fixture, name);
      }
    }
  }
  for (const file of rustFiles(join(root, 'tests'))) {
    const name = file.slice(join(root, 'tests').length + 1), source = readFileSync(file, 'utf8');
    const parts = modules(file), used = new Set();
    const usesCommon = /\bmod\s+common\s*;/u.test(parts.map(rustCode).join('\n'));
    // Imports and calls both bring helper implementations onto the Rust path.
    for (let cursor = 0; usesCommon && cursor < parts.length; cursor++) {
      const code = rustCode(parts[cursor]);
      for (const [helper, body] of common) {
        if (!used.has(helper) && new RegExp(`\\b${helper}\\b`, 'u').test(code)) { used.add(helper); parts.push(body); }
      }
    }
    const text = parts.join('\n'), code = rustCode(text);
    const dependencies = [...new Set([...code.matchAll(/\b(jet(?:_[a-z][a-z0-9_]*)?|jetpack|rustls|rustls_pemfile|wat)\s*::/gu)].map((m) => m[1].replaceAll('_', '-')).filter((name) => crates.has(name)))].sort();
    const cli = /CARGO_BIN_EXE_(?:jet|jetpack|jetos)(?:["\s])/u.test(text) || /\b(?:jet_bin|jetpack_bin|jet_command|jetpack_command)\s*\(/u.test(code);
    const internal = dependencies.length > 0 || /#\[path\s*=\s*"\.\.\/(?:Source|crates|Compiler)\//u.test(text);
    // Pure harness/registry tests have no portable CLI case, even when they do
    // not import a production crate. Retire/rehome these with the Rust harness.
    const category = cli ? (internal ? 'c' : 'a') : 'b';
    const gate = name === 'golden.rs' ? 'golden;workspace-test' : name === 'diagnostic_snapshots.rs' ? 'UI;lint;workspace-test' : name.includes('/') ? `support:${[...(owners.get(file) ?? [])].sort().join(',') || 'unreferenced-Rust-fixture'}` : 'workspace-test';
    rows.push({ file: `tests/${name}`, lines: source.split('\n').length - Number(source.endsWith('\n')), class: category, dependencies: dependencies.join(',') || (internal ? 'jet' : cli ? '-' : 'Rust-test-harness'), gate, helpers: [...used].sort().join(',') || '-' });
  }
  return rows;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const output = process.argv[2];
  if (!output) throw new Error('usage: test-harness-inventory.mjs OUTPUT.tsv');
  const rows = inventory();
  writeFileSync(output, 'file\tlines\tclass\tcrate_dependencies\tgate_membership\tcommon_helpers\n' + rows.map((r) => [r.file, r.lines, r.class, r.dependencies, r.gate, r.helpers].join('\t')).join('\n') + '\n');
  console.log(JSON.stringify({ files: rows.length, lines: rows.reduce((n, r) => n + r.lines, 0), classes: Object.fromEntries(['a', 'b', 'c'].map((c) => [c, rows.filter((r) => r.class === c).length])) }));
}
