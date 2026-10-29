// Mock WebDriver BiDi endpoint for tests/cases/browser_bidi. Port of the std-only mock servers that
// tests/browser_bidi.rs ran on a thread: one WebSocket client, text frames, and the method of every
// request printed to stdout, one per line, in arrival order. Any protocol surprise prints to stderr
// and exits 1, the way the Rust server thread panicked and failed `server.join().unwrap()`.
//
// node browser_bidi_mock.mjs <mode> <port> [spec.json]
//   script      replies from spec.json (see runScript)
//   handshake   run_handshake: spec.json {"status": RESULT, "expect_session": BOOL}
//   malformed-session, event-storm, continuous-events
//   hostile-malformed, hostile-mismatched-id, hostile-protocol-error, hostile-timeout, hostile-no-cdp
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:net';

const [mode, portText, specPath] = process.argv.slice(2);
const spec = specPath ? JSON.parse(readFileSync(specPath, 'utf8')) : {};

function fail(message) {
  process.stderr.write(`browser_bidi_mock: ${message}\n`);
  process.exit(1);
}

// The Rust tests had no deadline of their own; a client that never connects or stalls ends the case.
// Cases start the mock after `jet build`, so the deadline covers only the run.
setTimeout(() => fail('deadline: the client never finished the exchange'), 30_000).unref();

class Peer {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.waiting = null;
    this.ended = false;
    socket.on('data', (chunk) => { this.buffer = Buffer.concat([this.buffer, chunk]); this.wake(); });
    socket.on('end', () => { this.ended = true; this.wake(); });
    socket.on('error', () => { this.ended = true; this.wake(); });
  }
  wake() { const w = this.waiting; this.waiting = null; if (w) w(); }
  async take(n) {
    while (this.buffer.length < n) {
      if (this.ended) fail(`client closed while ${n} bytes were expected`);
      await new Promise((resolve) => { this.waiting = resolve; });
    }
    const out = this.buffer.subarray(0, n);
    this.buffer = this.buffer.subarray(n);
    return out;
  }
  // Port of accept_websocket.
  async accept() {
    while (!this.buffer.includes('\r\n\r\n')) {
      if (this.ended) fail('client closed during handshake');
      if (this.buffer.length > 16 * 1024) fail('oversized handshake');
      await new Promise((resolve) => { this.waiting = resolve; });
    }
    const end = this.buffer.indexOf('\r\n\r\n') + 4;
    const request = this.buffer.subarray(0, end).toString('latin1');
    this.buffer = this.buffer.subarray(end);
    const keyLine = request.split('\r\n').find((line) => line.split(':')[0].trim().toLowerCase() === 'sec-websocket-key');
    if (!keyLine) fail('websocket key');
    const key = keyLine.slice(keyLine.indexOf(':') + 1).trim();
    const accept = createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64');
    this.socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  }
  // Port of read_text_frame.
  async read() {
    const head = await this.take(2);
    if ((head[0] & 0x0f) !== 1) fail('expected text frame');
    if ((head[1] & 0x80) === 0) fail('client frame must be masked');
    let len = head[1] & 0x7f;
    if (len === 126) len = (await this.take(2)).readUInt16BE(0);
    else if (len === 127) len = Number((await this.take(8)).readBigUInt64BE(0));
    const mask = await this.take(4);
    const payload = Buffer.from(await this.take(len));
    for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
    return payload.toString('utf8');
  }
  // Port of write_text_frame.
  write(text) {
    const bytes = Buffer.from(text, 'utf8');
    let head;
    if (bytes.length <= 125) head = Buffer.from([0x81, bytes.length]);
    else if (bytes.length <= 65535) { head = Buffer.from([0x81, 126, 0, 0]); head.writeUInt16BE(bytes.length, 2); }
    else fail('mock response too large');
    this.socket.write(Buffer.concat([head, bytes]));
  }
  // Data pending within `ms`, the way run_handshake peeked with a 100ms read timeout.
  async pending(ms) {
    if (this.buffer.length > 0) return true;
    await new Promise((resolve) => { this.waiting = resolve; setTimeout(resolve, ms); });
    return this.buffer.length > 0;
  }
}

// Port of field: the string or digit run after `"name":`.
function field(text, name) {
  const marker = `"${name}":`;
  const at = text.indexOf(marker);
  if (at < 0) fail(`missing ${name}: ${text}`);
  const rest = text.slice(at + marker.length);
  if (rest.startsWith('"')) return rest.slice(1, rest.indexOf('"', 1));
  return rest.match(/^[0-9]*/)[0];
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const success = (id, result) => `{"type":"success","id":${id},"result":${result}}`;
const record = (method) => process.stdout.write(`${method}\n`);

// spec: {"max": N, "stop": METHOD?, "replies": {METHOD: REPLY}, "after": {METHOD or METHOD#K: [FRAME...]},
//        "require": {METHOD: [SUBSTRING...]}, "forbid": {METHOD: [SUBSTRING...]}}
// REPLY is a result JSON string, {"frame": TEMPLATE} for a whole frame, {"if_contains": S, "then": REPLY,
// "else": REPLY}, or a list indexed by the method's 1-based occurrence (the last entry repeats).
// `{id}` names the request id and `{n}` the occurrence in results and frames. require/forbid are the
// request assertions the Rust server made before replying.
async function runScript(peer) {
  const seen = new Map();
  for (let i = 0; i < spec.max; i++) {
    const request = await peer.read();
    const id = field(request, 'id');
    const method = field(request, 'method');
    record(method);
    const n = (seen.get(method) ?? 0) + 1;
    seen.set(method, n);
    let reply = spec.replies[method];
    if (reply === undefined) fail(`unexpected BiDi method ${method}: ${request}`);
    for (const text of spec.require?.[method] ?? []) if (!request.includes(text)) fail(`${method} request lacks ${text}: ${request}`);
    for (const text of spec.forbid?.[method] ?? []) if (request.includes(text)) fail(`${method} request carries ${text}: ${request}`);
    for (;;) {
      if (Array.isArray(reply)) reply = reply[Math.min(n, reply.length) - 1];
      else if (reply !== null && typeof reply === 'object' && 'if_contains' in reply) reply = request.includes(reply.if_contains) ? reply.then : reply.else;
      else break;
    }
    const fill = (text) => text.replaceAll('{id}', id).replaceAll('{n}', String(n));
    peer.write(typeof reply === 'string' ? success(id, fill(reply)) : fill(reply.frame));
    for (const frame of [...(spec.after?.[method] ?? []), ...(spec.after?.[`${method}#${n}`] ?? [])]) peer.write(frame);
    if (method === spec.stop) break;
  }
}

const READY = '{"ready":true,"message":"ready"}';

const modes = {
  script: runScript,
  // Port of run_handshake.
  async handshake(peer) {
    const request = await peer.read();
    record(field(request, 'method'));
    peer.write(success(field(request, 'id'), spec.status));
    if (spec.expect_session) {
      const session = await peer.read();
      record(field(session, 'method'));
      peer.write(success(field(session, 'id'), '{"sessionId":"codec","capabilities":{}}'));
      const end = await peer.read();
      record(field(end, 'method'));
      peer.write(success(field(end, 'id'), '{}'));
    } else if (await peer.pending(100)) {
      record(field(await peer.read(), 'method'));
    }
  },
  // Port of run_malformed_session.
  async 'malformed-session'(peer) {
    for (const result of [READY, '{"sessionId":1,"capabilities":[]}', '{}']) {
      const request = await peer.read();
      record(field(request, 'method'));
      peer.write(success(field(request, 'id'), result));
    }
  },
  // Port of run_event_storm.
  async 'event-storm'(peer) {
    for (let command = 0; command < 4; command++) {
      const request = await peer.read();
      const id = field(request, 'id');
      record(field(request, 'method'));
      if (command === 2) {
        for (let index = 0; index < 300; index++) {
          const method = index === 100 ? `SECRET_REMOTE_METHOD_${'x'.repeat(4000)}` : `event.${index}`;
          peer.write(`{"type":"event","method":"${method}","params":{}}`);
        }
      }
      peer.write(success(id, [READY, '{"sessionId":"storm","capabilities":{}}', READY, '{}'][command]));
    }
  },
  // Port of run_continuous_events.
  async 'continuous-events'(peer) {
    for (const result of [READY, '{"sessionId":"deadline","capabilities":{}}']) {
      const request = await peer.read();
      record(field(request, 'method'));
      peer.write(success(field(request, 'id'), result));
    }
    record(field(await peer.read(), 'method'));
    for (let index = 0; index < 60; index++) {
      peer.write(`{"type":"event","method":"network.tick${index}","params":{}}`);
      await sleep(5);
    }
    const request = await peer.read();
    record(field(request, 'method'));
    peer.write(success(field(request, 'id'), '{}'));
  },
};

// Port of run_hostile and its HostileReply arms.
const hostile = {
  'hostile-malformed': (peer) => peer.write('{not-json'),
  'hostile-mismatched-id': (peer, id) => peer.write(`{"type":"success","id":${Number(id) + 1},"result":{}}`),
  'hostile-protocol-error': (peer, id) => peer.write(`{"type":"error","id":${id},"error":"invalid argument","message":"SECRET SERVER DATA","stacktrace":"SECRET STACK"}`),
  'hostile-timeout': () => sleep(500),
  async 'hostile-no-cdp'(peer, id) {
    peer.write(success(id, READY));
    const request = await peer.read();
    if (field(request, 'method') !== 'session.new') fail(`expected session.new: ${request}`);
    if (!request.includes('"capabilities":{"alwaysMatch":{}}')) fail(`session.new without empty alwaysMatch: ${request}`);
    peer.write(success(field(request, 'id'), '{"sessionId":"no-cdp","capabilities":{"goog:cdp":false}}'));
  },
};
for (const [name, reply] of Object.entries(hostile)) {
  modes[name] = async (peer) => {
    const request = await peer.read();
    record(field(request, 'method'));
    await reply(peer, field(request, 'id'));
  };
}

const run = modes[mode] ?? fail(`unknown mode ${mode}`);
const server = createServer({ allowHalfOpen: false }, async (socket) => {
  server.close();
  const peer = new Peer(socket);
  await peer.accept();
  await run(peer);
  socket.end(() => process.exit(0));
});
server.listen(Number(portText), '127.0.0.1');
