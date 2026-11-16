import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";

const counts = new Map();
let total = 0;
for await (const line of createInterface({ input: createReadStream("data/words.txt") })) {
  for (const word of line.toLowerCase().match(/[a-z]+/g) ?? []) {
    counts.set(word, (counts.get(word) ?? 0) + 1);
    total++;
  }
}
const top = [...counts].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
console.log(`total ${total}`);
console.log(`distinct ${counts.size}`);
for (const [word, n] of top.slice(0, 10)) console.log(`${n} ${word}`);
