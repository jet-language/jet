import { readFile } from "node:fs/promises";
import { readdir } from "node:fs/promises";
import path from "node:path";

const root = process.argv[2] ?? "files";
const needle = process.argv[3] ?? "needle-7f";
const entries = await readdir(root, { withFileTypes: true });
const files = entries
  .filter((entry) => entry.isFile() && entry.name.endsWith(".txt"))
  .map((entry) => entry.name)
  .sort();

function countOccurrences(text, value) {
  let count = 0;
  let offset = 0;
  while (true) {
    const found = text.indexOf(value, offset);
    if (found < 0) return count;
    count += 1;
    offset = found + value.length;
  }
}

const results = await Promise.all(files.map(async (name) => {
  const text = await readFile(path.join(root, name), "utf8");
  let count = 0;
  for (const line of text.split(/\r?\n/)) count += countOccurrences(line, needle);
  return [name, count];
}));
let total = 0;
let matched = 0;
for (const [name, count] of results) {
  if (count === 0) continue;
  console.log(`${path.join(root, name)}:${count}`);
  total += count;
  matched += 1;
}
console.log(`files ${matched}/${files.length} total ${total}`);
