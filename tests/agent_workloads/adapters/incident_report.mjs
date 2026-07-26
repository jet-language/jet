import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: incident_report INPUT_FILE");
}

const incidents = [];
const rejects = [];
const lines = readFileSync(process.argv[2], "utf8").split(/\r?\n/);
for (let index = 1; index < lines.length; index += 1) {
  const line = lines[index];
  if (!line) continue;
  const fields = line.split("\t");
  const lineNumber = index + 1;
  if (fields.length !== 3) {
    rejects.push(`reject|${lineNumber}|field-count`);
    continue;
  }
  const [service, status] = fields;
  if (status !== "ok" && status !== "error") {
    rejects.push(`reject|${lineNumber}|status`);
  } else if (!service) {
    rejects.push(`reject|${lineNumber}|service`);
  } else {
    incidents.push([service, status]);
  }
}

const rows = [`accepted|${incidents.length}`, `rejected|${rejects.length}`, ...rejects];
const services = [...new Set(incidents.map(([service]) => service))].sort();
for (const service of services) {
  const ok = incidents.filter((item) => item[0] === service && item[1] === "ok").length;
  const errors = incidents.filter(
    (item) => item[0] === service && item[1] === "error",
  ).length;
  rows.push(`${service}|ok=${ok}|error=${errors}`);
}
console.log(rows.join("\n"));
