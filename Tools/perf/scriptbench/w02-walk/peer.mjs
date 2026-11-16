import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

let files = 0;
let bytes = 0;
function walk(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path);
    else {
      files += 1;
      bytes += statSync(path).size;
    }
  }
}
walk("w02");
console.log(`files=${files} bytes=${bytes}`);
