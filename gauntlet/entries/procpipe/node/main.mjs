import { spawn, spawnSync } from "node:child_process";

const calc = spawnSync("python3", ["-c", "print(40 + 2)"], { encoding: "utf8" });
if (calc.status !== 0) throw new Error(calc.stderr.trim());
console.log(`calc ${calc.stdout.trim()}`);

const producer = spawn("python3", ["-c", "print('c'); print('a'); print('b')"], { stdio: ["ignore", "pipe", "pipe"] });
const sorter = spawn("sort", [], { stdio: ["pipe", "pipe", "pipe"] });
producer.stdout.pipe(sorter.stdin);
let sortedOutput = "";
for await (const chunk of sorter.stdout) sortedOutput += chunk.toString();
const [producerStatus, sorterStatus] = await Promise.all([
  new Promise((resolve) => producer.once("close", (status) => resolve(status))),
  new Promise((resolve) => sorter.once("close", (status) => resolve(status))),
]);
if (producerStatus !== 0 || sorterStatus !== 0) throw new Error("pipeline failed");
const sorted = sortedOutput.trim().split(/\s+/).filter(Boolean);
console.log(`sorted ${sorted.join(",")}`);

const slow = spawnSync("python3", ["-c", "import time; time.sleep(5)"], { stdio: "ignore", timeout: 300 });
if (slow.error?.code === "ETIMEDOUT" || slow.signal) console.log("slow timeout");
else console.log(`slow exit ${slow.status}`);

const exited = spawnSync("python3", ["-c", "raise SystemExit(3)"], { stdio: "ignore" });
console.log(`exit ${exited.status}`);
