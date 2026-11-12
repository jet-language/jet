// The gauntlet driver of the HTTP probe exports of
// Tools/gauntlet/harness/run.mjs (#5072 c8): httpProbe and probeMatches.
// Tools/gauntlet/entries/run-probe/driver/run.jet is the twin on the kit's
// Probe module.
//
//   node run-probe.mjs matches FILE
//   node run-probe.mjs probe FILE
//
// `matches` judges each case of the JSON array in FILE, a `probe`, the
// `result` it got and the service's `protocol`, and shows its name and
// whether the result matches. `probe` serves the `routes` of the JSON object
// in FILE on a free local port, one connection at a time, each the raw
// response bytes of its `raw` (after `delayMs`), the request's method,
// target, body length and body for `echo`, nothing for `drop` and a 404 for
// an unknown target; it then sends each of its `probes` (a `name`, the
// `probe` and its `timeoutMs`), stops serving and sends each of its
// `refused` probes to the closed port. Each probe shows its name, what it
// answered (`ok`, then the status, headers and body or the error, with the
// port as PORT, and whether a latency was measured) and whether that
// matches the probe.
import fs from 'node:fs';
import net from 'node:net';
import { httpProbe, probeMatches } from '../run.mjs';

const usage = 'usage: node run-probe.mjs matches FILE | probe FILE';
const [command, file] = process.argv.slice(2);

const NOT_FOUND = 'HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n';

// The request in `bytes` once its head and body are complete.
function request(bytes) {
  const end = bytes.indexOf('\r\n\r\n');
  if (end < 0) return null;
  const head = bytes.subarray(0, end).toString('utf8').split('\r\n');
  const length = head.slice(1).map((line) => line.split(':')).find(([name]) => name.trim().toLowerCase() === 'content-length');
  const want = length ? Number(length.slice(1).join(':').trim()) : 0;
  if (bytes.length < end + 4 + want) return null;
  const [method, target] = head[0].split(' ');
  return { method, target, body: bytes.subarray(end + 4, end + 4 + want).toString('utf8') };
}

// The raw response `route` gives `asked`.
function response(route, asked) {
  if (!route) return NOT_FOUND;
  if (!route.echo) return route.raw;
  const text = `${asked.method} ${asked.target} ${Buffer.byteLength(asked.body)} ${asked.body}`;
  return `HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: ${Buffer.byteLength(text)}\r\nConnection: close\r\n\r\n${text}`;
}

function serve(routes) {
  const server = net.createServer((socket) => {
    let bytes = Buffer.alloc(0);
    socket.on('error', () => {});
    socket.on('data', (chunk) => {
      bytes = Buffer.concat([bytes, chunk]);
      const asked = request(bytes);
      if (!asked) return;
      const route = Object.hasOwn(routes, asked.target) ? routes[asked.target] : null;
      if (route?.drop) return socket.destroy();
      setTimeout(() => socket.end(response(route, asked)), route?.delayMs ?? 0);
    });
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// What a probe answered, as the driver shows it.
function shown(result, port) {
  const out = { ok: result.ok };
  for (const key of ['status', 'headers', 'body']) if (key in result) out[key] = result[key];
  if ('error' in result) out.error = String(result.error).replaceAll(String(port), 'PORT');
  out.latency = typeof result.latencyMs === 'number' && result.latencyMs >= 0;
  return out;
}

async function probeAll(spec) {
  const server = await serve(spec.routes ?? {});
  const { port } = server.address();
  for (const { name, probe, timeoutMs } of spec.probes ?? []) {
    const result = await httpProbe(port, probe, timeoutMs);
    console.log(`${name}\t${JSON.stringify(shown(result, port))}\t${probeMatches(probe, result, 'http')}`);
  }
  server.close();
  for (const { name, probe } of spec.refused ?? []) {
    const result = await httpProbe(port, probe);
    console.log(`${name}\t${JSON.stringify(shown(result, port))}\t${probeMatches(probe, result, 'http')}`);
  }
}

if (command === 'matches' && file) {
  for (const { name, probe, result, protocol } of JSON.parse(fs.readFileSync(file, 'utf8'))) {
    console.log(`${name}\t${probeMatches(probe, result, protocol)}`);
  }
} else if (command === 'probe' && file) {
  await probeAll(JSON.parse(fs.readFileSync(file, 'utf8')));
} else {
  console.error(usage);
  process.exit(64);
}
