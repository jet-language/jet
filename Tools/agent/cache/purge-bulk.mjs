// Dry-run by default. Delete only regenerable, inactive output older than six hours.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
const ROOT = path.join(process.env.HOME, '.cache/jet-dev');
const apply = process.argv.includes('--apply');
const KEEP = new Set(['Stage1Driver', 'SpeedHost', 'FixL5-KeepGoing', 'EndToEndQA']);
for (const arg of process.argv.slice(2)) if (arg.startsWith('--keep=')) KEEP.add(arg.slice(7));
const cutoff = Date.now() - 6 * 3600e3;
function liveReferences() {
  const refs = [];
  for (const pid of fs.readdirSync('/proc').filter(p => /^\d+$/.test(p))) {
    const root = `/proc/${pid}`;
    try { refs.push(fs.readFileSync(`${root}/cmdline`, 'utf8').replaceAll('\0', ' ')); } catch {}
    try { refs.push(fs.readFileSync(`${root}/maps`, 'utf8')); } catch {}
    const links = ['cwd', 'exe'];
    try { for (const fd of fs.readdirSync(`${root}/fd`)) links.push(`fd/${fd}`); } catch {}
    for (const link of links) try { refs.push(fs.readlinkSync(`${root}/${link}`)); } catch {}
  }
  return refs;
}
const inUse = (p, refs) => refs.some(r => r.includes(p) || r.includes(p.replace('/jet-dev/', '/jet-luna/')) || r.includes(p.replace('/jet-dev/scratch/', '/jet-test-scratch/')));
const files = [], trees = [];
function bulk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) bulk(p);
    else if (e.isFile()) {
      const st = fs.statSync(p);
      if (st.size > 20 * 2 ** 20 && st.mtimeMs < cutoff) files.push({ path: p, bytes: st.blocks * 512 });
    }
  }
}
const initialRefs = liveReferences();
for (const e of fs.readdirSync(path.join(ROOT, 'scratch'), { withFileTypes: true })) {
  const p = path.join(ROOT, 'scratch', e.name);
  if (e.isDirectory() && !KEEP.has(e.name) && !inUse(p, initialRefs)) bulk(p);
}
function recent(p) {
  const st = fs.lstatSync(p);
  if (st.mtimeMs >= cutoff) return true;
  return st.isDirectory() && fs.readdirSync(p).some(e => recent(path.join(p, e)));
}
const candidates = fs.readdirSync(ROOT, { withFileTypes: true }).filter(e => e.isDirectory() && /^cand\d/.test(e.name))
  .sort((a, b) => Number(b.name.match(/\d+/)[0]) - Number(a.name.match(/\d+/)[0]));
const old = [...candidates.slice(3).map(e => e.name), 'loop', 'loop7', 'loop8', 'loop9', 'loop10'];
for (const name of old) {
  const p = path.join(ROOT, name);
  if (fs.existsSync(p) && !inUse(p, initialRefs) && !recent(p)) {
    const bytes = Number(execFileSync('du', ['-s', '-B1', p], { encoding: 'utf8' }).split(/\s/)[0]);
    trees.push({ path: p, bytes });
  }
}
const refs = apply ? liveReferences() : initialRefs;
let freedBytes = 0;
const removed = [];
for (const item of [...files, ...trees]) {
  if (!fs.existsSync(item.path) || inUse(item.path, refs) || recent(item.path)) continue;
  if (apply) { fs.rmSync(item.path, { recursive: true }); freedBytes += item.bytes; }
  removed.push(item);
}
console.log(JSON.stringify({ mode: apply ? 'applied' : 'dry-run', preservedCandidates: candidates.slice(0, 3).map(e => e.name), items: removed, freedBytes }, null, 2));
