import { readFile, readdir } from "node:fs/promises";
import { availableParallelism } from "node:os";
import path from "node:path";

const root = process.argv[2] ?? "files";
const needle = process.argv[3] ?? "needle-7f";

async function collectFiles(directory, files = []) {
  const entries = await readdir(directory, { withFileTypes: true });
  entries.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
  for (const entry of entries) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) await collectFiles(full, files);
    else if (entry.isFile() && entry.name.endsWith(".txt")) files.push(full);
  }
  return files;
}

function countOccurrences(text, value) {
  if (value.length === 0) return 0;
  let count = 0;
  let offset = 0;
  while (true) {
    const found = text.indexOf(value, offset);
    if (found < 0) return count;
    count += 1;
    offset = found + value.length;
  }
}

const files = await collectFiles(root);
let cursor = 0;
const matches = [];

async function scan() {
  while (cursor < files.length) {
    const file = files[cursor++];
    const text = await readFile(file, "utf8");
    let count = 0;
    for (const line of text.split(/\r?\n/)) count += countOccurrences(line, needle);
    if (count > 0) matches.push([file, count]);
  }
}

await Promise.all(Array.from({ length: Math.min(32, availableParallelism()) }, scan));
matches.sort((left, right) => left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0);
let total = 0;
for (const [file, count] of matches) {
  console.log(`${file}:${count}`);
  total += count;
}
console.log(`files ${matches.length}/${files.length} total ${total}`);
