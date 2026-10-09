// One result stream may cover the union of several native exit corpora.
// Consumers select their stems, but duplicate or malformed evidence fails loud.
import {readFileSync} from 'node:fs';
export function readGoldenResults(file) {
  const text = readFileSync(file, 'utf8').trim();
  if (!text) throw Error(`empty native golden results: ${file}`);
  const rows = new Map();
  for (const line of text.split('\n')) {
    const [stem, verdict] = line.split('\t');
    if (!stem || !verdict || rows.has(stem)) throw Error(`malformed or duplicate native golden result in ${file}: ${line}`);
    rows.set(stem, verdict);
  }
  return rows;
}
