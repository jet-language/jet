// The gauntlet driver of the result-shape export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): validateResultShape.
// Tools/gauntlet/entries/run-result/driver/run.jet is the twin on the kit's
// Result module.
//
//   node run-result.mjs results MATRIX DIR
//
// DIR holds result JSON files, each checked in name order against MATRIX, a
// gauntlet matrix.json, or against no matrix when MATRIX is `-`. Each file
// shows its issues, one per line, or `ok` when it has none.
import path from 'node:path';
import fs from 'node:fs';
import { validateResultShape } from '../run.mjs';

const usage = 'usage: node run-result.mjs results MATRIX DIR';
const [command, first, second] = process.argv.slice(2);
const read = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));

if (command === 'results' && first && second) {
  const matrix = first === '-' ? null : read(first);
  const names = fs.readdirSync(second).filter((name) => name.endsWith('.json')).sort();
  for (const name of names) {
    const issues = validateResultShape(read(path.join(second, name)), matrix);
    if (issues.length === 0) console.log(`${name}\tok`);
    for (const issue of issues) console.log(`${name}\t${issue}`);
  }
} else {
  console.error(usage);
  process.exit(64);
}
