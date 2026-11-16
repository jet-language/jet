// The local HTTP fixture of w08-http and w11-fetch: node server.js PORT DELAY_MS.
// GET /item/<i> answers 200 with a JSON body of about 1 KB after DELAY_MS.
import { createServer } from "node:http";

const port = Number(process.argv[2]);
const delay = Number(process.argv[3] ?? 0);
const body = (i) => JSON.stringify({ id: i, data: "x".repeat(1000 + (i % 7)) });

createServer((req, res) => {
  const m = /^\/item\/(\d+)$/.exec(req.url);
  if (!m) { res.writeHead(404); res.end("not found\n"); return; }
  const send = () => {
    const b = body(Number(m[1]));
    res.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(b) });
    res.end(b);
  };
  if (delay > 0) setTimeout(send, delay); else send();
}).listen(port, "127.0.0.1", () => console.log(`listening ${port}`));
