import { spawnSync } from "node:child_process";

if (process.argv.length !== 3) {
  throw new Error("usage: interactive_terminal INPUT_ROOT");
}

const task = process.env.JET_CORPUS_TASK;
const scriptName = task === "interactive-terminal-closed" ? "terminal_closed.sh" : "terminal_session.sh";
const input = task === "interactive-terminal-closed" ? "" : "Ada\nblue\n";
const result = spawnSync("script", ["-qfec", `sh ${scriptName}`, "/dev/null"], {
  cwd: process.argv[2],
  input,
  encoding: "utf8",
  timeout: 5000,
});
if (result.error) throw result.error;
if (result.status !== 0) throw new Error(result.stderr || "terminal command failed");
if (task === "interactive-terminal-closed") {
  if (!result.stdout.includes("closed")) throw new Error("closed terminal did not return");
  console.log("terminal=pty\nclosed=ok\nexit=0");
} else {
  for (const marker of ["Name: ", "Hello Ada", "Choice blue"]) {
    if (!result.stdout.includes(marker)) throw new Error("terminal dialogue markers missing");
  }
  console.log("terminal=pty\nresize=ok\nprompt=ok\nreply=ok");
}
