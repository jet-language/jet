import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { marker } from "./client.mjs";

const portIndex = process.argv.indexOf("--port");
const port = Number(portIndex >= 0 ? process.argv[portIndex + 1] : "");
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("missing --port");

const counterPath = ".axis-counter";
let counter = 0;
try {
  counter = Number.parseInt(await readFile(counterPath, "utf8"), 10) || 0;
} catch {}
counter += 1;
await writeFile(counterPath, `${counter}\n`);

createServer((request, response) => {
  if (request.url === "/__axis_ready") {
    response.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
    response.end(String(counter));
    return;
  }
  if (request.url === "/__axis_output") {
    response.writeHead(200, { "content-type": "text/plain; charset=utf-8" });
    response.end(marker);
    return;
  }
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(`<div id="axis-ready">${marker}</div>`);
}).listen(port, "127.0.0.1");
