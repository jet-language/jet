import { existsSync, readFileSync, writeFileSync } from "node:fs";

function load() {
  return existsSync("tasks.json") ? JSON.parse(readFileSync("tasks.json", "utf8")) : { tasks: [] };
}

function save(store) {
  writeFileSync("tasks.json", JSON.stringify(store), "utf8");
}

const command = process.argv[2] ?? "";
const store = load();
const tasks = store.tasks;
if (command === "add") {
  const text = process.argv[3] ?? "";
  const task = { id: tasks.length + 1, text, done: false };
  tasks.push(task);
  save(store);
  console.log(`added ${task.id} ${text}`);
} else if (command === "done") {
  const taskId = Number(process.argv[3] ?? -1);
  const task = tasks.find((candidate) => candidate.id === taskId);
  if (!task) {
    console.log(`no task ${taskId}`);
  } else {
    task.done = true;
    save(store);
    console.log(`done ${taskId}`);
  }
} else if (command === "list") {
  const open = tasks.filter((task) => !task.done).length;
  for (const task of tasks) console.log(`[${task.done ? "x" : " "}] ${task.id} ${task.text}`);
  console.log(`open ${open} done ${tasks.length - open}`);
}
