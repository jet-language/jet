import { readFileSync } from "node:fs";
import { TextDecoder } from "node:util";

// Code point order matches UTF-8 byte order for valid UTF-8 strings.
function compareCodePoints(left, right) {
  let leftIndex = 0;
  let rightIndex = 0;
  while (leftIndex < left.length && rightIndex < right.length) {
    const leftCodePoint = left.codePointAt(leftIndex);
    const rightCodePoint = right.codePointAt(rightIndex);
    if (leftCodePoint !== rightCodePoint) return leftCodePoint - rightCodePoint;
    leftIndex += leftCodePoint > 0xffff ? 2 : 1;
    rightIndex += rightCodePoint > 0xffff ? 2 : 1;
  }
  return (leftIndex < left.length) - (rightIndex < right.length);
}

const input = process.argv[2] ?? "input.txt";
const text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(
  readFileSync(input),
);
const words = text.split(/[ \t\n\r\f\v]+/).filter(Boolean);
const counts = new Map();
for (const word of words) counts.set(word, (counts.get(word) ?? 0) + 1);
const top = [...counts.entries()]
  .sort((a, b) => b[1] - a[1] || compareCodePoints(a[0], b[0]))
  .slice(0, 20);
for (const [word, count] of top) console.log(`${count} ${word}`);
console.log(`distinct ${counts.size} total ${words.length}`);
