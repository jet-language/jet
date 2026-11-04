// audit2.mjs: #6563 c1 closure audit, pass 2 (read-only on the repo and Tower).
// Every criterion of a done card, every met/verified criterion of a not-done card (live + archived), and every
// done card with no criteria gets one row:
//   DURABLE      evidence names a commit reachable from dev or master, or one on the rust branch only (commit recorded)
//   KIND         the row declares --kind decision|duplicate|obsolete and its target checks out
//   NONCODE      evidence names a ratified D-… or a duplicate/merged/superseded #N that exists, or says obsolete
//   VERIFIED     no commit, but every repo path the evidence cites exists on dev; commit = last dev commit touching them
//   RETIRED      every cited path that is absent landed on dev and was deleted later; commit = the deleting commit
//   LANDED-REF   otherwise REOPEN, but a dev/master commit message names the card; commit = the newest such commit
//   REOPEN       none of the above (reason recorded)
// A done card's REOPEN criterion is retried against the card log before LANDED-REF.
// Usage: node audit2.mjs [OUTDIR]. Writes OUTDIR/CLOSURE-AUDIT-v2.tsv (default: beside this script), tally on stdout.
// Tower is read from $TOWER_DIR, else Tools/tower/.tower in the main checkout (the live board).
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const git = (a) => execFileSync('git', a, { cwd: HERE, env, encoding: 'utf8', maxBuffer: 1 << 30 });
const REPO = path.dirname(git(['rev-parse', '--path-format=absolute', '--git-common-dir']).trim());
const TOWER = process.env.TOWER_DIR || `${REPO}/Tools/tower/.tower`;
const DEV = git(['rev-parse', 'dev']).trim();
const reach = new Set(git(['rev-list', 'dev', 'master']).trim().split('\n'));
const rustOnly = new Set(git(['rev-list', 'rust', '--not', 'dev', 'master']).trim().split('\n').filter(Boolean));
const pre = new Map();
for (const h of [...reach, ...rustOnly]) { const k = h.slice(0, 7); if (!pre.has(k)) pre.set(k, []); pre.get(k).push(h); }
const resolve = (s) => (pre.get(s.slice(0, 7)) || []).filter((h) => h.startsWith(s.toLowerCase()));
const tree = new Set(git(['ls-tree', '-r', '-t', '--name-only', 'dev']).split('\n').filter(Boolean));
// Commits on dev/master whose message names a card (#N): a card's own landed commits.
const byCard = new Map();
for (const rec of git(['log', 'dev', 'master', '--format=%H%x1f%s%x1f%b%x1e']).split('\x1e')) {
  const [h, s, b] = rec.trim().split('\x1f');
  if (!h) continue;
  for (const m of `${s} ${b}`.matchAll(/#(\d{2,5})\b/g)) { const n = +m[1]; if (!byCard.has(n)) byCard.set(n, []); if (!byCard.get(n).includes(h)) byCard.get(n).push(h); }
}

const live = JSON.parse(fs.readFileSync(`${TOWER}/tower.json`, 'utf8'));
const hist = JSON.parse(fs.readFileSync(`${TOWER}/history.json`, 'utf8'));
const decisions = [...(live.decisions || []), ...(hist.decisions || [])];
const ratified = new Set(decisions.filter((d) => d.status === 'ratified').map((d) => d.id));
const cards = [...live.cards.map((c) => ({ ...c, _arch: false })), ...(hist.cards || []).map((c) => ({ ...c, _arch: true }))];
const cardNums = new Set(cards.map((c) => c.num));

const SHA = /\b[0-9a-f]{7,40}\b/g;
const TOPS = 'Compiler|Core|Docs|Examples|Jetpack|Tools|tests|\\.github|\\.jet|docs|examples|scripts|crates|editors|jetpack|core|compiler';
const RPATH = new RegExp(`(?<![\\w/.~-])(?:${TOPS})/[A-Za-z0-9_@+.,/-]*[A-Za-z0-9_]`, 'g');
// Every path ever deleted on dev, with the newest commit that deleted it.
const deletedBy = new Map();
for (const rec of git(['log', DEV, '--diff-filter=D', '--format=%x1e%H', '--name-only']).split('\x1e')) {
  const [h, ...names] = rec.trim().split('\n');
  if (!h) continue;
  for (const n of names) if (n && !deletedBy.has(n)) deletedBy.set(n, h);
}
// Pre-cutover spellings of today's top folders.
const LEGACY = [[/^docs\//, 'Docs/'], [/^examples\//, 'Examples/'], [/^scripts\//, 'Tools/'], [/^jetpack\//, 'Jetpack/'], [/^core\//, 'Core/'], [/^compiler\//, 'Compiler/'], [/^editors\//, 'Tools/editors/']];
const renamed = (p) => { for (const [re, to] of LEGACY) if (re.test(p)) { const q = p.replace(re, to); if (tree.has(q)) return q; } return null; };
const DEC = /\bD-[A-Z0-9]+(?:-[A-Z0-9]+)*\b/g;
const DUP = /\b(?:duplicate[sd]?(?: of)?|dup of|merged (?:in)?to|folded into|superseded by|subsumed by|covered by|moved to|replaced by)\s+(?:card\s+)?#(\d+)/gi;
const OBSOLETE = /\b(obsolete|retired|no longer applies|won'?t fix|wontfix|not needed|moot)\b/i;

const lastTouch = new Map();
function touchCommit(paths) {
  const key = paths.join('\n');
  if (!lastTouch.has(key)) lastTouch.set(key, git(['log', '-1', '--format=%H', DEV, '--', ...paths]).trim());
  return lastTouch.get(key);
}
function cleanPath(p) {
  let q = p.replace(/[.,:;]+$/, '').replace(/:\d+(?:[-,]\d+)*$/, '');
  while (q && !tree.has(q) && q.includes('/')) {
    // a trailing `.` sentence end or a `/`-joined list item; also try the dirname for globs like Foo/*.jet
    if (/\.[A-Za-z]+$/.test(q) && tree.has(q.replace(/\.[A-Za-z]+$/, ''))) return q.replace(/\.[A-Za-z]+$/, '');
    break;
  }
  return q;
}
// A ratified decision closes a row only when the row is a decision row: its text asks for a ruling, or its
// evidence says no code was needed. Otherwise a decision name is context, not proof of the change.
const DECISION_ROW = /\b(decid|decision|ballot|ratif|owner (?:choice|ruling|picks?)|rul(?:e|ing)\b|choose|chosen|option [A-E]\b|pick )/i;
const NO_CODE = /\b(no code (?:change|needed)|decision[- ]only|nothing to (?:land|change)|ruling only|policy only)\b/i;
function classify(text, kind, critText = '') {
  const shas = [...new Set((text.match(SHA) || []).filter((s) => /[a-f]/.test(s) && /[0-9]/.test(s)))];
  const hits = shas.map(resolve).filter((r) => r.length).map((r) => r[0]);
  const onDev = hits.filter((h) => reach.has(h));
  if (onDev.length) return { cls: 'DURABLE', commit: onDev[0].slice(0, 10), detail: `${onDev.length} commit(s) on dev/master` };
  if (hits.length) return { cls: 'DURABLE', commit: hits[0].slice(0, 10), detail: `${hits.length} commit(s) on the rust branch only (pre-cutover)` };
  if (kind) {
    if (kind === 'decision') { const ids = (text.match(DEC) || []).filter((d) => ratified.has(d)); if (ids.length) return { cls: 'KIND', commit: '', detail: `decision ${ids[0]} ratified` }; }
    else if (kind === 'duplicate') { const n = [...text.matchAll(/#(\d+)/g)].map((m) => +m[1]).find((x) => cardNums.has(x)); if (n) return { cls: 'KIND', commit: '', detail: `duplicate #${n}` }; }
    else if (kind === 'obsolete' && text.trim()) return { cls: 'KIND', commit: '', detail: 'obsolete: ' + text.replace(/\s+/g, ' ').slice(0, 80) };
  }
  const decs = [...new Set((text.match(DEC) || []).filter((d) => ratified.has(d)))];
  const dupRefs = [...new Set([...text.matchAll(DUP)].map((m) => +m[1]))];
  const dups = dupRefs.filter((n) => cardNums.has(n));
  const raw = [...new Set((text.match(RPATH) || []).map(cleanPath))];
  const paths = raw.map((p) => (tree.has(p) ? p : renamed(p) ?? p));
  const present = paths.filter((p) => tree.has(p));
  const missing = paths.filter((p) => !tree.has(p));
  const retired = missing.filter((p) => deletedBy.has(p));
  if (present.length && !missing.length) return { cls: 'VERIFIED', commit: touchCommit(present).slice(0, 10), detail: `${present.length} repo path(s) on dev ${DEV.slice(0, 10)}: ${present.slice(0, 3).join(' ')}` };
  if (missing.length && retired.length === missing.length) {
    const by = deletedBy.get(retired[0]);
    return { cls: 'RETIRED', commit: by.slice(0, 10), detail: `every absent path landed on dev and was deleted later (${retired.slice(0, 2).join(' ')} by ${by.slice(0, 10)})${present.length ? `; ${present.length} path(s) still present` : ''}` };
  }
  if (dups.length) return { cls: 'NONCODE', commit: '', detail: `kind=duplicate #${dups[0]}` };
  if (decs.length && (DECISION_ROW.test(critText) || NO_CODE.test(text))) return { cls: 'NONCODE', commit: '', detail: `kind=decision ${decs[0]}` };
  if (decs.length && !paths.length && !shas.length) return { cls: 'REOPEN', commit: '', detail: `cites ${decs[0]} but the row asks for a change and names no commit` };
  if (dupRefs.length) return { cls: 'REOPEN', commit: '', detail: `merged into #${dupRefs[0]}, which is not on the board (live or archived)` };
  if (present.length) return { cls: 'REOPEN', commit: '', detail: `cited path(s) never on dev: ${missing.filter((p) => !deletedBy.has(p)).slice(0, 3).join(' ')} (present: ${present.length})` };
  if (missing.length) return { cls: 'REOPEN', commit: '', detail: `cited path(s) never on dev: ${missing.filter((p) => !deletedBy.has(p)).slice(0, 3).join(' ')}` };
  if (shas.length) return { cls: 'REOPEN', commit: '', detail: `commit(s) not in the repo: ${shas.slice(0, 3).join(' ')}` };
  if (OBSOLETE.test(text)) return { cls: 'NONCODE', commit: '', detail: 'kind=obsolete: ' + text.replace(/\s+/g, ' ').slice(0, 80) };
  if (!text.trim()) return { cls: 'REOPEN', commit: '', detail: 'no evidence text' };
  return { cls: 'REOPEN', commit: '', detail: 'evidence names no commit, repo path, decision or card: ' + text.replace(/\s+/g, ' ').slice(0, 100) };
}
// One line per field; home-directory paths quoted from old evidence become ~ (#6564 c5), including clipped ones.
const clip = (s) => String(s || '').replace(/[\t\n\r]+/g, ' ').replace(/\/home\/[^/\s]*/g, '~');
const rows = [];
const tally = {};
// Last resort before REOPEN: a dev/master commit whose message names this card (or the card it was merged from).
function byRef(c, r) {
  if (r.cls !== 'REOPEN') return r;
  const hs = byCard.get(c.num) || [];
  if (!hs.length) return r;
  return { cls: 'LANDED-REF', commit: hs[0].slice(0, 10), detail: `${hs.length} dev/master commit(s) name #${c.num}; was: ${r.detail}` };
}
for (const c of cards) {
  const crit = c.criteria || [];
  if (c.phase === 'done') {
    // a done card: each criterion is judged on its own evidence; the card row takes the weakest row
    const parts = crit.map((k) => ({ k, r: classify(String(k.evidence || ''), k.kind, String(k.text || '')) }));
    const logText = (c.log || []).map((l) => (typeof l === 'string' ? l : l.text || l.message || JSON.stringify(l))).join(' ');
    for (const { k, r } of parts) {
      let rr = r;
      if (rr.cls === 'REOPEN') {
        const viaLog = classify(logText + ' ' + (c.completedAt || ''), null, '');
        if (viaLog.cls === 'DURABLE' || viaLog.cls === 'VERIFIED' || /kind=duplicate/.test(viaLog.detail)) rr = { ...viaLog, detail: 'via card log: ' + viaLog.detail };
        rr = byRef(c, rr);
      }
      const st = k.status === 'met' || k.status === 'verified' ? '' : `[status ${k.status}] `;
      rows.push(['CRIT', `#${c.num} c${k.n}`, c._arch ? 'archived' : 'live', 'done', c.kind || '', rr.cls, rr.commit, clip(st + rr.detail), clip(k.text).slice(0, 90)]);
      tally['crit:' + rr.cls] = (tally['crit:' + rr.cls] || 0) + 1;
    }
    if (!crit.length) {
      const r = byRef(c, classify(logText, null, String(c.title || '')));
      rows.push(['CARD', `#${c.num}`, c._arch ? 'archived' : 'live', 'done', c.kind || '', r.cls, r.commit, clip('no criteria; ' + r.detail), clip(c.title).slice(0, 90)]);
      tally['card-nocrit:' + r.cls] = (tally['card-nocrit:' + r.cls] || 0) + 1;
    }
    continue;
  }
  for (const k of crit) {
    if (k.status !== 'met' && k.status !== 'verified') continue;
    const r = byRef(c, classify(String(k.evidence || ''), k.kind, String(k.text || '')));
    rows.push(['CRIT', `#${c.num} c${k.n}`, c._arch ? 'archived' : 'live', c.phase || '', c.kind || '', r.cls, r.commit, clip(r.detail), clip(k.text).slice(0, 90)]);
    tally['crit:' + r.cls] = (tally['crit:' + r.cls] || 0) + 1;
  }
}
const out = path.join(process.argv[2] || HERE, 'CLOSURE-AUDIT-v2.tsv');
fs.writeFileSync(out, `# #6563 c1 closure audit v2; dev ${DEV}; generated ${new Date().toISOString()} by audit2.mjs\ntype\tref\twhere\tphase\tkind\tclass\tcommit\tdetail\tcriterion_or_title\n` + rows.map((r) => r.join('\t')).join('\n') + '\n');
console.log(JSON.stringify({ dev: DEV, cards: cards.length, rows: rows.length, tally }, null, 1));
