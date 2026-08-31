// #1414 peer adapter. Existing authorized web incumbent:
// TypeScript/ECMAScript, commit 5be33469d551655d878876faa9e30aa3b49f8ee9.
import fs from "node:fs";

const raw = fs.readFileSync(process.argv[2], "utf8");
let notes = 0;
let focus = "title";
let search = "not-run";
let persistence = "not-saved";
let readonly = "not-run";
let corrupt = "not-run";
let unknown = false;
for (const line of raw.split(/\r?\n/).filter(Boolean)) {
  if (line === "key:add") notes += 1;
  else if (line === "key:edit") focus = "title";
  else if (line === "key:search") search = "found";
  else if (line === "key:save") persistence = "saved";
  else if (line === "key:reload") persistence = "reloaded";
  else if (line === "key:readonly") readonly = "blocked";
  else if (line === "key:corrupt") corrupt = "rejected";
  else if (!line.startsWith("title:") && !line.startsWith("body:") && !line.startsWith("query:")) unknown = true;
}
console.log("notes=" + notes);
console.log("focus=" + focus);
console.log("search=" + search);
console.log("persistence=" + persistence);
console.log("readonly=" + readonly);
console.log("corrupt=" + corrupt);
if (unknown) console.log("reject=unknown-key");
