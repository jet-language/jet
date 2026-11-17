// Prints the first argv[2] lines of words.txt.
import fs from 'node:fs';

const lines = fs.readFileSync('words.txt', 'utf8').split('\n');
process.stdout.write(lines.slice(0, Number(process.argv[2])).map((line) => `${line}\n`).join(''));
