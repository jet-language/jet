// Package-building helpers shared by the scaling corpora (corpora.mjs) and
// the domain corpora (domains.mjs), Tower #4601.

import crypto from 'node:crypto';

export const MODULUS = 1000003;
const M = MODULUS;

export function manifest(name) {
  return `name: "${name.replace(/\W/g, '_')}"\nversion: "0.1.0"\nedition: "2028"\noutputs: { app: .Executable{ entry: run } }\n`;
}

// mulberry32: the stream depends only on the seed.
export function random(seed) {
  let state = seed >>> 0;
  const next = () => {
    state = (state + 0x6D2B79F5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int: (lo, hi) => lo + Math.floor(next() * (hi - lo + 1)),
    pick: list => list[Math.floor(next() * list.length)],
  };
}

export function isCodeLine(line) {
  const text = line.trim();
  return text.length > 0 && !text.startsWith('//');
}

export function codeLines(files) {
  let total = 0;
  for (const [name, text] of Object.entries(files)) {
    if (name.endsWith('package.jet')) continue;
    for (const line of text.split('\n')) if (isCodeLine(line)) total += 1;
  }
  return total;
}

export function sourceDigest(files) {
  const hash = crypto.createHash('sha256');
  for (const name of Object.keys(files).sort()) hash.update(`${name}\0${files[name]}\0`);
  return hash.digest('hex');
}

// Consumers: `entries` are functions `(seed: Int) -> Int`. They are called
// from group functions of at most GROUP calls each (a tree, so no consumer
// function grows with N), written to `calls_<prefix>_<k>.jet` files of at
// most FILE_GROUPS groups; `run` calls the roots.
const GROUP = 32;
const FILE_GROUPS = 24;
export function consumerTree(files, entries, prefix) {
  let level = entries;
  let depth = 0;
  while (level.length > GROUP) {
    const next = [];
    const groups = [];
    for (let start = 0; start < level.length; start += GROUP) {
      const name = `${prefix}_l${depth}_g${start / GROUP}`;
      const body = [`fn ${name}(seed: Int) -> Int {`, `    acc := seed % ${M}`];
      level.slice(start, start + GROUP).forEach((entry, index) => {
        body.push(`    acc = (acc * 31 + ${entry}(acc + ${index + 1})) % ${M}`);
      });
      body.push('    acc', '}', '');
      groups.push(body);
      next.push(name);
    }
    for (let start = 0; start < groups.length; start += FILE_GROUPS) {
      files[`calls/${prefix}_l${depth}_${start / FILE_GROUPS}.jet`] = groups.slice(start, start + FILE_GROUPS).flat().join('\n');
    }
    level = next;
    depth += 1;
  }
  return level;
}

// The entry file: `run` folds every root into one checksum line, after any
// declarations the shape already placed in main.jet.
export function finish(name, files, roots) {
  const lines = ['fn run() {', `    checksum := 0`];
  roots.forEach((root, index) => lines.push(`    checksum = (checksum * 31 + ${root}(${index + 1})) % ${M}`));
  lines.push(`    print("${name} checksum {checksum}")`, '}', '');
  files['main.jet'] = `${files['main.jet'] ? `${files['main.jet']}\n` : ''}${lines.join('\n')}`;
  files['package.jet'] = manifest(name);
  return files;
}

// Split `items` (arrays of lines) into files of about `perFile` code lines.
export function pack(files, prefix, items, perFile = 600) {
  let current = [];
  let size = 0;
  let index = 0;
  const flush = () => {
    if (!current.length) return;
    files[`${prefix}_${index}.jet`] = current.join('\n');
    index += 1;
    current = [];
    size = 0;
  };
  for (const lines of items) {
    for (const line of lines) current.push(line);
    current.push('');
    size += lines.length;
    if (size >= perFile) flush();
  }
  flush();
}

