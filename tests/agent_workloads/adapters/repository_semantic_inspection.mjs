import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: repository_semantic_inspection INPUT_ROOT");
}

const source = `${process.argv[2]}/project/examples/main.jet`;
const lines = readFileSync(source, "utf8").split(/\r?\n/);
const definitions = lines.filter((line) => line.startsWith("fn ")).length;
const references = lines.filter((line) => /^\s*(?:print|prepare)\(/.test(line)).length;
console.log(`definitions=${definitions}`);
console.log(`references=${references}`);
console.log(`calls=${references}`);
