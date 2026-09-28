import http from "node:http";

const port = Number(process.argv[2] ?? 18080);
const values = new Map();

function send(response, status, payload, onFinish = null) {
  const body = Buffer.from(JSON.stringify(payload));
  response.statusCode = status;
  response.setHeader("Content-Type", "application/json");
  response.setHeader("Content-Length", body.length);
  response.end(body, onFinish ?? undefined);
}

const server = http.createServer((request, response) => {
  const url = new URL(request.url, "http://127.0.0.1");
  const key = url.pathname.startsWith("/kv/") ? url.pathname.slice(4) : null;
  if (request.method === "GET" && url.pathname === "/health") {
    send(response, 200, { status: "ok" });
    return;
  }
  if (request.method === "GET" && url.pathname === "/shutdown") {
    send(response, 200, { bye: true }, () => server.close());
    return;
  }
  if (request.method === "GET" && key !== null) {
    if (!values.has(key)) send(response, 404, { error: "not found" });
    else send(response, 200, { key, value: values.get(key) });
    return;
  }
  if (request.method === "PUT" && key !== null) {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => {
      values.set(key, Buffer.concat(chunks).toString("utf8"));
      send(response, 200, { stored: key });
    });
    return;
  }
  send(response, 404, { error: "not found" });
});

server.listen(port, "127.0.0.1");
