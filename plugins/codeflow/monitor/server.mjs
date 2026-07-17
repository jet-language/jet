#!/usr/bin/env node
import { createServer } from 'node:http';
import { existsSync } from 'node:fs';
import { readFile, readdir, realpath } from 'node:fs/promises';
import { dirname, extname, join, resolve, sep } from 'node:path';
import { hostname, networkInterfaces } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const PUBLIC = join(HERE, 'public');
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
};
const CSP = "default-src 'self'; img-src 'self' data:; style-src 'self' https://fonts.googleapis.com; script-src 'self'; connect-src 'self'; font-src 'self' https://fonts.gstatic.com; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const LEDGER_STALE_AFTER_MS = 30 * 60 * 1000;

export function parseArgs(args) {
  const config = { host: '0.0.0.0', port: 8899, workspace: process.cwd() };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--host') config.host = args[++index];
    else if (arg === '--port') config.port = Number(args[++index]);
    else if (arg === '--workspace') config.workspace = resolve(args[++index]);
    else throw new Error(`Unknown option: ${arg}`);
  }
  if (!config.host) throw new Error('--host needs a value');
  if (!Number.isInteger(config.port) || config.port < 0 || config.port > 65535) throw new Error('--port must be between 0 and 65535');
  return config;
}

function findWorkspace(start) {
  let current = resolve(start);
  while (true) {
    if (existsSync(join(current, '.codeflow'))) return current;
    const parent = dirname(current);
    if (parent === current) return resolve(start);
    current = parent;
  }
}

async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

async function loadReport(runDir, relative) {
  if (!relative) return null;
  const path = resolve(runDir, relative);
  if (path !== runDir && !path.startsWith(runDir + sep)) return { error: 'Report path leaves run directory.' };
  try {
    const [realRunDir, realReport] = await Promise.all([realpath(runDir), realpath(path)]);
    if (realReport !== realRunDir && !realReport.startsWith(realRunDir + sep)) return { error: 'Report path leaves run directory.' };
    return await readJson(realReport);
  } catch (error) {
    return { error: `Cannot read report: ${error.message}` };
  }
}

function countStatuses(nodes) {
  const counts = {};
  for (const node of Object.values(nodes || {})) counts[node.status] = (counts[node.status] || 0) + 1;
  return counts;
}

async function loadRun(runDir) {
  const state = await readJson(join(runDir, 'run.json'));
  const workflow = state.workflow || await readJson(join(runDir, 'workflow.json'));
  const reports = {};
  for (const [id, node] of Object.entries(state.nodes || {})) reports[id] = await loadReport(runDir, node.report);
  const counts = countStatuses(state.nodes);
  const total = Object.values(counts).reduce((sum, count) => sum + count, 0);
  const passed = counts.passed || 0;
  const ledgerAge = state.updated_at ? Math.max(0, Date.now() - new Date(state.updated_at).valueOf()) : null;
  return {
    ...state,
    workflow,
    reports,
    counts,
    progress: total ? passed / total : 0,
    ledger_age_ms: Number.isFinite(ledgerAge) ? ledgerAge : null,
    ledger_stale: !Number.isFinite(ledgerAge) || ledgerAge > LEDGER_STALE_AFTER_MS,
  };
}

async function loadState(workspace, requestedRun) {
  const runRoot = join(workspace, '.codeflow', 'runs');
  let entries = [];
  try {
    entries = (await readdir(runRoot, { withFileTypes: true })).filter(entry => entry.isDirectory());
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }

  const loaded = [];
  for (const entry of entries) {
    try {
      loaded.push(await loadRun(join(runRoot, entry.name)));
    } catch (error) {
      loaded.push({ run_id: entry.name, status: 'invalid', updated_at: null, error: error.message, counts: {}, progress: 0, ledger_age_ms: null, ledger_stale: true });
    }
  }
  loaded.sort((left, right) => String(right.updated_at || '').localeCompare(String(left.updated_at || '')));
  const summaries = loaded.map(run => ({
    run_id: run.run_id,
    status: run.status,
    goal: run.workflow?.goal || '',
    created_at: run.created_at || null,
    updated_at: run.updated_at || null,
    counts: run.counts,
    progress: run.progress,
    ledger_age_ms: run.ledger_age_ms,
    ledger_stale: run.ledger_stale,
    error: run.error,
  }));
  const selected = requestedRun ? loaded.find(run => run.run_id === requestedRun) : loaded.find(run => !run.error);
  if (requestedRun && !selected) {
    const error = new Error(`Unknown run: ${requestedRun}`);
    error.status = 404;
    throw error;
  }
  return {
    generated_at: new Date().toISOString(),
    workspace,
    run_root: runRoot,
    poll_interval_ms: 2000,
    runs: summaries,
    selected: selected?.error ? null : selected || null,
  };
}

function headers(type) {
  return {
    'content-type': type,
    'cache-control': 'no-store',
    'content-security-policy': CSP,
    'referrer-policy': 'no-referrer',
    'x-content-type-options': 'nosniff',
  };
}

function sendJson(res, status, value) {
  res.writeHead(status, headers('application/json; charset=utf-8'));
  res.end(JSON.stringify(value));
}

function allowedHosts(bindHost) {
  const hosts = new Set(['localhost', '127.0.0.1', '::1', hostname().toLowerCase()]);
  if (!['0.0.0.0', '::'].includes(bindHost)) hosts.add(bindHost.toLowerCase());
  for (const addresses of Object.values(networkInterfaces())) {
    for (const address of addresses || []) hosts.add(address.address.toLowerCase().split('%')[0]);
  }
  return hosts;
}

function requestHost(header) {
  try {
    return new URL(`http://${header}`).hostname.toLowerCase().replace(/^\[|\]$/g, '').split('%')[0];
  } catch {
    return '';
  }
}

async function staticFile(pathname, res) {
  const files = { '/': 'index.html', '/index.html': 'index.html', '/monitor.css': 'monitor.css', '/monitor.js': 'monitor.js', '/monitor-state.mjs': 'monitor-state.mjs' };
  const file = files[pathname];
  if (!file) return false;
  const path = join(PUBLIC, file);
  const body = await readFile(path);
  res.writeHead(200, headers(MIME[extname(path)] || 'application/octet-stream'));
  res.end(body);
  return true;
}

export function serve({ workspace = process.cwd(), host = '0.0.0.0', port = 8899 } = {}) {
  const root = findWorkspace(workspace);
  const hosts = allowedHosts(host);
  const server = createServer(async (req, res) => {
    try {
      if (!hosts.has(requestHost(req.headers.host))) return sendJson(res, 403, { error: 'Host not allowed.' });
      const url = new URL(req.url, 'http://codeflow.local');
      if (req.method !== 'GET') {
        res.writeHead(405, { ...headers('application/json; charset=utf-8'), allow: 'GET' });
        return res.end(JSON.stringify({ error: 'Method not allowed.' }));
      }
      if (url.pathname === '/api/state') return sendJson(res, 200, await loadState(root, url.searchParams.get('run')));
      if (url.pathname === '/api/health') return sendJson(res, 200, { ok: true, workspace: root });
      if (await staticFile(url.pathname, res)) return;
      sendJson(res, 404, { error: 'Not found.' });
    } catch (error) {
      sendJson(res, error.status || 500, { error: error.message });
    }
  });
  server.listen(port, host);
  return server;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const config = parseArgs(process.argv.slice(2));
    const server = serve(config);
    server.on('listening', () => {
      const address = server.address();
      const displayHost = config.host === '0.0.0.0' ? hostname() : config.host;
      console.log(`Codeflow monitor: http://${displayHost}:${address.port}`);
      if (config.host === '0.0.0.0') console.log(`Local fallback: http://127.0.0.1:${address.port}`);
      console.log(`Workspace: ${findWorkspace(config.workspace)}`);
    });
    server.on('error', error => {
      console.error(`codeflow monitor: ${error.code === 'EADDRINUSE' ? `port ${config.port} is already in use` : error.message}`);
      process.exitCode = 1;
    });
  } catch (error) {
    console.error(`codeflow monitor: ${error.message}`);
    process.exitCode = 1;
  }
}
