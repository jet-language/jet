// The gauntlet driver of the corpus-entry exports of
// Tools/gauntlet/harness/run.mjs (#5072 c8): loadEntries and
// validateEntryShape. Tools/gauntlet/entries/run-shape/driver/run.jet is the
// twin on the kit's Shape module.
//
//   node run-shape.mjs entries DIR | shapes MATRIX DIR | corpus
//
// DIR holds one directory per entry, MATRIX is a gauntlet matrix.json, and
// corpus checks the repository's own entries against its matrix. A skipped
// entry's reason is shown up to its first ": ", since what follows is the
// engine's own wording of a read or parse failure.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import { loadEntries, validateEntryShape } from '../run.mjs';

const usage = 'usage: node run-shape.mjs entries DIR | shapes MATRIX DIR | corpus';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..');
const [command, first, second] = process.argv.slice(2);
const read = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));
const shapes = async (matrix, dir) => {
  const { loaded } = await loadEntries(dir);
  const issues = loaded.flatMap((item) => validateEntryShape(item, matrix));
  for (const issue of issues) console.log(issue);
  console.log(`${issues.length} issues`);
};

if (command === 'entries' && first) {
  const { loaded, tools, skipped } = await loadEntries(first);
  for (const item of loaded) console.log(`loaded\t${item.directoryName}\t${JSON.stringify(item.entry.name)}\t${item.nameDeclared}`);
  for (const item of tools) console.log(`tool\t${item.directoryName}`);
  for (const item of skipped) console.log(`skipped\t${item.name}\t${item.reason.split(': ')[0]}`);
} else if (command === 'shapes' && first && second) {
  await shapes(read(first), second);
} else if (command === 'corpus') {
  await shapes(read(path.join(repo, 'Tools/gauntlet/matrix.json')), path.join(repo, 'Tools/gauntlet/entries'));
} else {
  console.error(usage);
  process.exit(64);
}
