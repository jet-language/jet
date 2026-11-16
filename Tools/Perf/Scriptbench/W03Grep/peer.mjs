import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";

const rx = /status=5\d\d ms=(\d+)/;
let count = 0;
let total = 0;
for await (const line of createInterface({ input: createReadStream("data/log100m.txt") })) {
  const m = rx.exec(line);
  if (m) {
    count++;
    total += Number(m[1]);
  }
}
console.log(`matches ${count}`);
console.log(`ms_sum ${total}`);
