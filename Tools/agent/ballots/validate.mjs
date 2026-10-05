import { fileURLToPath as jetToolPath } from 'node:url';
const JET_TOOL_REPO = process.env.JET_REPO ?? jetToolPath(new URL('../../../', import.meta.url));
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { ballotGaps, addDecision, updateDecision } from '../../tower/app/store.mjs';

const repo = JET_TOOL_REPO;
const root = path.join(process.env.HOME, '.cache/jet-dev/ballots');
const towerRead = (...args) => JSON.parse(execFileSync('node', ['Tools/tower/tower.mjs', ...args, '--json'], {
  cwd: repo, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024,
}));
const files = process.argv.slice(2);
if (!files.length) {
  files.push(...fs.readdirSync(path.join(root, 'gates')).filter(f => f.endsWith('.json')).sort().map(f => path.join(root, 'gates', f)));
  files.push(...['D-SQL-SAFETY', 'D-UNSAFE-EFFECT', 'D-LANG-IN-LIBS'].map(id => path.join(root, `${id}.json`)));
}
const results = [];
for (const file of files) {
  const p = JSON.parse(fs.readFileSync(file, 'utf8'));
  const card = towerRead('card', 'show', p.cardId);
  const existing = (card.decisions || []).some(decision => decision.id === p.id);
  const gaps = ballotGaps({ ...p, status: 'open', ballotProcessVersion: 4 }, {
    requireBeginner: true, requireSurface: true, requireRecFirst: !existing, card,
  });
  if (!card) gaps.push('cardId does not identify a current card');
  if (p.draft !== false) gaps.push('posting payload must set draft:false');
  let dryRun = null;
  try {
    // Only a detached in-memory snapshot is mutated. No store, locks or Tower writes are opened.
    const snapshot = structuredClone({ cards: [card], decisions: card.decisions || [], events: [] });
    const saved = existing ? updateDecision(snapshot, p.id, { ...p, ready: true }, 'Pip')
      : addDecision(snapshot, { ...p, by: 'Pip' });
    dryRun = { operation: existing ? 'update' : 'add', id: saved.id, card: card.num, draft: saved.draft, reviewOrder: Object.keys(saved.reviewPasses || {}) };
  } catch (error) {
    gaps.push(`${error.code || 'ERROR'}: ${error.message}`);
  }
  results.push({ file, id: p.id, card: card?.num ?? null, gaps, dryRun });
}
console.log(JSON.stringify({ checkedAt: new Date().toISOString(), boardWritten: false, results }, null, 2));
if (results.some(result => result.gaps.length)) process.exitCode = 1;
