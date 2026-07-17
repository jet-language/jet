import { after, before, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { request } from 'node:http';
import { parseArgs, serve } from '../monitor/server.mjs';
import { createRequestGate, selectedSummary } from '../monitor/public/monitor-state.mjs';

const workspace = mkdtempSync(join(tmpdir(), 'codeflow-monitor-'));
const runDir = join(workspace, '.codeflow', 'runs', 'sample-run');
mkdirSync(join(runDir, 'results'), { recursive: true });
const invalidRunDir = join(workspace, '.codeflow', 'runs', 'broken-run');
mkdirSync(invalidRunDir, { recursive: true });
writeFileSync(join(invalidRunDir, 'run.json'), '{broken');

const workflow = {
  schema_version: 1,
  goal: 'Ship a monitored workflow.',
  workspace,
  limits: { max_parallel: 2, max_attempts: 3, max_cycles: 4 },
  acceptance: ['Monitor shows proof.'],
  nodes: [
    {
      id: 'inspect', kind: 'investigate', mode: 'read', objective: 'Map the work.',
      depends_on: [], paths: ['src'], acceptance: ['Map exists.'],
      forbidden: ['edit files'], fresh_context: false,
    },
    {
      id: 'build', kind: 'implement', mode: 'write', objective: 'Build the feature.',
      depends_on: ['inspect'], paths: ['src'], write_scope: ['src'],
      acceptance: ['Feature works.'], forbidden: ['touch .codeflow'], fresh_context: false,
    },
  ],
};

writeFileSync(join(runDir, 'results', 'inspect-execution-1.json'), JSON.stringify({
  status: 'passed', summary: 'Mapped work.', acceptance: ['Map exists.'],
  evidence: ['src inspected'], artifacts: ['src'], checks: [{ command: 'test', status: 'passed', detail: 'green' }],
  changed_paths: [], findings: ['One seam'], risks: [], next: ['Build'],
}));
writeFileSync(join(runDir, 'run.json'), JSON.stringify({
  schema_version: 1, run_id: 'sample-run', status: 'active', created_at: '2026-07-16T12:00:00Z',
  updated_at: '2026-07-16T12:01:00Z', cycles: 1, reviewer_worker: null, workflow,
  events: [{ at: '2026-07-16T12:01:00Z', event: 'node-passed', node: 'inspect', detail: 'Mapped work.' }],
  nodes: {
    inspect: { status: 'passed', attempts: 1, sequence: 1, worker: '/root/scout', started_at: '2026-07-16T12:00:00Z', finished_at: '2026-07-16T12:01:00Z', report: 'results/inspect-execution-1.json', artifact_digests: { src: 'abc' }, acceptance: ['Map exists.'] },
    build: { status: 'ready', attempts: 0, sequence: 0, worker: null, started_at: null, finished_at: null, report: '../../outside.json', artifact_digests: {}, acceptance: [] },
  },
}));

let server;
let base;

before(async () => {
  server = serve({ workspace, host: '127.0.0.1', port: 0 });
  await new Promise((resolve, reject) => {
    server.once('listening', resolve);
    server.once('error', reject);
  });
  base = `http://127.0.0.1:${server.address().port}`;
});

after(() => server.close());

test('defaults to trusted-network port 8899 for mobile access', () => {
  assert.deepEqual(parseArgs([]), { host: '0.0.0.0', port: 8899, workspace: process.cwd() });
});

test('client keeps the newest response and checks the current run summary', async () => {
  const gate = createRequestGate();
  const applied = [];
  let releaseOld;
  let releaseNew;
  const oldResponse = new Promise(resolve => { releaseOld = resolve; });
  const newResponse = new Promise(resolve => { releaseNew = resolve; });
  const apply = async response => {
    const request = gate.begin();
    const value = await response;
    if (gate.isCurrent(request)) applied.push(value);
  };
  const old = apply(oldResponse);
  const fresh = apply(newResponse);
  releaseNew('fresh');
  await fresh;
  releaseOld('stale');
  await old;
  assert.deepEqual(applied, ['fresh']);

  const data = { selected: { run_id: 'stale', ledger_stale: true }, runs: [
    { run_id: 'stale', ledger_stale: true },
    { run_id: 'fresh', ledger_stale: false },
  ] };
  assert.equal(selectedSummary(data, 'fresh').ledger_stale, false);
});

test('serves projected run, graph, reports, and exposure data', async () => {
  const response = await fetch(`${base}/api/state`);
  assert.equal(response.status, 200);
  const state = await response.json();
  assert.equal(state.selected.run_id, 'sample-run');
  assert.deepEqual(state.selected.counts, { passed: 1, ready: 1 });
  assert.equal(state.selected.ledger_stale, true);
  assert.equal(state.runs[0].ledger_stale, true);
  assert.equal(state.runs.find(run => run.run_id === 'broken-run').ledger_stale, true);
  assert.equal(state.selected.reports.inspect.summary, 'Mapped work.');
  assert.equal(state.selected.reports.build.error, 'Report path leaves run directory.');
  assert.equal(state.selected.workflow.nodes[1].write_scope[0], 'src');
});

test('serves UI with restrictive browser policy', async () => {
  const response = await fetch(base);
  assert.equal(response.status, 200);
  assert.match(response.headers.get('content-security-policy'), /default-src 'self'/);
  const body = await response.text();
  assert.match(body, /Codeflow flight recorder/i);
  assert.match(body, /role="status" aria-live="polite"/);
  const module = await fetch(`${base}/monitor-state.mjs`);
  assert.match(module.headers.get('content-type'), /^text\/javascript/);
});

test('keeps an empty workspace distinct from a live ledger', async t => {
  const empty = mkdtempSync(join(tmpdir(), 'codeflow-monitor-empty-'));
  const emptyServer = serve({ workspace: empty, host: '127.0.0.1', port: 0 });
  await new Promise((resolve, reject) => {
    emptyServer.once('listening', resolve);
    emptyServer.once('error', reject);
  });
  t.after(() => emptyServer.close());
  const response = await fetch(`http://127.0.0.1:${emptyServer.address().port}/api/state`);
  const state = await response.json();
  assert.equal(state.selected, null);
  assert.deepEqual(state.runs, []);
});

test('rejects unknown routes and non-GET methods', async () => {
  assert.equal((await fetch(`${base}/missing`)).status, 404);
  assert.equal((await fetch(`${base}/api/state?run=missing`)).status, 404);
  assert.equal((await fetch(`${base}/api/state`, { method: 'POST' })).status, 405);
});

test('rejects hostile Host headers to block DNS rebinding', async () => {
  const url = new URL(base);
  const status = await new Promise((resolve, reject) => {
    const req = request({ hostname: url.hostname, port: url.port, path: '/api/state', headers: { Host: 'hostile.example' } }, response => {
      response.resume();
      resolve(response.statusCode);
    });
    req.on('error', reject);
    req.end();
  });
  assert.equal(status, 403);
});
