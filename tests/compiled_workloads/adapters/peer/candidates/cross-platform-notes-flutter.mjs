import fs from "node:fs";
const rows = fs.readFileSync(process.argv[2], "utf8").split(/\r?\n/);
let notes = 0, focus = "title", search = "not-run", persistence = "not-saved", readonly = "not-run", corrupt = "not-run";
let active = false, saved = false, readOnlyStorage = false, unknown = false, query = "", title = "", body = "";
const updateSearch = () => { search = title.includes(query) || body.includes(query) ? "found" : "not-found"; };
for (const row of rows) {
  if (row.length === 0) continue;
  const separator = row.indexOf(":");
  if (separator < 0) { unknown = true; continue; }
  const key = row.slice(0, separator), value = row.slice(separator + 1);
  if (key === "key") {
    if (value === "add") { notes += 1; active = true; title = ""; body = ""; focus = "title"; }
    else if (value === "edit") { if (notes > 0) active = true; focus = "title"; }
    else if (value === "search") updateSearch();
    else if (value === "save") { if (!readOnlyStorage) { saved = true; persistence = "saved"; } }
    else if (value === "reload") { if (saved) persistence = "reloaded"; }
    else if (value === "readonly") { readOnlyStorage = true; readonly = "blocked"; }
    else if (value === "corrupt") corrupt = "rejected";
    else unknown = true;
  } else if (key === "title") {
    if (active) { title = value; if (search !== "not-run") updateSearch(); }
  } else if (key === "body") {
    if (active) { body = value; if (search !== "not-run") updateSearch(); }
  } else if (key === "query") {
    query = value; if (search !== "not-run") updateSearch();
  } else unknown = true;
}
const output = [`notes=${notes}`, `focus=${focus}`, `search=${search}`, `persistence=${persistence}`, `readonly=${readonly}`, `corrupt=${corrupt}`];
if (unknown) output.push("reject=unknown-key");
process.stdout.write(output.join("\n") + "\n");
