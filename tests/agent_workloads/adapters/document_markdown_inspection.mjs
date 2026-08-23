import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

if (process.argv.length !== 3) {
  throw new Error("usage: document_markdown_inspection INPUT_ROOT");
}

function files(root) {
  return readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}

const rows = [];
for (const path of files(process.argv[2]).sort()) {
  if (!statSync(path).isFile()) continue;
  let headings = 0;
  let bullets = 0;
  let malformed = false;
  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    const stripped = line.trim();
    if (line.startsWith("#")) {
      if (stripped === "#") malformed = true;
      else headings += 1;
    }
    if (line.startsWith("- ")) bullets += 1;
  }
  const name = relative(process.argv[2], path).replaceAll("\\", "/");
  if (malformed) {
    rows.push(`reject|${name}|empty-heading`);
  } else {
    rows.push(`document|${name}|headings=${headings}|bullets=${bullets}`);
  }
}
for (const row of rows.sort()) console.log(row);
