import fs from "node:fs";
const input = process.argv[2];
try {
  const value = JSON.parse(fs.readFileSync(input, "utf8"));
  process.stdout.write(`canonical=${JSON.stringify(value)}\nvalid=true\n`);
} catch {
  process.stdout.write("reject=json\nvalid=false\n");
}
