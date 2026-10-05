import { fileURLToPath as jetToolPath } from 'node:url';
const JET_TOOL_REPO = process.env.JET_REPO ?? jetToolPath(new URL('../../../', import.meta.url));
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

// Owner-approved literal-only cutover. Dry-run is the default.
const repo = JET_TOOL_REPO;
const cache = path.join(process.env.HOME, '.cache/jet-dev');
const apply = process.argv.includes('--apply');
const coupled = new Set(['Compiler/Bootstrap/Tests.rs', 'Compiler/Bootstrap/assemble.mjs', 'Compiler/Bootstrap/check.sh']);
const manual = [];
const replacements = new Map([
  ['jet-luna', 'jet-dev'], ['jet-test-scratch', 'jet-dev/scratch'],
  ['jet-proof-scratch', 'jet-dev/proof-scratch'], ['jet-gauntlet', 'jet-dev/gauntlet'],
  ['jet-perf', 'jet-dev/perf-bench'],
]);
function rewrite(text) {
  return text.replace(/\.cache\/(jet-luna|jet-test-scratch|jet-proof-scratch|jet-gauntlet|jet-perf)(?=\/|[\s"'`})\];,:]|$)/g,
    (match, name, offset) => {
      // Running host4 still embeds this relative bootstrap path. Private HOMEs
      // and the explanatory comments must keep matching it until host rebuild.
      if (name === 'jet-luna' && text.slice(offset + match.length).startsWith('/compiler-bootstrap')) return match;
      return '.cache/' + replacements.get(name);
    });
}
const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: repo, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }).split('\0').filter(Boolean);
const repoPaths = tracked.filter(p => !coupled.has(p)
  && !/^(Docs\/(audits|research)\/|\.agent-worktrees\/|\.claude\/worktrees\/|target(?:[^/]*)\/|build\/|\.jet\/|\.tmp\/)/.test(p)
  && !/(^|\/)\.tower(\/|$)/.test(p)
  && !/^Tools\/agent-eval\/.*\/_defects[^/]*\//.test(p)
  && !/^Tools\/gauntlet\/performance-surface-evidence-\d/.test(p)
  && p !== 'Docs/proposals/automatic-build-optimization.md');
const cachePaths = [];
function tooling(dir, recursive) {
  if (!fs.existsSync(dir)) return;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isSymbolicLink()) continue;
    if (e.isDirectory()) { if (recursive) tooling(p, true); continue; }
    if (/\.(sh|mjs|md)$/.test(e.name) && p !== path.join(cache, 'stage1/lib.sh')) cachePaths.push(p);
  }
}
// Stage1's retained outputs are excluded: only its top-level live tooling/docs.
tooling(path.join(cache, 'stage1'), false);
tooling(path.join(cache, 'gates'), true);
for (const p of ['port/wave']) {
  const dir = path.join(cache, p);
  if (fs.existsSync(dir)) for (const e of fs.readdirSync(dir, { withFileTypes: true }))
    if (e.isFile() && e.name.endsWith('.mjs')) cachePaths.push(path.join(dir, e.name));
}
for (const e of fs.readdirSync(cache, { withFileTypes: true })) {
  if (e.isFile() && /^(lane[SB]|safe-(jet|cargo|lanecheck)|isocheck(?:-priority)?|jetc0[^.]*|mem-guard|host[34]-build(?:-nosync)?|overlay-(build|sync)|stage0(?:[^.]*)|.*perf[^.]*)\.(sh|mjs)$/.test(e.name))
    cachePaths.push(path.join(cache, e.name));
}
const changedRepo = [], changedCache = [];
for (const [p, label] of [...repoPaths.map(p => [path.join(repo, p), p]), ...cachePaths.map(p => [p, p])]) {
  const stat = fs.lstatSync(p);
  if (!stat.isFile()) continue;
  const bytes = fs.readFileSync(p);
  if (bytes.includes(0)) continue;
  const before = bytes.toString('utf8');
  const after = rewrite(before);
  if (before === after) continue;
  const changed = p.startsWith(repo + '/') ? changedRepo : changedCache;
  changed.push(label);
  console.log(label);
  for (const [i, line] of before.split('\n').entries())
    if (line !== rewrite(line)) console.log(`  ${i + 1}: ${line.length > 450 ? line.slice(0, 450) + ' [truncated]' : line}`);
  if (apply) {
    // Do not overwrite sibling changes between read and write.
    if (!fs.readFileSync(p).equals(bytes)) throw new Error(`Concurrent change: ${p}`);
    fs.writeFileSync(p, after);
  }
}
const inventory = { mode: apply ? 'applied' : 'dry-run', repoFiles: [...new Set([...manual, ...changedRepo])].sort(), cacheFiles: changedCache.sort(), preserved: [...coupled, path.join(cache, 'stage1/lib.sh')] };
console.log(JSON.stringify(inventory, null, 2));
if (apply) fs.writeFileSync(path.join(cache, 'cache-cutover-inventory.json'), JSON.stringify(inventory, null, 2) + '\n');
