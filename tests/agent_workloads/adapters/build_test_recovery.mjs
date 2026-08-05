import { cpSync, rmSync } from "node:fs";
import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

if (process.argv.length !== 3) {
  throw new Error("usage: build_test_recovery INPUT_ROOT");
}

const project = "project";
cpSync(process.argv[2], project, { recursive: true });

function run(program, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.resume();
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout }));
  });
}

try {
  const invalid = await run("node", ["--check", `${project}/invalid.mjs`]);
  if (invalid.code === 0) throw new Error("invalid source passed");

  const checked = await run("node", ["--check", `${project}/valid.mjs`]);
  if (checked.code !== 0) throw new Error("valid source did not build");

  const output = [];
  const originalLog = console.log;
  console.log = (...args) => output.push(args.join(" "));
  try {
    await import(pathToFileURL(resolve(project, "valid.mjs")).href);
  } finally {
    console.log = originalLog;
  }
  if (output.length !== 1) throw new Error("valid source test failed");
  console.log("recovery=ok");
  console.log(`test=${output[0]}`);
} finally {
  rmSync(project, { recursive: true, force: true });
}
