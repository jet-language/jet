const output = [];
const originalLog = console.log;
console.log = (...values) => output.push(values.join(" "));

try {
  const { jet_main } = await import("./.jet/build/app.js");
  await jet_main();
  await new Promise((resolve) => setImmediate(resolve));
} finally {
  console.log = originalLog;
}

for (const line of output) originalLog(line);
