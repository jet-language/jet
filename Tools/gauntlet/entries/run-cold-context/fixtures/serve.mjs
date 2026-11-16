// The service a fake candidate runs: /health, /hello/:name and /shutdown,
// after which it exits 0. Mode `bad` answers /hello/:name wrongly.
import http from "node:http";

const [mode, port] = process.argv.slice(2);
const server = http.createServer((request, response) => {
  const reply = (status, type, body) => {
    response.writeHead(status, { "content-type": type });
    response.end(body);
  };
  if (request.url === "/health") return reply(200, "application/json", '{"status":"ok"}');
  if (request.url.startsWith("/hello/")) {
    const name = decodeURIComponent(request.url.slice("/hello/".length));
    return reply(200, "text/plain; charset=utf-8", mode === "bad" ? `Goodbye, ${name}` : `Hello, ${name}`);
  }
  if (request.url === "/shutdown") {
    response.on("finish", () => process.exit(0));
    return reply(200, "application/json", '{"bye":true}');
  }
  reply(404, "text/plain", "not found");
});
server.listen(Number(port), "127.0.0.1");
