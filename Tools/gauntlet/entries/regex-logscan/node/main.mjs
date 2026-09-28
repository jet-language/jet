import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "access.log";
const pattern = /^([0-9.]+) - - \[[^\]]+\] "GET (\/api\/[^ ]+) HTTP\/1\.1" 5[0-9]{2} /;
const counts = new Map();
let total = 0;
for (const rawLine of readFileSync(input, "utf8").split("\n")) {
  const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
  const match = pattern.exec(line);
  if (!match) continue;
  const ip = match[1];
  counts.set(ip, (counts.get(ip) ?? 0) + 1);
  total += 1;
}
const top = [...counts.entries()]
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .slice(0, 5);
console.log(`matches ${total}`);
for (const [ip, count] of top) console.log(`${count} ${ip}`);
