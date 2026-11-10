// Stand-in Tower reader: `card show N --json` prints the card in cards/N.json.
import fs from "node:fs";
const [verb, sub, num] = process.argv.slice(2);
const file = new URL(`../../cards/${num}.json`, import.meta.url);
if (verb !== "card" || sub !== "show" || !fs.existsSync(file)) {
  console.error(`tower: no card ${num}`);
  process.exit(1);
}
process.stdout.write(fs.readFileSync(file));
