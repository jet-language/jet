// The gauntlet driver of the peer-comparison export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): comparisons.
// Tools/gauntlet/entries/run-comparison/driver/run.jet is the twin on the
// kit's Comparison module.
//
//   node run-comparison.mjs comparisons DIR
//
// DIR holds JSON files, each an object with the `entry`, its `languages`,
// its `rows` and, when it has them, its Jet `tiers`; each is compared in
// name order and shown as the file name and the comparisons as JSON.
import path from 'node:path';
import fs from 'node:fs';
import { comparisons } from '../run.mjs';

const usage = 'usage: node run-comparison.mjs comparisons DIR';
const [command, dir] = process.argv.slice(2);

if (command === 'comparisons' && dir) {
  const names = fs.readdirSync(dir).filter((name) => name.endsWith('.json')).sort();
  for (const name of names) {
    const input = JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'));
    console.log(`${name}\t${JSON.stringify(comparisons(input.entry, input.languages, input.rows, input.tiers))}`);
  }
} else {
  console.error(usage);
  process.exit(64);
}
