import { parseArgs } from "node:util";

const [command, ...rest] = process.argv.slice(2);
if (command === "greet") {
  const { values } = parseArgs({
    args: rest,
    options: { name: { type: "string" }, times: { type: "string", default: "1" }, shout: { type: "boolean", default: false } },
  });
  if (!values.name) throw new Error("--name is required");
  const text = `Hello, ${values.name}!`;
  for (let i = 0; i < Number(values.times); i++) console.log(values.shout ? text.toUpperCase() : text);
} else if (command === "add") {
  const { values } = parseArgs({ args: rest, options: { a: { type: "string" }, b: { type: "string" } } });
  console.log(Number(values.a) + Number(values.b));
} else {
  console.error("usage: main <greet|add> [options]");
  process.exit(2);
}
