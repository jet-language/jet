#!/usr/bin/env node
// Protection-aware recovery. Only approved, idle bulk is eligible for removal.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

function absolute(name, value) {
  if (!value || !path.isAbsolute(value)) throw Error(`${name} must be an absolute path`);
  return path.resolve(value);
}
const C = absolute('JET_DEV_ROOT', process.env.JET_DEV_ROOT);
const R = fs.realpathSync(absolute('JET_REPO', process.env.JET_REPO));
const S = fs.realpathSync(absolute('JET_SCRATCH_ROOT', process.env.JET_SCRATCH_ROOT));
const ROOT = fs.realpathSync(absolute('JET_DISK_ROOT', process.env.JET_DISK_ROOT || '/'));
const Q = absolute('JET_PROOFQ_ROOT', process.env.JET_PROOFQ_ROOT);
const LSOF = process.env.JET_DISK_LSOF || 'lsof';
const PAUSE = path.join(Q, 'PAUSE');
const ALERT = path.join(C, 'DISK-ALERT');
const PROTECT = path.join(C, 'disk-protect.txt');
const SCRATCH_BUDGET = 350 * 1000 ** 3;
const args = process.argv.slice(2);
const pauseText = 'disk-guard recovery\n';

function command(bin, argv, timeout = 120000) {
  const result = spawnSync(bin, argv, { cwd: C, encoding: 'utf8', timeout, maxBuffer: 128 * 1024 * 1024 });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.error?.message || result.stderr || result.status}`);
  return result.stdout;
}
function dirs(p) {
  try { return fs.readdirSync(p, { withFileTypes: true }); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
}
function st(p) {
  try { return fs.lstatSync(p); }
  catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}
function beneath(p, base) { return p === base || p.startsWith(`${base}/`); }
function real(p) {
  let existing = path.resolve(p);
  while (!fs.existsSync(existing) && existing !== '/') existing = path.dirname(existing);
  return fs.realpathSync(existing);
}
function disk(p) {
  const row = command('df', ['-B1', '--output=size,used,avail,pcent,fstype', p]).trim().split('\n').at(-1).trim().split(/\s+/);
  const [size, used, available, percent, type] = row;
  const result = { size: Number(size), used: Number(used), available: Number(available), percent: Number(percent.replace('%', '')) };
  if (Object.values(result).some(value => !Number.isFinite(value)) || !type) throw Error(`invalid df output for ${p}`);
  return { ...result, type };
}
function scratchMounted() {
  return fs.statSync(S).dev !== fs.statSync(ROOT).dev && !['tmpfs', 'ramfs'].includes(disk(S).type);
}
function log(text) { fs.appendFileSync(path.join(C, 'disk-usage.log'), `${new Date().toISOString()} ${text}\n`); }
function alert(text) { fs.writeFileSync(ALERT, `${new Date().toISOString()} ${text}\n`); log(`ALERT ${text}`); }
function protectionLines() {
  // Missing or unreadable protection is an error, never permission to remove.
  return fs.readFileSync(PROTECT, 'utf8').split('\n').map(line => line.trim()).filter(line => line && !line.startsWith('#'));
}
function protectedItem(item) {
  const lines = protectionLines();
  const patterns = lines.map(line => new RegExp(`^${line.replace(/[.+^${}()|[\]\\]/g, '\\$&').replaceAll('*', '.*').replaceAll('?', '.')}(?:/.*)?$`));
  const explicit = lines.filter(line => !/[*?]/.test(line));
  // Protect ancestors of wildcard evidence too, even before that evidence exists.
  const globPrefixes = lines.filter(line => /[*?]/.test(line)).map(line => line.split(/[*?]/)[0]);
  const kept = path.join(C, 'disk-kept-binaries.tsv');
  if (fs.existsSync(kept)) {
    for (const line of fs.readFileSync(kept, 'utf8').trimEnd().split('\n').slice(1)) explicit.push(...line.split('\t').slice(0, 2));
  }
  const aliases = [item.path, item.old || item.path];
  if (item.old?.startsWith(`${C}/cand`)) aliases.push(path.join(S, 'candidates', path.basename(item.old)));
  if (item.old?.startsWith(`${C}/stage1/`)) aliases.push(path.join(S, 'stage1', path.basename(item.old)));
  return aliases.some(p => patterns.some(pattern => pattern.test(p))
    || explicit.some(child => beneath(child, p))
    || globPrefixes.some(prefix => prefix.startsWith(`${p}/`) || p.startsWith(prefix)));
}
function processSnapshot() {
  const open = command(LSOF, ['-nP', '-u', os.userInfo().username, '-F', 'n'], 60000)
    .split('\n').filter(line => line.startsWith('n/')).map(line => line.slice(1).replace(/ \(deleted\)$/, ''));
  const refs = [];
  for (const entry of dirs('/proc')) {
    if (!/^\d+$/.test(entry.name) || Number(entry.name) === process.pid) continue;
    const p = `/proc/${entry.name}`;
    try {
      if (fs.statSync(p).uid !== process.getuid()) continue;
      refs.push(fs.readFileSync(`${p}/cmdline`, 'utf8').replaceAll('\0', ' ') + ' ' + fs.readFileSync(`${p}/environ`, 'utf8').replaceAll('\0', ' '));
      try { open.push(fs.readlinkSync(`${p}/cwd`)); } catch { /* A process can exit while sampled. */ }
    } catch (error) {
      if (!['ENOENT', 'ESRCH'].includes(error.code)) throw error;
    }
  }
  // Queued and held scripts protect inputs, not just currently running jobs.
  for (const dir of ['running', 'queue']) {
    for (const entry of dirs(path.join(Q, dir))) {
      if (entry.isFile()) refs.push(fs.readFileSync(path.join(Q, dir, entry.name), 'utf8'));
    }
  }
  return { open, refs };
}
function idle(item) {
  const snapshot = processSnapshot();
  return !snapshot.open.some(p => beneath(p, item.path) || beneath(p, item.old || item.path))
    && !snapshot.refs.some(text => text.includes(item.path) || text.includes(item.old || item.path));
}
function metrics(p) { return Number(command('du', ['-s', '-B1', '--', p]).trim().split(/\s+/)[0]); }
function oldEnough(p, hours) { return command('find', [p, '-xdev', '-newermt', `${hours} hours ago`, '-print', '-quit']).trim() === ''; }
function table(name) {
  const p = path.join(C, name);
  if (!fs.existsSync(p)) return [];
  return fs.readFileSync(p, 'utf8').trimEnd().split('\n').slice(1).filter(Boolean).map(line => line.split('\t'));
}
function plan() {
  return table('DISK-PLAN.tsv').map(a => ({ path: a[0], bytes: Number(a[1]), class: a[2], safe: a[3] === 'yes', reason: a[4], mtime: Date.parse(a[5]) }));
}
function moves() {
  return table('disk-moves.tsv').map(a => ({ old: a[0], path: a[1], bytes: Number(a[2]), class: a[3], mtime: Date.parse(a[4]), safe: true }));
}
function authorized(item) {
  return item.safe && path.isAbsolute(item.path)
    && /^(candidate|stage1-run-fixedpoint|worktree-target|scratch-target|repo-root-target)$/.test(item.class);
}
function destination(item) {
  if (item.class === 'candidate') return path.join(S, 'candidates', path.basename(item.path));
  if (item.class === 'stage1-run-fixedpoint') return path.join(S, 'stage1', path.basename(item.path));
  const prefix = item.path.startsWith(`${R}/.agent-worktrees/`) ? 'worktree-' : 'cache-';
  return path.join(S, 'targets', prefix + path.relative(os.homedir(), item.path).replaceAll('/', '__'));
}
function savepointSummary(p) {
  const destRoot = path.join(C, 'stage1', path.basename(p));
  if (destRoot === p) return;
  function preserve(dir, relative = '') {
    for (const entry of dirs(dir)) {
      const source = path.join(dir, entry.name), rel = path.join(relative, entry.name);
      if (entry.isDirectory()) { preserve(source, rel); continue; }
      if (!entry.isFile() || !(entry.name === 'summary.txt' || /\.(receipt|json|md|tsv|rc)$/.test(entry.name) || entry.name.includes('receipt'))) continue;
      const dest = path.join(destRoot, rel);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.copyFileSync(source, dest);
    }
  }
  preserve(p);
}
function evict(item) {
  if (!authorized(item) || protectedItem(item) || !st(item.path)?.isDirectory() || !oldEnough(item.path, 6) || !idle(item)) return false;
  const bytes = metrics(item.path);
  if (item.class === 'stage1-run-fixedpoint') savepointSummary(item.path);
  if (item.old && st(item.old)?.isSymbolicLink() && fs.realpathSync(item.old) === item.path) fs.unlinkSync(item.old);
  fs.rmSync(item.path, { recursive: true });
  log(`EVICT ${item.path} bytes=${bytes}`);
  return true;
}
function scratchEviction() {
  if (!scratchMounted()) { alert('scratch filesystem not mounted; main recovery attempts only approved idle-target deletion'); return; }
  let usage = disk(S);
  if (usage.percent < 75 && usage.used < SCRATCH_BUDGET) return;
  // Never infer approval merely from a directory's name or location.
  const known = new Map([...moves(), ...plan()].filter(authorized).map(item => [item.path, item]));
  const items = [...known.values()].filter(item => beneath(item.path, S) && st(item.path)?.isDirectory() && !protectedItem(item)).sort((a, b) => a.mtime - b.mtime);
  for (const item of items) {
    evict(item);
    usage = disk(S);
    if (usage.percent < 75 && usage.used < SCRATCH_BUDGET) break;
  }
  if (usage.percent >= 75 || usage.used >= SCRATCH_BUDGET) alert('scratch remains over 75%/350GB budget; no approved idle bulk available');
}
function move(item) {
  if (!scratchMounted() || !authorized(item) || protectedItem(item) || !st(item.path)?.isDirectory() || !oldEnough(item.path, 3) || !idle(item)) return false;
  const dest = destination(item), bytes = metrics(item.path), usage = disk(S);
  if (st(dest) || usage.used + bytes > (usage.used + usage.available) * 0.75 || usage.used + bytes > SCRATCH_BUDGET) return false;
  if (fs.statSync(item.path).dev !== fs.statSync(ROOT).dev) return false;
  const modified = st(item.path).mtime.toISOString();
  const partial = `${dest}.moving-${process.pid}`;
  if (st(partial)) throw Error(`existing partial move ${partial}`);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  command('cp', ['-a', '--reflink=auto', '--', item.path, partial], 3600000);
  // A job may have started during the copy; recheck before replacing source.
  if (protectedItem(item) || !idle(item) || !oldEnough(item.path, 3)) {
    fs.rmSync(partial, { recursive: true });
    log(`CANCEL MOVE now active ${item.path}`);
    return false;
  }
  fs.renameSync(partial, dest);
  fs.rmSync(item.path, { recursive: true });
  fs.symlinkSync(dest, item.path, 'dir');
  const ledger = path.join(C, 'disk-moves.tsv');
  if (!fs.existsSync(ledger)) fs.writeFileSync(ledger, 'old_path\tnew_path\tsize_bytes\tclass\tlast_modified\tmoved_at\n');
  fs.appendFileSync(ledger, [item.path, dest, bytes, item.class, modified, new Date().toISOString()].join('\t') + '\n');
  log(`MOVE ${item.path} -> ${dest} bytes=${bytes}`);
  return true;
}
function deleteStoppedTarget(item) {
  if (!['worktree-target', 'scratch-target', 'repo-root-target'].includes(item.class) || !authorized(item) || protectedItem(item) || !st(item.path)?.isDirectory() || !idle(item)) return false;
  if (item.class === 'worktree-target') {
    const checkout = path.dirname(item.path);
    const result = spawnSync('git', ['-C', checkout, 'merge-base', '--is-ancestor', 'HEAD', 'dev'], { cwd: C, timeout: 30000 });
    if (result.status !== 0) return false;
  }
  const bytes = metrics(item.path);
  fs.rmSync(item.path, { recursive: true });
  log(`DELETE IDLE TARGET ${item.path} bytes=${bytes}`);
  return true;
}

if (args[0] === '--check') {
  const gib = Number(args[1]), output = args[2];
  if (args.length !== 3 || !Number.isFinite(gib) || gib <= 0 || !output) throw Error('usage: disk-guard.sh --check EXPECTED_GIB OUTPUT_PATH');
  const resolved = real(output), usage = disk(resolved), scratch = beneath(resolved, S);
  const limit = scratch ? 75 : 85, expected = gib * 1024 ** 3;
  const evidence = JSON.stringify({ resolved, expected, limit, ...usage });
  if (['tmpfs', 'ramfs'].includes(usage.type) || (scratch && !scratchMounted()) || fs.existsSync(PAUSE) || usage.available < expected
    || 100 * (usage.used + expected) / (usage.used + usage.available) >= limit
    || (scratch && usage.used + expected > SCRATCH_BUDGET)) {
    console.error(`DISK GATE: insufficient headroom/paused/unmounted; ${evidence}`);
    process.exit(75);
  }
  console.log(`DISK GATE OK ${evidence}`);
  process.exit(0);
}
if (args.length && !(args.length === 1 && args[0] === '--once')) throw Error('usage: disk-guard.sh [--once | --check EXPECTED_GIB OUTPUT_PATH]');
for (const dir of ['candidates', 'stage1', 'targets', 'sccache']) {
  if (!scratchMounted()) break;
  fs.mkdirSync(path.join(S, dir), { recursive: true });
}
function pause() {
  try { fs.writeFileSync(PAUSE, pauseText, { flag: 'wx' }); }
  catch (error) { if (error.code !== 'EEXIST') throw error; }
}
function ownsPause() { return fs.existsSync(PAUSE) && fs.readFileSync(PAUSE, 'utf8') === pauseText; }
async function tick() {
  const main = disk(ROOT), scratch = disk(S);
  log(command('df', ['-h', ROOT, S]).trim().replaceAll('\n', ' | '));
  if (main.percent >= 85) { alert('main >=85%: proof queue paused; recovering toward <80%'); pause(); }
  if (scratch.percent >= 90) alert('scratch >=90%: only approved safe eviction allowed');
  scratchEviction();
  if (ownsPause()) {
    const items = plan().filter(item => authorized(item) && st(item.path)?.isDirectory() && !protectedItem(item)).sort((a, b) => a.mtime - b.mtime);
    for (const item of items) {
      if (disk(ROOT).percent < 80) break;
      if (!move(item)) deleteStoppedTarget(item);
    }
    if (disk(ROOT).percent < 80) {
      if (ownsPause()) fs.unlinkSync(PAUSE);
      log('main <80%; disk-guard pause released');
    } else alert('main still >=80%: no safe idle bulk; queue remains paused; Main must extend protection-aware plan');
  }
}
while (true) {
  try { await tick(); }
  catch (error) {
    alert(`guard failed closed: ${error.message}`);
    pause();
    if (args[0] === '--once') process.exitCode = 1;
  }
  if (args[0] === '--once') break;
  await delay(600000);
}
