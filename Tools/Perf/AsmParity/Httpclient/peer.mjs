#!/usr/bin/env node
// The httpclient cell's peer: the gauntlet entry's fixtures/peer.py, byte
// for byte in its responses, as a node script (asm-parity serves a cell with
// `node script port delay_ms`; the delay is unused).
import { createServer } from 'node:http';

const ITEMS = [
  { id: 1, name: 'alpha', qty: 3 },
  { id: 2, name: 'beta', qty: 7 },
  { id: 3, name: 'gamma', qty: 2 },
  { id: 4, name: 'delta', qty: 5 },
  { id: 5, name: 'epsilon', qty: 4 },
  { id: 6, name: 'zeta', qty: 6 },
];

createServer((request, response) => {
  const path = new URL(request.url, 'http://peer').pathname;
  let status = 404;
  let payload = { error: 'not found' };
  if (path === '/items') {
    status = 200;
    payload = { items: ITEMS };
  } else if (path.startsWith('/items/')) {
    const raw = path.slice(path.lastIndexOf('/') + 1).trim();
    const id = /^[-+]?\d+$/.test(raw) ? Number(raw) : -1;
    const item = ITEMS.find((candidate) => candidate.id === id);
    if (item) {
      status = 200;
      payload = item;
    }
  }
  const body = Buffer.from(JSON.stringify(payload), 'ascii');
  response.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': body.length, Connection: 'close' });
  response.end(body);
}).listen(Number(process.argv[2]), '127.0.0.1');
