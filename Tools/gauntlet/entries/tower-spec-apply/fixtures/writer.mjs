// Stand-in Tower writer: prints its arguments (a --file by its name) and the file's text.
import fs from "node:fs";
import path from "node:path";
const args = process.argv.slice(2);
const at = args.indexOf("--file");
console.log(`tower-dev: ${args.map((a, i) => (at >= 0 && i === at + 1 ? path.basename(a) : a)).join(" ")}`);
if (at >= 0) console.log(fs.readFileSync(args[at + 1], "utf8"));
