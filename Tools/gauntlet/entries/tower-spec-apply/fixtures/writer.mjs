// Stand-in Tower writer: prints its arguments (a --file by its name) and the
// file's text, and appends both to writes.log beside it, the record of every
// Tower write a case makes.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const args = process.argv.slice(2);
const at = args.indexOf("--file");
const line = `tower-dev: ${args.map((a, i) => (at >= 0 && i === at + 1 ? path.basename(a) : a)).join(" ")}`;
const patch = at >= 0 ? fs.readFileSync(args[at + 1], "utf8") : "";
console.log(line);
if (at >= 0) console.log(patch);
fs.appendFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "writes.log"), `${line}\n${patch ? `${patch}\n` : ""}`);
