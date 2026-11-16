import { spawnSync } from "node:child_process";

let ok = 0;
for (let i = 0; i < 100; i++) {
  if (spawnSync("true").status === 0) ok++;
}
console.log(`children ok=${ok}`);
