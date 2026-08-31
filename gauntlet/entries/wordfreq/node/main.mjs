import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "input.txt";
const words = readFileSync(input, "utf8").split(/[ \t\n\r\f\v]+/).filter(Boolean);
const counts = new Map();
for (const word of words) counts.set(word, (counts.get(word) ?? 0) + 1);
const top = [...counts.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .slice(0, 20);
for (const [word, count] of top) console.log(`${count} ${word}`);
console.log(`distinct ${counts.size} total ${words.length}`);
