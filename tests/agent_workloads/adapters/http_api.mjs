import { readFileSync } from "node:fs";
import { createServer, request as httpRequest } from "node:http";

if (process.argv.length !== 3) {
  throw new Error("usage: http_api INPUT_FILE");
}

function responseFor(body) {
  try {
    const payload = JSON.parse(body);
    if (payload.name === "") return [422, "invalid-name"];
    return [200, `accepted|${payload.name}|${payload.role}`];
  } catch {
    return [400, "invalid-json"];
  }
}

const server = createServer((request, response) => {
  const chunks = [];
  request.on("data", (chunk) => chunks.push(chunk));
  request.on("end", () => {
    const [status, body] = request.url === "/agent" && request.method === "POST"
      ? responseFor(Buffer.concat(chunks).toString("utf8"))
      : [404, "not-found"];
    response.statusCode = status;
    response.setHeader("Content-Length", Buffer.byteLength(body));
    response.end(body);
  });
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
try {
  const body = readFileSync(process.argv[2]);
  const result = await new Promise((resolve, reject) => {
    const request = httpRequest(
      {
        hostname: "127.0.0.1",
        port: server.address().port,
        path: "/agent",
        method: "POST",
        headers: { "Content-Type": "application/json", "Content-Length": body.length },
      },
      (response) => {
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () => resolve([response.statusCode, Buffer.concat(chunks).toString("utf8")]));
      },
    );
    request.on("error", reject);
    request.end(body);
  });
  console.log(`${result[0]}|${result[1]}`);
} finally {
  await new Promise((resolve) => server.close(resolve));
}
