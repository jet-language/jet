import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";

const root = path.resolve(process.argv[2] || ".");
const rows = [];
let bytes = 0;
let links = 0;
let rejects = 0;
function digest(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}
function walk(directory, relative = "") {
  let names;
  try {
    names = fs.readdirSync(directory).sort();
  } catch {
    rejects += 1;
    return;
  }
  for (const name of names) {
    if (name === ".fixture-modes.tsv") continue;
    const file = path.join(directory, name);
    const childRelative = relative ? `${relative}/${name}` : name;
    let stat;
    try {
      stat = fs.lstatSync(file);
    } catch {
      rejects += 1;
      continue;
    }
    if (stat.isSymbolicLink()) {
      links += 1;
    } else if (stat.isDirectory()) {
      walk(file, childRelative);
    } else if (stat.isFile()) {
      let data;
      try {
        data = fs.readFileSync(file);
      } catch {
        rejects += 1;
        continue;
      }
      bytes += data.length;
      rows.push(`file|${childRelative}|${data.length}|${digest(file)}`);
    } else {
      rejects += 1;
    }
  }
}
walk(root);
rows.sort();
const canonical = rows.map(row => `${row}\n`).join("");
const index = crypto.createHash("sha256").update(canonical).digest("hex");
process.stdout.write(`files=${rows.length}\nbytes=${bytes}\nlinks=${links}\nrejects=${rejects}\nindex_sha256=${index}\n${rows.join("\n")}${rows.length ? "\n" : ""}`);
