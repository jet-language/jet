#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const AXES = Object.freeze([
  'functions', 'declarations', 'depth', 'modules', 'match-arms',
  'string-literals', 'interpolation', 'fan-in', 'generics',
]);

// Only the selected dimension changes. Extra call sites/bindings are the
// necessary consumers, not additional independently scaled workloads.
export function sources(axis, n) {
  if (!AXES.includes(axis)) throw new Error(`Unknown axis: ${axis}`);
  if (!Number.isSafeInteger(n) || n < 1) throw new Error('N must be a positive safe integer');
  const files = {};
  const items = [];
  const body = ['    total := 0'];
  switch (axis) {
    case 'functions':
      for (let i = 0; i < n; i++) {
        items.push(`fn work_${i}(x: Int) -> Int { x + 1 }`);
        body.push(`    total += work_${i}(1)`);
      }
      break;
    case 'declarations':
      for (let i = 0; i < n; i++) {
        // Exactly N declarations, cycling through all three declaration kinds.
        items.push(i % 3 === 0 ? `struct Record${i} { value: Int }`
          : i % 3 === 1 ? `enum Choice${i} { First\n    Second }`
          : `alias Alias${i}<T> :: [T]`);
      }
      break;
    case 'depth':
      items.push(`fn nested(x: Int) -> Int { ${'(1 + '.repeat(n)}x${')'.repeat(n)} }`);
      body.push('    total += nested(1)');
      break;
    case 'modules':
      for (let i = 0; i < n; i++) {
        items.push(`module part_${i}`);
        files[`part_${i}.jet`] = `pub VALUE :: ${i}\n`;
        body.push(`    total += part_${i}.VALUE`);
      }
      break;
    case 'match-arms':
      items.push(`fn classify(x: Int) -> Int {\n    if x == {\n${Array.from({ length: n }, (_, i) => `        ${i} -> ${i}`).join('\n')}\n        else -> 0\n    }\n}`);
      body.push('    total += classify(0)');
      break;
    case 'string-literals':
      items.push(`fn text() -> String { "${'x'.repeat(n)}" }`);
      body.push('    total += text().len()');
      break;
    case 'interpolation':
      items.push(`fn text(x: Int) -> String { "${'x{x}'.repeat(n)}" }`);
      body.push('    total += text(1).len()');
      break;
    case 'fan-in':
      items.push('fn shared(x: Int) -> Int { x + 1 }');
      // Fan-in counts call edges (sites), not distinct caller definitions.
      for (let i = 0; i < n; i++) body.push('    total += shared(1)');
      break;
    case 'generics':
      // Distinct fixed-array type arguments without O(N^2) initializer text,
      // new nominal declarations, or increasingly nested generic types.
      items.push('fn probe<T>(value: T?) -> Int {\n    if value == {\n        .Val(_) -> 1\n        .None -> 0\n    }\n}');
      for (let i = 1; i <= n; i++) body.push(`    total += probe<[Int#${i}]>(None)`);
      break;
  }
  body.push('    print(total)');
  files['main.jet'] = `// Deterministic scaling workload: ${axis}, N=${n}\n${items.join('\n\n')}\n\nfn run() {\n${body.join('\n')}\n}\n`;
  return files;
}

export function generate(axis, n, directory) {
  const files = sources(axis, n);
  fs.mkdirSync(directory, { recursive: true });
  // Refuse to leave stale module files when regenerating a smaller workload.
  for (const name of Object.keys(files)) {
    if (fs.existsSync(path.join(directory, name))) throw new Error(`Output already exists: ${path.join(directory, name)}`);
  }
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(directory, name), text);
  return path.resolve(directory, 'main.jet');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [axis, size, directory, ...extra] = process.argv.slice(2);
    if (!directory || extra.length) throw new Error(`Usage: node generate.mjs AXIS N NEW_DIRECTORY\nAxes: ${AXES.join(', ')}`);
    console.log(generate(axis, Number(size), directory));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
