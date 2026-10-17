#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SHAPES, corpus } from './corpora.mjs';
import { mixedSources } from './mixed.mjs';

// Writes the package into a new directory and returns its entry.
export function writeFiles(files, directory) {
  fs.mkdirSync(directory, { recursive: true });
  // Refuse to leave stale module files when regenerating a smaller workload.
  for (const name of Object.keys(files)) {
    if (fs.existsSync(path.join(directory, name))) throw new Error(`Output already exists: ${path.join(directory, name)}`);
  }
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(directory, name)), { recursive: true });
    fs.writeFileSync(path.join(directory, name), text);
  }
  return path.resolve(directory, 'main.jet');
}

// One corpus of the scaling family (corpora.mjs): entry, digest, expected
// diagnostics and code lines.
export function generate(shape, n, directory, seed = 1) {
  const built = corpus(shape, n, seed);
  return { entry: writeFiles(built.files, directory), shape, n, seed, kind: built.kind,
    code_lines: built.code_lines, files: Object.keys(built.files).length, expect: built.expect, source_sha256: built.source_sha256 };
}

// The `mixed` profile (mixed.mjs): returns the entry, the package digest, the
// expected stdout and the shape statistics.
export function generateMixed(codeLines, seed, directory) {
  const { files, expected, stats, source_sha256 } = mixedSources(codeLines, seed);
  return { entry: writeFiles(files, directory), source_sha256, expected, stats };
}

const USAGE = `Usage: node generate.mjs SHAPE N [--seed S] NEW_DIRECTORY
       node generate.mjs mixed --code-lines N --seed S NEW_DIRECTORY
       node generate.mjs --list
N is the stress dimension for stress shapes and code lines for domain shapes.
Shapes: ${Object.keys(SHAPES).join(', ')}
Both forms print one JSON line describing the written package.`;

function numericFlags(args, names) {
  const values = {};
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    if (names.includes(args[i])) {
      if (args[i + 1] === undefined || !/^\d+$/.test(args[i + 1])) throw new Error(`${args[i]} needs a nonnegative integer\n${USAGE}`);
      values[args[i]] = Number(args[++i]);
    } else {
      rest.push(args[i]);
    }
  }
  return { values, rest };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [shape, ...args] = process.argv.slice(2);
    if (shape === '--list') {
      for (const [name, entry] of Object.entries(SHAPES)) console.log(`${name.padEnd(14)} ${entry.kind.padEnd(7)} base ${String(entry.base).padStart(7)} ${entry.unit}: ${entry.dimension}`);
    } else if (shape === 'mixed') {
      const { values, rest } = numericFlags(args, ['--code-lines', '--seed']);
      if (values['--code-lines'] === undefined || values['--seed'] === undefined || rest.length !== 1) throw new Error(USAGE);
      console.log(JSON.stringify(generateMixed(values['--code-lines'], values['--seed'], rest[0])));
    } else {
      const { values, rest } = numericFlags(args, ['--seed']);
      if (rest.length !== 2 || !/^\d+$/.test(rest[0])) throw new Error(USAGE);
      console.log(JSON.stringify(generate(shape, Number(rest[0]), rest[1], values['--seed'] ?? 1)));
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
