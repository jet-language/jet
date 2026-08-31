import { readFileSync } from "node:fs";

const input = process.argv[2] ?? "meetings.txt";
const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const meetings = [];
for (const rawLine of readFileSync(input, "utf8").split("\n")) {
  const line = rawLine.trim();
  if (!line) continue;
  const separator = line.indexOf("|");
  const name = line.slice(0, separator);
  const value = line.slice(separator + 1);
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) throw new Error(`invalid timestamp ${value}`);
  meetings.push({ name, timestamp });
}
meetings.sort((a, b) => a.timestamp - b.timestamp || a.name.localeCompare(b.name));
let previous = null;
for (const meeting of meetings) {
  const date = new Date(meeting.timestamp);
  const iso = date.toISOString().slice(0, 19) + "Z";
  const gap = previous === null ? "-" : String(Math.floor((meeting.timestamp - previous) / 60000));
  console.log(`${meeting.name} utc=${iso} day=${days[date.getUTCDay()]} gap=${gap}`);
  previous = meeting.timestamp;
}
console.log(`span ${Math.floor(((meetings.at(-1)?.timestamp ?? meetings[0]?.timestamp ?? 0) - (meetings[0]?.timestamp ?? 0)) / 60000)} minutes ${meetings.length}`);
