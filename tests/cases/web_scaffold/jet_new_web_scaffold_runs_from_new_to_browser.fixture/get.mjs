// GET http://127.0.0.1:PORT/PATH (tests/web_scaffold.rs http_get): prints the status, then the body.
const [port, route] = process.argv.slice(2);
const response = await fetch(`http://127.0.0.1:${port}${route}`);
const body = await response.text();
console.log(`status ${response.status}`);
process.stdout.write(body);
process.exit(response.status === 200 ? 0 : 1);
