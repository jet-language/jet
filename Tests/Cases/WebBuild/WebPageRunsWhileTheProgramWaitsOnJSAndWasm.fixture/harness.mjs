// The page's own script: once the program writes its first line, it queues a
// piece of work, which the event loop runs as soon as the program's code
// returns to it and before any timer. A wait that suspends the program
// returns to the loop, so the work runs before the program's second line
// whatever the machine's load; a wait that blocks would hold it until the
// program ended. Under node the host writes stdout with fs.writeSync, so the
// harness watches that.
import fs from "node:fs";
import { syncBuiltinESMExports } from "node:module";

const write = fs.writeSync;
fs.writeSync = (fd, buffer, offset, length, position) => {
  const count = write(fd, buffer, offset, length, position);
  const text = new TextDecoder().decode(buffer.subarray(offset, offset + count));
  if (fd === 1 && text.includes("before the wait")) queueMicrotask(() => write(1, "the page ran while the program waited\n"));
  return count;
};
syncBuiltinESMExports();
const { jet_main } = await import("./app.js");
await jet_main();
