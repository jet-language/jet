const output = [];
const originalLog = console.log;
console.log = (...values) => output.push(values.join(" "));

try {
  await import("./build/app.js");
  await new Promise((resolve) => setImmediate(resolve));
} finally {
  console.log = originalLog;
}

for (const line of output) originalLog(line);
