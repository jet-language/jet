import { readFileSync } from "node:fs";

const path = process.argv[2] ?? "corpus.txt";
const text = readFileSync(path, "utf8");
const needle = "struct";
let matches = 0;
let offset = 0;
while ((offset = text.indexOf(needle, offset)) !== -1) {
  matches += 1;
  offset += needle.length;
}
console.log(`matches ${matches}`);
