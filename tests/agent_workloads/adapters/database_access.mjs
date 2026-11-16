import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";

if (process.argv.length !== 3) {
  throw new Error("usage: database_access INPUT_FILE");
}

const database = new DatabaseSync(":memory:");
try {
  database.exec("CREATE TABLE agent_rows (id INTEGER PRIMARY KEY, name TEXT NOT NULL)");
  const insert = database.prepare("INSERT INTO agent_rows (id, name) VALUES (?, ?)");
  const lines = readFileSync(process.argv[2], "utf8").split(/\r?\n/);
  let invalid = false;
  for (let index = 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line) continue;
    const fields = line.split("\t");
    if (fields.length !== 2) {
      console.log(`invalid-row|${index + 1}|field-count`);
      invalid = true;
      break;
    }
    const rowId = Number.parseInt(fields[0], 10);
    if (!Number.isInteger(rowId) || rowId <= 0) {
      console.log(`invalid-row|${index + 1}|id`);
      invalid = true;
      break;
    }
    insert.run(rowId, fields[1]);
  }
  if (!invalid) {
    const rows = database.prepare("SELECT COUNT(*) AS n FROM agent_rows").get().n;
    const selected = database.prepare("SELECT name FROM agent_rows WHERE id = ?").get(2).name;
    console.log(`rows=${rows}`);
    console.log(`selected=${selected}`);
    console.log("table=present");
  }
} finally {
  database.close();
}
