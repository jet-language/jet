// A stand-in for census-errors.mjs: the keep-going report's rows, one line
// each; without a report, the files of the tree it runs in and their first line.
import fs from "node:fs";
import path from "node:path";
const [report] = process.argv.slice(2);
if (!fs.existsSync(report)) {
  console.error(`census-errors: no keep-going report at ${report}`);
  const walk = (dir) => fs.readdirSync(dir, { withFileTypes: true })
    .flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));
  for (const file of walk(".").sort()) console.log(`  ${file}: ${fs.readFileSync(file, "utf8").split("\n")[0]}`);
  process.exit(1);
}
const rows = fs.readFileSync(report, "utf8").split("\n").slice(1).filter((l) => l !== "");
console.log(`census-errors: ${rows.length} rows`);
for (const row of rows) {
  const [kind, location, code, message] = row.split("\t");
  console.log(`${location}: ${kind} ${code}: ${message}`);
}
