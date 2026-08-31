import { readFileSync } from "node:fs";

function parseCsv(text) {
  const rows = [];
  let row = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i += 1;
        } else {
          quoted = false;
        }
      } else {
        field += ch;
      }
    } else if (ch === '"' && field.length === 0) {
      quoted = true;
    } else if (ch === ",") {
      row.push(field);
      field = "";
    } else if (ch === "\n") {
      row.push(field.endsWith("\r") ? field.slice(0, -1) : field);
      field = "";
      if (row.length > 1 || row[0] !== "") rows.push(row);
      row = [];
    } else {
      field += ch;
    }
  }
  if (field.length > 0 || row.length > 0) {
    row.push(field.endsWith("\r") ? field.slice(0, -1) : field);
    if (row.length > 1 || row[0] !== "") rows.push(row);
  }
  return rows;
}

const input = process.argv[2] ?? "sales.csv";
const rows = parseCsv(readFileSync(input, "utf8"));
const headers = rows.shift() ?? [];
const regionIndex = headers.indexOf("region");
const amountIndex = headers.indexOf("amount");
const grouped = new Map();
let accepted = 0;
for (const row of rows) {
  const amount = Number(row[amountIndex]);
  if (!(amount > 0)) continue;
  const region = row[regionIndex];
  const current = grouped.get(region) ?? { count: 0, total: 0 };
  current.count += 1;
  current.total += amount;
  accepted += 1;
  grouped.set(region, current);
}
let total = 0;
for (const region of [...grouped.keys()].sort()) {
  const value = grouped.get(region);
  total += value.total;
  console.log(`${region} n=${value.count} sum=${value.total.toFixed(2)}`);
}
console.log(`total n=${accepted} sum=${total.toFixed(2)}`);
