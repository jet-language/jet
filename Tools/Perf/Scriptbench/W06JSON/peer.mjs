import { readFileSync } from "node:fs";

const people = JSON.parse(readFileSync("w06.json", "utf8"));
let active = 0;
let salary = 0;
for (const person of people) {
  if (!person.active) continue;
  active += 1;
  salary += person.salary;
}
console.log(`records=${people.length} active=${active} salary=${salary}`);
