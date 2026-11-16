import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: desktop_interaction_focus INPUT_FILE");
}

const focus = ["Save", "Cancel"];
let index = 1;
for (const [lineNumber, key] of readFileSync(process.argv[2], "utf8")
  .split(/\r?\n/)
  .entries()) {
  if (lineNumber === 0 || !key) continue;
  if (key === "Tab") {
    console.log(`focus|${focus[index]}`);
    index = (index + 1) % focus.length;
  } else if (key === "Empty") {
    console.log("event|Empty|observed");
  } else {
    console.log(`event|${key}|observed`);
  }
}
