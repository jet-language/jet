import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: process_batch INPUT_FILE");
}

function run(program, argument, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, [argument], { stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    let timedOut = false;
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.on("error", reject);
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, output: output.trim(), timedOut });
    });
  });
}

const lines = readFileSync(process.argv[2], "utf8").split(/\r?\n/);
for (let index = 1; index < lines.length; index += 1) {
  const line = lines[index];
  if (!line) continue;
  const fields = line.split("\t");
  if (fields.length !== 4) {
    throw new Error(`bad process row ${index + 1}`);
  }
  const [label, program, argument, timeoutText] = fields;
  const result = await run(program, argument, Number.parseInt(timeoutText, 10));
  if (result.timedOut) {
    console.log(`${label}|timeout`);
  } else {
    console.log(`${label}|exit=${result.code}|stdout=${result.output}`);
  }
}
