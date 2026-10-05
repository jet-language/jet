#!/usr/bin/env node
// Scheduling is serialized across the two services; proof jobs are not.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

const root = path.resolve(process.env.JET_PROOFQ_ROOT);
const mainDisk = process.env.JET_DISK_ROOT || '/';
const lane = Number(process.argv[3]);
if (![1, 2].includes(lane) || !['--lane', '--claim'].includes(process.argv[2])) {
  throw Error('usage: proof-queue.mjs --lane 1|2 (use proof-queue.sh)');
}
for (const dir of ['queue', 'running', 'done']) fs.mkdirSync(path.join(root, dir), { recursive: true });
const script = fileURLToPath(import.meta.url);
const runningDir = path.join(root, 'running');
const measurement = name => /^(Perf|SP0)/.test(name);
const log = text => fs.appendFileSync(path.join(root, 'runner.log'), `${new Date().toISOString()} lane${lane} ${text}\n`);

function fixedPointActive() {
  for (const pid of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(pid)) continue;
    let cmd;
    try { cmd = fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8'); } catch { continue; }
    if (cmd.includes('stage1/fixedpoint.sh')) return true;
  }
  return false;
}

function claim() {
  if (fs.existsSync(path.join(root, 'PAUSE'))) return;
  const df = spawnSync('df', ['--output=pcent', mainDisk], { encoding: 'utf8', timeout: 30000 });
  if (df.error || df.status !== 0) throw Error(`disk gate: ${df.error?.message || df.stderr}`);
  const percent = Number(df.stdout.trim().split('\n').at(-1).trim().replace('%', ''));
  if (!Number.isFinite(percent)) throw Error('invalid disk usage');
  if (percent >= 88) return;
  const mem = fs.readFileSync('/proc/meminfo', 'utf8').match(/^MemAvailable:\s+(\d+)\s+kB$/m);
  if (!mem) throw Error('MemAvailable unavailable');
  const available = Number(mem[1]) / 1048576;
  const fixed = fixedPointActive();
  const need = lane === 1 ? (fixed ? 45 : 30) : (fixed ? 55 : 40);
  if (available < need) return;
  const running = fs.readdirSync(runningDir).filter(name => name.endsWith('.sh'));
  if (running.length >= 2) return;
  if (lane === 2 && (!running.length || running.some(name => Date.now() - fs.statSync(path.join(runningDir, name)).ctimeMs < 600000))) return;
  const jobs = fs.readdirSync(path.join(root, 'queue'))
    .filter(name => name.endsWith('.sh') && (lane === 1 || !measurement(name)))
    .map(name => ({ name, stat: fs.statSync(path.join(root, 'queue', name)) }))
    .filter(job => job.stat.isFile())
    .sort((a, b) => a.stat.mtimeMs - b.stat.mtimeMs || a.name.localeCompare(b.name));
  const job = jobs[0];
  if (!job || (lane === 1 && measurement(job.name) && running.length)) return;
  const name = job.name.slice(0, -3);
  if (fs.existsSync(path.join(runningDir, job.name)) || fs.existsSync(path.join(root, 'done', `${name}.rc`))) {
    throw Error(`duplicate proof name ${name}; choose a unique attempt name`);
  }
  fs.renameSync(path.join(root, 'queue', job.name), path.join(runningDir, job.name));
  // Rename updates ctime: age measures claiming, not the priority mtime.
  log(`start ${name} (MemAvailable ${available.toFixed(1)} GiB, need ${need})`);
  process.stdout.write(name);
}

if (process.argv[2] === '--claim') {
  claim();
} else {
  let stopping = false;
  let child;
  for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => {
    stopping = true;
    if (child) {
      try { process.kill(-child.pid, signal); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
  });
  while (!stopping) {
    const result = spawnSync('flock', ['-o', path.join(root, 'claim.lock'), process.execPath, script, '--claim', String(lane)], {
      encoding: 'utf8', timeout: 120000,
    });
    if (result.error || result.status !== 0) {
      log(`claim failed: ${result.error?.message || result.stderr || result.status}`);
      await delay(60000);
      continue;
    }
    const name = result.stdout;
    if (!name) { await delay(30000); continue; }
    const logFile = fs.openSync(path.join(root, 'done', `${name}.log`), 'w');
    let rc;
    try {
      rc = await new Promise(resolve => {
        child = spawn('timeout', ['3600', 'bash', path.join(runningDir, `${name}.sh`)], {
          cwd: process.env.HOME, detached: true, stdio: ['ignore', logFile, logFile],
        });
        child.once('error', error => { fs.writeSync(logFile, `${error.message}\n`); resolve(127); });
        child.once('close', (code, signal) => resolve(code ?? (signal === 'SIGINT' ? 130 : 143)));
      });
    } finally {
      child = undefined;
      fs.closeSync(logFile);
    }
    fs.writeFileSync(path.join(root, 'done', `${name}.rc`), `${rc}\n`);
    fs.renameSync(path.join(runningDir, `${name}.sh`), path.join(root, 'done', `${name}.sh`));
    log(`done ${name} rc=${rc}`);
  }
}
