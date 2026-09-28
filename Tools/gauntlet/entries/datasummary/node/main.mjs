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

function summary(values) {
  const ordered = [...values].sort((a, b) => a - b);
  const n = ordered.length;
  const mean = values.reduce((sum, value) => sum + value, 0) / n;
  const median = ordered[Math.floor((n - 1) / 2)];
  const p95 = ordered[Math.floor((19 * n + 19) / 20) - 1];
  const variance = values.reduce((sum, value) => sum + (value - mean) ** 2, 0) / n;
  return `n=${n} mean=${mean.toFixed(2)} median=${median.toFixed(2)} p95=${p95.toFixed(2)} sd=${Math.sqrt(variance).toFixed(2)}`;
}

const input = process.argv[2] ?? "measurements.csv";
const rows = parseCsv(readFileSync(input, "utf8"));
const headers = rows.shift() ?? [];
const groupIndex = headers.indexOf("group");
const valueIndex = headers.indexOf("value");
const groups = new Map();
for (const row of rows) {
  const values = groups.get(row[groupIndex]) ?? [];
  values.push(Number(row[valueIndex]));
  groups.set(row[groupIndex], values);
}
const allValues = [];
for (const group of [...groups.keys()].sort()) {
  const values = groups.get(group);
  allValues.push(...values);
  console.log(`${group} ${summary(values)}`);
}
const overallMean = allValues.reduce((sum, value) => sum + value, 0) / allValues.length;
console.log(`overall n=${allValues.length} mean=${overallMean.toFixed(2)}`);
