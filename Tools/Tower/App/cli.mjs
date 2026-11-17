// Tower CLI — the full agent + owner surface. Every operation the UI can do
// is available here, so nobody ever hand-edits tower.json.
//
//   tower <noun> <verb> [args] [--flags]     e.g. tower card update 12 --phase building
//   --json on any command → machine-readable output
//   complex payloads (decisions) → --file payload.json or `-` for stdin
import { readFileSync, mkdirSync, existsSync, writeFileSync, readdirSync, chmodSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as db from './store.mjs';
import { openStore, TowerError, PHASE_IDS } from './store.mjs';
import { findDataDir, projectRoot, readJSON, readLatestJSON, writeJSON } from './paths.mjs';
import { SHARD_BYTES, historyFiles, planShards } from './history-shards.mjs';
import { ConfigError } from './config.mjs';
import { migrate } from './migrate.mjs';
import { criteriaOverLimit, evidenceProvenance, lint } from './lint.mjs';
import { executeRuns, proveEvidence, proveRuns, proveTargets, unmatchedProofs } from './proof.mjs';
import { findDuplicateCandidates } from './card-matching.mjs';
import { hasHardeningPayload } from './hardening.mjs';
import * as docs from './docs.mjs';
import { applyRepairManifest } from './repair.mjs';
import { machinePersonalInfo } from './personal-info.mjs';
import { readGauntletStandings } from './gauntlet-standings.mjs';
import { gauntletStandingsText, projectGauntletStandings } from './UI/gauntlet.js';
import { CARD_SORTS, closestCards, epochChain, epochLabel, parseSort, proofCommand, sortCards, sortEpochs, sortMilestones } from './UI/board-state.js';
import { applyPlan } from './apply.mjs';

// ---- arg parsing (zero-dep) ------------------------------------------------

// A repeatable flag collects every value in order (`--exit-criteria a
// --exit-criteria b`); any other flag keeps its last value.
const REPEATABLE_FLAGS = new Set(['exitCriteria']);

function parseArgs(argv) {
  const pos = []; const flags = {};
  const set = (key, value) => {
    flags[key] = REPEATABLE_FLAGS.has(key) && Object.hasOwn(flags, key) ? [flags[key], value].flat() : value;
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const raw = a.slice(2);
      const eq = raw.indexOf('=');
      const key = (eq < 0 ? raw : raw.slice(0, eq)).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      if (eq >= 0) { set(key, raw.slice(eq + 1)); continue; }
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('--')) set(key, true);
      else { set(key, next); i++; }
    } else pos.push(a);
  }
  return { pos, flags };
}

// A flag a command does not read used to vanish in silence: `card add --text
// "…"` created a card with an empty body, and a typo'd `--phase` on `card
// update` no-oped. Every flag is declared here (camelCase, as parseArgs
// normalizes it) and anything else is a usage error.
const GLOBAL_FLAGS = ['json', 'help', 'data'];
const by = (...flags) => [...new Set(['by', ...flags])];
const payload = (...flags) => by('file', 'stdin', ...flags);

// Flags belong to verbs, not nouns. A noun-wide allow-list makes a valid flag
// on one verb silently disappear on its sibling (for example --meet on card
// add). This table is the CLI contract; help below is rendered from it.
export const COMMAND_FLAGS = {
  init: { flags: ['name', 'dir'] },
  import: { flags: ['dir', 'name', 'force'] },
  serve: { flags: ['port', 'open', 'noWatch', 'worker'] },
  status: { flags: ['days', 'window', 'color'], verbs: {
    post: payload('expectRev'),
    show: [],
  }},
  briefing: { verbs: {
    post: by('file', 'title', 'sections', 'expectRev'),
    list: [],
    show: [],
  }},
  state: { flags: [] },
  help: { flags: ['check'] },
  card: { verbs: {
    list: ['lane', 'phase', 'epoch', 'track', 'kind', 'milestone', 'tag', 'untagged', 'parent', 'hardeningDedupKey', 'sort'],
    show: ['hardeningDedupKey'],
    add: payload('title', 'body', 'kind', 'track', 'epoch', 'milestone', 'phase', 'priority', 'plan',
      'checkSteps', 'probe', 'workOrder', 'needsAcceptance', 'blockedBy', 'refs', 'tags', 'addTag', 'parent', 'force', 'overLimit',
      'hardeningDedupKey', 'hardeningSeam', 'hardeningRelation', 'hardeningWrongTierMask',
      'hardeningInputPartition', 'hardeningFindingId', 'hardeningFixture'),
    update: payload('title', 'body', 'kind', 'track', 'epoch', 'milestone', 'phase', 'priority', 'plan',
      'checkSteps', 'probe', 'workOrder', 'log', 'needsAcceptance', 'blockedBy', 'refs', 'tags', 'addTag', 'removeTag', 'parent', 'expectRev', 'overLimit',
      'hardeningDedupKey', 'hardeningSeam', 'hardeningRelation', 'hardeningWrongTierMask',
      'hardeningInputPartition', 'hardeningFindingId', 'hardeningFixture'),
    claim: by(),
    release: by('handoff'),
    delete: by(),
    criteria: by('add', 'edit', 'text', 'proof', 'blockedBy', 'unblock', 'remove', 'move', 'to', 'meet', 'verify', 'kind', 'reopen', 'reason', 'evidence', 'list', 'overLimit'),
    split: by('criteria', 'title', 'overLimit'),
    log: by('text'),
    history: ['limit'],
  }},
  decision: { verbs: {
    list: ['open', 'card', 'drafts'],
    show: [],
    scaffold: ['id', 'out'],
    add: payload('id', 'card', 'title', 'situation', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec',
      'group', 'ballotMode', 'shortAuthorizedBy', 'draft', 'reviewedBy'),
    update: payload('title', 'situation', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group',
      'ballotMode', 'shortAuthorizedBy'),
    review: by('reject', 'reason'),
    post: by(),
    report: by('path', 'url', 'title', 'note', 'remove'),
    ratify: by('outcome', 'comment', 'quote', 'expectRev'),
    reopen: by(),
    delete: by(),
  }},
  question: { verbs: {
    list: ['open', 'card'],
    ask: by('text', 'kind', 'decision'),
    answer: by('text'),
    delete: by(),
  }},
  message: { verbs: {
    list: ['card', 'status', 'all'],
    add: by('text'),
    done: by(),
  }},
  idea: { verbs: {
    list: ['all'],
    add: by('title', 'body', 'text', 'kind', 'track', 'priority', 'tags', 'note'),
    promote: by('title', 'body', 'kind', 'track', 'priority'),
    delete: by(),
  }},
  epoch: { verbs: {
    list: [],
    add: payload('name', 'goal', 'status', 'order', 'exitCriteria'),
    update: payload('name', 'goal', 'status', 'order', 'exitCriteria'),
    current: by(),
    remove: by(),
  }},
  milestone: { verbs: {
    list: ['epoch', 'retired'],
    add: payload('id', 'epoch', 'title', 'goal', 'order', 'exitCriteria'),
    update: payload('title', 'goal', 'order', 'exitCriteria', 'status', 'epoch'),
    criteria: by('add', 'edit', 'text', 'remove', 'meet', 'verify', 'kind', 'reopen', 'reason', 'evidence', 'list'),
    retire: by('reason'),
    closeout: by('commit'),
    verify: by('evidence', 'expectRev'),
    delete: by(),
  }},
  next: { flags: ['agent', 'epoch', 'track', 'limit', 'burndown', 'parallel', 'readyAcrossEpochs'] },
  chain: { flags: [] },
  closest: { flags: ['epoch', 'limit', 'sort'] },
  ballots: { flags: [] },
  brief: { flags: ['agent', 'noClaim', 'color'] },
  lint: { flags: ['proofs', 'tree', 'evidence', 'limits'] },
  prove: { flags: by('card', 'epoch', 'jet', 'tree', 'commit', 'dryRun', 'yes') },
  docs: { verbs: {
    list: [],
    show: ['path', 'scratch'],
    add: payload('section', 'title', 'path', 'id', 'body'),
    update: payload('title', 'path', 'body', 'scratch'),
    delete: by('path'),
  }},
  verdict: { flags: by('outcome', 'title', 'supersedes') },
  archive: { verbs: {
    status: [],
    show: [],
    restore: by(),
    shard: by('apply', 'expectRev'),
  }},
  repair: { verbs: {
    apply: by('manifest', 'dryRun', 'expectRev'),
  }},
  redact: { flags: by('apply', 'expectRev') },
  events: { flags: ['limit'] },
  undo: { flags: by('expectRev') },
  githook: { verbs: { install: [], 'post-commit': [] } },
  board: { verbs: { commit: by(), check: [] } },
  apply: { flags: payload('dryRun', 'expectRev') },
  publish: { flags: by('title', 'file') },
  gauntlet: { verbs: { standings: ['table'] } },
};

// What a command cannot run without: its positional argument (named as help
// shows it) and its required flags, keyed `noun verb` (or the bare command).
// `waivedBy` flags stand in for all of them: a --file/--stdin payload carries
// the same fields, and a hardening card is matched by its dedup key.
const PAYLOAD_WAIVER = ['file', 'stdin'];
const REQUIRED = {
  'status post': { flags: ['file'], waivedBy: ['stdin'] },
  'briefing post': { flags: ['file'] },
  'card show': { arg: 'REF', waivedBy: ['hardeningDedupKey'] },
  'card add': { flags: ['title'], waivedBy: [...PAYLOAD_WAIVER, 'hardeningDedupKey'] },
  'card update': { arg: 'REF' },
  'card claim': { arg: 'REF' },
  'card release': { arg: 'REF' },
  'card delete': { arg: 'REF' },
  'card criteria': { arg: 'REF' },
  'card log': { arg: 'REF', flags: ['text'] },
  'card history': { arg: 'REF' },
  'card split': { arg: 'REF', flags: ['criteria', 'title'] },
  'decision show': { arg: 'ID' },
  'decision scaffold': { arg: 'REF', flags: ['id'] },
  'decision add': { flags: ['card'], waivedBy: PAYLOAD_WAIVER },
  'decision update': { arg: 'ID' },
  'decision review': { arg: 'ID' },
  'decision post': { arg: 'ID' },
  'decision report': { arg: 'ID' },
  'decision ratify': { arg: 'ID', flags: ['outcome'] },
  'decision reopen': { arg: 'ID' },
  'decision delete': { arg: 'ID' },
  'question ask': { arg: 'REF', flags: ['text'] },
  'question answer': { arg: 'ID', flags: ['text'] },
  'question delete': { arg: 'ID' },
  'message add': { arg: 'REF', flags: ['text'] },
  'message done': { arg: 'ID' },
  'idea promote': { arg: 'ID' },
  'idea delete': { arg: 'ID' },
  'epoch add': { arg: 'E', waivedBy: PAYLOAD_WAIVER },
  'epoch update': { arg: 'E' },
  'epoch current': { arg: 'E|none' },
  'epoch remove': { arg: 'E' },
  'milestone add': { flags: ['epoch', 'title'], waivedBy: PAYLOAD_WAIVER },
  'milestone update': { arg: 'M' },
  'milestone criteria': { arg: 'M' },
  'milestone retire': { arg: 'M', flags: ['reason'] },
  'milestone closeout': { arg: 'M' },
  'milestone verify': { arg: 'M' },
  'milestone delete': { arg: 'M' },
  verdict: { arg: 'REF' },
  'archive show': { arg: 'ID' },
  'archive restore': { arg: 'ID' },
  'repair apply': { flags: ['manifest'] },
  import: { arg: 'FILE' },
  'board commit': { flags: ['by'] },
  apply: { flags: ['file'], waivedBy: ['stdin'] },
  publish: { flags: ['title', 'file', 'by'] },
};

const spellFlag = (key) => `--${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
const commandFlags = (cmd, verb) => {
  const spec = COMMAND_FLAGS[cmd];
  if (!spec) return null;
  if (!spec.verbs) return spec.flags;
  return verb ? spec.verbs[verb] : spec.flags || spec.verbs[cmd === 'githook' ? 'install' : verb];
};
// `noun verb` for a command with verbs, else the bare command.
const commandPath = (cmd, verb) => COMMAND_FLAGS[cmd]?.verbs && verb !== undefined ? `${cmd} ${verb}` : cmd;

// Optimal-string-alignment distance: insertions, deletions, substitutions and
// adjacent swaps each cost one.
function editDistance(a, b) {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
    }
  }
  return d[a.length][b.length];
}

// The option a mistyped word most likely meant: the nearest one within a third
// of the word's length (at least one edit), else the only option it begins.
function nearest(word, options) {
  const w = String(word).toLowerCase();
  let best = null;
  let bestDistance = Infinity;
  for (const option of options) {
    const distance = editDistance(w, option.toLowerCase());
    if (distance < bestDistance) { best = option; bestDistance = distance; }
  }
  if (best !== null && bestDistance <= Math.max(1, Math.floor(w.length / 3))) return best;
  const prefixed = w.length >= 2 ? options.filter(option => option.toLowerCase().startsWith(w)) : [];
  return prefixed.length === 1 ? prefixed[0] : null;
}
const didYouMean = (guess) => guess ? ` — did you mean \`${guess}\`?` : '';

function checkFlags(cmd, verb, flags) {
  const spec = COMMAND_FLAGS[cmd];
  if (!spec) {
    const guess = nearest(cmd, Object.keys(COMMAND_FLAGS));
    throw new TowerError('E_USAGE', `unknown command "${cmd}"${didYouMean(guess && `tower ${guess}`)} — run \`tower help\``);
  }
  if (spec.verbs) {
    const verbs = Object.keys(spec.verbs);
    if (verb === undefined && !spec.flags && cmd !== 'githook')
      throw new TowerError('E_USAGE', `\`tower ${cmd}\` needs a verb: ${verbs.join('|')}`);
    if (verb !== undefined && !Object.hasOwn(spec.verbs, verb)) {
      const guess = nearest(verb, verbs);
      throw new TowerError('E_USAGE', `unknown ${cmd} verb "${verb}"${didYouMean(guess && `tower ${cmd} ${guess}`)} — one of ${verbs.join('|')}`);
    }
  }
  const known = [...GLOBAL_FLAGS, ...commandFlags(cmd, verb)];
  const unknown = Object.keys(flags).filter(f => !known.includes(f));
  if (!unknown.length) return;
  const named = unknown.map((f) => {
    const guess = nearest(spellFlag(f), known.map(spellFlag));
    return `${spellFlag(f)}${guess ? ` (did you mean ${guess}?)` : ''}`;
  });
  throw new TowerError('E_USAGE',
    `unknown flag${unknown.length > 1 ? 's' : ''} for \`tower ${commandPath(cmd, verb)}\`: `
    + `${named.join(', ')} — run \`tower help\``);
}

// A required flag counts as given only with a value; `--text` alone or with
// blank text is missing.
function checkRequired(cmd, verb, args, flags) {
  const path = commandPath(cmd, verb);
  const need = REQUIRED[path];
  if (!need || (need.waivedBy || []).some(f => flags[f] !== undefined)) return;
  const given = (f) => flags[f] !== undefined && flags[f] !== true && String(flags[f]).trim() !== '';
  const missing = [
    ...(need.arg && !args.length ? [need.arg] : []),
    ...(need.flags || []).filter(f => !given(f)).map(f => flagToken(f, path)),
  ];
  if (missing.length) throw new TowerError('E_USAGE', `tower ${path} needs ${missing.join(' and ')}`);
}

const FLAG_VALUE = {
  add: '"text"', edit: 'n', remove: 'n', move: 'n', to: 'REF', meet: 'n', verify: 'n', reopen: 'n', unblock: 'n', reason: '"…"', evidence: '"…"',
  overLimit: '"reason"', criteria: 'N,N,…', limits: null, sort: 'KEY[:asc|desc]',
  proof: '"jet test …"', proofs: null, jet: 'PATH', tree: 'DIR', yes: null,
  by: 'X', file: 'FILE', stdin: null, color: '=auto|always|never',
  sections: 'FILE',
  title: '"…"', body: '"…"', text: '"…"', path: 'PATH', section: 'SECTION',
  id: 'ID', card: 'REF', decision: 'ID', epoch: 'E', milestone: 'M',
  phase: 'P', track: 'T', kind: 'K', priority: 'P', plan: '"…"', checkSteps: '"…"', probe: '"jet run …"', lane: 'L',
  tag: 'T', tags: 'a,b', addTag: 'T', removeTag: 'T', parent: 'REF', blockedBy: 'REF',
  refs: 'a,b', workOrder: 'N', needsAcceptance: 'true|false', log: '"…"', handoff: '"…"',
  expectRev: 'N', name: 'X', dir: 'PATH', port: 'N', days: 'N', window: 'N', goal: '"…"', order: 'N', exitCriteria: '"…"',
  status: 'S', open: null, all: null, untagged: null, force: null, retired: null,
  draft: null, drafts: null, reviewedBy: 'X', reject: null, comment: '"…"', quote: '"…"',
  outcome: 'K', agent: 'A', noClaim: null, limit: 'N', burndown: null, parallel: null,
  readyAcrossEpochs: null, scratch: null, note: '"…"',
  manifest: 'FILE', dryRun: null, apply: null, commit: 'OID', hardeningDedupKey: 'KEY', hardeningSeam: 'SEAM',
  hardeningRelation: 'RELATION', hardeningWrongTierMask: 'TIERS', hardeningInputPartition: 'PARTITION',
  hardeningFindingId: 'ID', hardeningFixture: 'PATH', out: 'FILE', results: 'PATH',
};

// A flag that is a value elsewhere but a switch on one command.
const SWITCH_AT = { lint: ['evidence'] };
const flagToken = (key, path) => {
  const value = SWITCH_AT[path]?.includes(key) ? null : FLAG_VALUE[key];
  return `${spellFlag(key)}${value == null ? '' : String(value).startsWith('=') ? value : ` ${value}`}`;
};
const flagSynopsis = (flags, path) => flags
  .filter((key) => key !== 'by')
  .concat(flags.includes('by') ? ['by'] : [])
  .map((key) => flagToken(key, path))
  .join(' ');
const helpLine = (path, flags) => {
  if (path === 'brief' && flags.includes('color')) {
    return `  tower brief [${flagSynopsis(flags.filter((key) => key !== 'color'), path)}]\n              [${flagToken('color', path)}]`;
  }
  const arg = REQUIRED[path]?.arg;
  return `  tower ${path}${arg ? ` ${arg}` : ''}${flags.length ? ` [${flagSynopsis(flags, path)}]` : ''}`;
};

// Generated from COMMAND_FLAGS. Keep this as the only user-facing command
// surface list; `tower help --check` verifies its flags against the parser.
const HELP_ENTRIES = Object.entries(COMMAND_FLAGS).flatMap(([cmd, spec]) => {
  if (!spec.verbs) return [helpLine(cmd, spec.flags)];
  return [
    ...(spec.flags ? [helpLine(cmd, spec.flags)] : []),
    `  tower ${cmd}     ${Object.keys(spec.verbs).join('|')}`,
    ...Object.entries(spec.verbs).map(([verb, flags]) => helpLine(`${cmd} ${verb}`, flags)),
  ];
});
const HELP_SURFACE = HELP_ENTRIES.join('\n');

// A --file/--stdin JSON payload; null when the command was given neither.
const readPayload = (flags) => {
  const source = flags.file === '-' || flags.stdin ? 0 : flags.file;
  if (source === undefined || source === false) return null;
  const named = source === 0 ? 'stdin' : source;
  let text;
  try { text = readFileSync(source, 'utf8'); }
  catch (error) { throw new TowerError('E_USAGE', `cannot read ${named}: ${error.message}`); }
  try { return JSON.parse(text); }
  catch (error) { throw new TowerError('E_USAGE', `${named} is not a JSON payload: ${error.message}`); }
};

const out = (flags, human, data) => {
  // `show` verbs pass human=null with the record in `data`; human output falls
  // back to the record instead of printing "null".
  const payload = flags.json ? data ?? human : human ?? data;
  if (typeof payload === 'string') console.log(payload);
  else console.log(JSON.stringify(payload, null, 2));
};

// One zero-dependency vocabulary shared by Tower's two human dashboard
// surfaces. JSON never passes through Theme, so machine output stays clean.
function Theme(flags) {
  const choice = typeof flags.color === 'string' ? flags.color : 'auto';
  const color = !flags.json && (choice === 'always' || (choice !== 'never'
    && !Object.hasOwn(process.env, 'NO_COLOR')
    && (Object.hasOwn(process.env, 'FORCE_COLOR') || !!process.stdout.isTTY)));
  const paint = (sgr, text) => color ? `\x1b[${sgr}m${text}\x1b[0m` : text;
  return {
    accent: (text) => paint('1;96', text),
    dim: (text) => paint('2', text),
    success: (text) => paint('32', text),
    warn: (text) => paint('33', text),
    error: (text) => paint('31', text),
    invert: (text) => paint('7', text),
    border: (text) => paint('90', text),
  };
}

const displayLane = (lane) => lane === 'verify' ? 'review' : lane;
// `epoch` is the label to show for the card's epoch, when the line has one.
const cardLine = (c, { epoch } = {}) =>
  `#${String(c.num).padEnd(4)}${epoch ? ` ${epoch.padEnd(4)} ` : ' '}${(c.priority || '').padEnd(3)} ${displayLane(c.lane.lane).padEnd(9)} ${c.title.slice(0, epoch ? 52 : 60)}`;
// The short `E18` tag for an epoch id on this board; `—` when there is none.
const epochTag = (s, id) => id ? epochLabel(s.epochs.find(e => e.id === id) || { id }, { name: false }) : '—';
// One criterion row of `card criteria --list`, with its proof, blocker and
// move links.
const criterionLine = (it) => `#${it.n} [${it.status}] ${it.text}`
  + (it.proof ? `  proof: ${proofCommand(it.proof)}` : '')
  + (it.blockedBy ? `  blocked by ${it.blockedBy}` : '')
  + (it.unblockedFrom ? `  unblocked: ${it.unblockedFrom} resolved` : '')
  + (it.movedFrom ? `  ← from #${it.movedFrom.card} c${it.movedFrom.n}` : '')
  + (it.movedTo ? `  → moved to #${it.movedTo.card} c${it.movedTo.n}` : '')
  + (it.metBy ? `  met:${it.metBy}` : '') + (it.verifiedBy ? `  verified:${it.verifiedBy}` : '')
  + (it.evidence && !it.movedTo ? `  — ${it.evidence}` : '');
const graphemes = new Intl.Segmenter('en', { granularity: 'grapheme' });
const zeroWidth = /[\p{Mark}\p{Default_Ignorable_Code_Point}\p{Control}]/u;
const glyphWidth = (glyph) => {
  const visible = [...glyph].filter(ch => !zeroWidth.test(ch));
  if (!visible.length) return 0;
  if (glyph.includes('\ufe0f') || visible.some(ch => {
    const cp = ch.codePointAt(0);
    return /[\p{Unified_Ideograph}\p{Extended_Pictographic}]/u.test(ch)
      || (cp >= 0x1100 && (cp <= 0x115f || cp === 0x2329 || cp === 0x232a
        || (cp >= 0x2e80 && cp <= 0xa4cf && cp !== 0x303f)
        || (cp >= 0xac00 && cp <= 0xd7a3) || (cp >= 0xf900 && cp <= 0xfaff)
        || (cp >= 0xfe10 && cp <= 0xfe6f) || (cp >= 0xff00 && cp <= 0xff60)
        || (cp >= 0xffe0 && cp <= 0xffe6) || (cp >= 0x1f000 && cp <= 0x1faff)
        || (cp >= 0x20000 && cp <= 0x3fffd)));
  })) return 2;
  return 1;
};
const displayWidth = (text) => [...graphemes.segment(text)].reduce((n, part) => n + glyphWidth(part.segment), 0);
const clip = (text, width) => {
  if (displayWidth(text) <= width) return text;
  if (width <= 0) return '';
  let used = 0;
  let out = '';
  for (const { segment } of graphemes.segment(text)) {
    const next = glyphWidth(segment);
    if (used + next + 1 > width) break;
    out += segment;
    used += next;
  }
  return out + '…';
};

// ---- commands ----------------------------------------------------------------

const DATA_IGNORES = ['backups/', '*.lock/', 'files/', 'server.log', 'secrets.json', '.secrets.json.tmp-*'];
function ensureDataIgnores(dir) {
  const file = join(dir, '.gitignore');
  const current = existsSync(file) ? readFileSync(file, 'utf8') : '';
  const lines = new Set(current.split(/\r?\n/));
  const missing = DATA_IGNORES.filter(entry => !lines.has(entry));
  if (!missing.length) return;
  const prefix = current && !current.endsWith('\n') ? '\n' : '';
  writeFileSync(file, current + prefix + missing.join('\n') + '\n');
}

function cmdInit({ flags }) {
  const dir = resolve(flags.dir || '.', '.tower');
  const file = join(dir, 'tower.json');
  if (existsSync(file)) throw new TowerError('E_EXISTS', `already initialized at ${file}`);
  mkdirSync(dir, { recursive: true });
  const name = flags.name || 'Project';
  writeJSON(file, db.empty(name));
  const cfg = { project: name };
  if (!existsSync(join(dir, 'config.json'))) writeJSON(join(dir, 'config.json'), cfg);
  ensureDataIgnores(dir);
  return out(flags, `initialized Tower for "${name}" at ${dir}\nnext: tower epoch add e1 --name "First epoch" && tower serve --open`,
    { project: name, dir, file });
}

function cmdStatus(store, { flags }) {
  const raw = store.load();
  const history = store.loadHistory();
  const s = db.project(raw, store.config, history);
  const rawWindow = flags.days ?? flags.window ?? 7;
  const windowDays = Number(rawWindow);
  if (typeof rawWindow === 'boolean' || !Number.isInteger(windowDays) || windowDays < 1)
    throw new TowerError('E_USAGE', '--days/--window must be a positive whole number');
  const trend = db.openCountTrend(raw, history, windowDays);
  if (flags.json) return out(flags, null, { meta: s.meta, counts: s.counts, trend });
  const t = Theme(flags);
  const bar = (n) => t.success('█'.repeat(Math.min(12, n))) + t.border('░'.repeat(Math.max(0, 12 - n)));
  const phase = (id, text) => ({ done: t.success, verify: t.warn, deciding: t.error }[id] || t.accent)(text);
  const current = s.epochs.find(e => e.id === db.activeEpoch(s));
  const epochText = current ? epochLabel(current) : `${store.config.terms.epoch.toLowerCase()} —`;
  console.log(`\n  ${t.invert('TOWER')} ${t.border('·')} ${t.accent(s.meta.project)} ${t.border('·')} ${t.dim(`${epochText} · rev ${s.meta.rev}`)}\n`);
  for (const ph of db.PHASES) {
    const n = s.counts.byPhase[ph.id];
    if (n) console.log(`  ${phase(ph.id, ph.label.padEnd(9))} ${bar(n)} ${t.dim(String(n))}`);
  }
  const delta = `${trend.delta > 0 ? '+' : ''}${trend.delta}`;
  const deltaText = trend.delta > 0 ? t.error(delta) : trend.delta < 0 ? t.success(delta) : t.dim(delta);
  console.log(`  ${t.accent('OPEN TREND')}        ${trend.openAtStart} → ${trend.openNow}  ${deltaText} ${t.dim(`over ${trend.windowDays}d`)}`);
  const day = s.counts.today;
  const today = (n, d) => `${n}${d ? ` ${(d > 0 ? t.error : t.success)(signed(d))}` : ''}`;
  console.log(`  ${t.accent('OPEN TODAY')}        ${today(day.openCards, day.openCardsDelta)} cards ${t.border('·')} ${today(day.openCriteria, day.openCriteriaDelta)} criteria ${t.dim(`since the start of ${day.date}`)}`);
  console.log(`\n  ${t.error('BLOCKED ON OWNER')}  ${t.warn(String(s.counts.decide))} decisions`);
  console.log(`  ${t.success('AGENT-READY')}       ${t.success(String(s.counts.agentReady))}  ${t.dim('(review / build / implement / plan)')}`);
  console.log(`  ${t.dim('open questions')}    ${t.warn(String(s.counts.openQuestions))}   ${t.dim(`sidequests ${s.counts.sidequests}   ideas ${s.counts.ideas}`)}\n`);
  const show = (label, lane) => {
    const cs = s.cards.filter(c => c.lane.lane === lane);
    if (!cs.length) return;
    console.log(`  ${t.accent(label)}${t.border(':')}`);
    for (const c of cs.slice(0, 12)) {
      const columns = process.stdout.columns || 80;
      const number = String(c.num).padEnd(4);
      const priority = (c.priority || '').padEnd(3);
      const lanePadded = displayLane(lane).padEnd(9);
      const prefix = `   · #${number} ${priority} ${lanePadded} `;
      const title = clip(c.title, columns - displayWidth(prefix));
      const laneText = (lane === 'decide' ? t.error : lane === 'verify' ? t.warn : t.success)(displayLane(lane).padEnd(9));
      console.log(`   ${t.border('·')} #${number} ${c.priority ? t.warn(priority) : priority} ${laneText} ${title}`);
    }
  };
  show('OWNER — decide', 'decide');
  show('AGENT — review', 'verify'); show('AGENT — building', 'building');
  show('AGENT — implement', 'implement'); show('AGENT — plan', 'plan');
  console.log('');
}

function cmdBriefing(store, { pos, flags }) {
  const [verb, id] = pos;
  if (verb === 'post') {
    const body = readFileSync(flags.file === '-' ? 0 : flags.file, 'utf8');
    const sections = flags.sections ? JSON.parse(readFileSync(flags.sections, 'utf8')) : [];
    const { result } = store.mutate(s => db.postBriefing(s, {
      title: flags.title, body, sections, by: flags.by,
    }), { expectRev: flags.expectRev });
    return out(flags, `posted briefing ${result.id} — ${result.title}`, result);
  }
  const records = store.loadLive().briefings || [];
  if (verb === 'list') {
    if (flags.json) return out(flags, null, records);
    for (const record of records) console.log(`${record.id}  ${record.created}  [${record.by}] ${record.title}`);
    if (!records.length) console.log('(no briefings)');
    return;
  }
  if (verb === 'show') {
    const record = id ? records.find(r => r.id === id) : records[0];
    if (!record) throw new TowerError('E_NOT_FOUND', `no briefing ${id || '(latest)'}`);
    return out(flags, null, record);
  }
  throw new TowerError('E_USAGE', 'briefing requires post/list/show');
}

function cmdStatusReport(store, { pos, flags }) {
  if (pos[0] === 'post') {
    const snapshot = readPayload(flags);
    const { result } = store.mutate(s => db.postStatus(s, { snapshot, by: flags.by }),
      { expectRev: flags.expectRev });
    return out(flags, `posted status ${result.id}`, result);
  }
  const record = store.loadLive().statusSnapshot;
  if (!record) throw new TowerError('E_NOT_FOUND', 'no status snapshot posted');
  return out(flags, null, record);
}

const HARDENING_CARD_FLAGS = [
  'hardeningDedupKey', 'hardeningSeam', 'hardeningRelation', 'hardeningWrongTierMask',
  'hardeningInputPartition', 'hardeningFindingId', 'hardeningFixture',
];

function cardPayload(flags, payload, by) {
  const result = { ...(payload || {}), by };
  for (const key of ['title', 'body', 'probe', ...HARDENING_CARD_FLAGS])
    if (flags[key] !== undefined) result[key] = flags[key];
  return result;
}

function cmdCard(store, { pos, flags }) {
  const [verb, ref] = pos;
  const by = flags.by;
  switch (verb) {
    case 'list': {
      const s = store.project();
      const allCards = Array.isArray(s.cards) ? s.cards : [];
      let cs = allCards;
      if (flags.lane) cs = cs.filter(c => c.lane.lane === flags.lane);
      if (flags.epoch) cs = cs.filter(c => c.epoch === flags.epoch);
      if (flags.track) cs = cs.filter(c => c.track === flags.track);
      if (flags.phase) cs = cs.filter(c => c.phase === flags.phase);
      if (flags.kind) cs = cs.filter(c => c.kind === flags.kind);
      if (flags.milestone) cs = cs.filter(c => c.milestoneId === flags.milestone);
      if (flags.tag) {
        const want = String(flags.tag).split(',').map(t => t.trim()).filter(Boolean);
        cs = cs.filter(c => want.every(t => (c.tags || []).includes(t)));
      }
      if (flags.untagged) cs = cs.filter(c => !(c.tags || []).length);
      if (flags.parent) {
        const parent = db.findCard({ cards: allCards }, flags.parent);
        if (!parent) throw new TowerError('E_NOT_FOUND', `no card ${flags.parent}`);
        cs = cs.filter(c => c.parentId === parent.id);
      }
      if (flags.hardeningDedupKey !== undefined)
        cs = cs.filter(c => c.hardeningDedupKey === String(flags.hardeningDedupKey)
          || (c.hardeningDedupAliases || []).includes(String(flags.hardeningDedupKey)));
      const sort = sortArg(flags);
      if (sort) cs = sortCards(cs, sort, store.config.priorities);
      if (flags.json) return out(flags, null, cs);
      for (const c of cs) console.log(cardLine(c));
      if (!cs.length) console.log('(no cards match)');
      return;
    }
    case 'show': {
      const s = store.project();
      const lookup = flags.hardeningDedupKey !== undefined ? flags.hardeningDedupKey : ref;
      const hardeningMatch = flags.hardeningDedupKey !== undefined
        ? db.findHardeningMatch(s, store.loadHistory(), lookup)
        : null;
      const c = hardeningMatch?.card || db.findCard({ cards: s.cards }, lookup);
      if (c && !hardeningMatch?.archived) return out(flags, null, s.cards.find(x => x.id === c.id));
      // #461: fall through to history once it's not live any more.
      const arch = hardeningMatch?.archived ? hardeningMatch.card : db.findInHistory(store.loadHistory(), lookup);
      if (arch) return out(flags, null, { ...arch, archived: true });
      throw new TowerError('E_NOT_FOUND', `no card ${lookup}`);
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const hardening = cardPayload(flags, p, by);
      if (hasHardeningPayload(hardening)) {
        if (flags.force) throw new TowerError('E_USAGE', 'hardening card writes cannot use --force');
        const { result } = store.mutate((s, cfg, history) => db.addOrUpdateHardeningCard(s, hardening, cfg, history));
        const action = result.action === 'added' ? 'added' : `${result.action} existing`;
        return out(flags, `${action} hardening card #${result.card.num}`, result.card);
      }
      const title = flags.title ?? p.title;
      const body = flags.body ?? p.body;
      const candidates = !flags.force
        ? findDuplicateCandidates(store.load().cards, { title, body })
        : [];
      if (candidates.length) {
        const message = [
          'possible duplicate cards (pass --force to create anyway):',
          ...candidates.map(c => `  ${c.id ?? '(no id)'}  #${c.num ?? '?'}  [${c.phase}]  ${c.title}`),
        ].join('\n');
        if (flags.json) {
          console.log(JSON.stringify({ error: { code: 'E_DUPLICATE', message, candidates } }, null, 2));
          process.exitCode = 1;
          return;
        }
        throw new TowerError('E_DUPLICATE', message);
      }
      const tags = flags.addTag || flags.tags
        ? String(flags.addTag || flags.tags).split(',')
        : p.tags;
      const { result } = store.mutate((s, cfg) => db.addCard(s, {
        title, body, kind: flags.kind ?? p.kind,
        track: flags.track ?? p.track, epoch: flags.epoch ?? p.epoch, milestoneId: flags.milestone ?? p.milestoneId,
        phase: flags.phase ?? p.phase, priority: flags.priority ?? p.priority, plan: flags.plan ?? p.plan,
        checkSteps: flags.checkSteps ?? p.checkSteps, probe: flags.probe ?? p.probe,
        blockedBy: flags.blockedBy ? String(flags.blockedBy).split(',') : p.blockedBy,
        criteria: p.criteria, overLimit: flags.overLimit ?? p.overLimit,
        refs: flags.refs ? String(flags.refs).split(',').map(x => x.trim()).filter(Boolean) : p.refs,
        tags,
        parent: flags.parent ?? p.parentId ?? p.parent,
        workOrder: flags.workOrder ?? p.workOrder, by,
        needsAcceptance: flags.needsAcceptance ?? p.needsAcceptance,
      }, cfg));
      return out(flags, `added card #${result.num}`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      const hardening = cardPayload(flags, p, by);
      if (hasHardeningPayload(hardening)) {
        const { result } = store.mutate((s, cfg, history) => db.updateHardeningCard(s, ref, hardening, cfg, history), { expectRev: flags.expectRev });
        return out(flags, `updated hardening card #${result.card.num}`, result.card);
      }
      const patch = { ...p, by };
      for (const [f, k] of [['title', 'title'], ['body', 'body'], ['kind', 'kind'], ['track', 'track'], ['epoch', 'epoch'],
        ['milestone', 'milestoneId'], ['phase', 'phase'], ['priority', 'priority'], ['plan', 'plan'],
        ['checkSteps', 'checkSteps'], ['probe', 'probe'],
        ['workOrder', 'workOrder'], ['log', 'logEntry'], ['needsAcceptance', 'needsAcceptance']])
        if (flags[f] !== undefined) patch[k] = flags[f];
      if (flags.overLimit !== undefined) patch.overLimit = flags.overLimit;
      if (flags.blockedBy !== undefined) patch.blockedBy = flags.blockedBy === '' ? [] : String(flags.blockedBy).split(',');
      if (flags.refs !== undefined) patch.refs = flags.refs === '' ? [] : String(flags.refs).split(',').map(x => x.trim()).filter(Boolean);
      if (flags.tags !== undefined) patch.tags = flags.tags === '' ? [] : String(flags.tags).split(',');
      if (flags.addTag !== undefined) patch.addTags = String(flags.addTag).split(',');
      if (flags.removeTag !== undefined) patch.removeTags = String(flags.removeTag).split(',');
      if (flags.parent !== undefined) patch.parent = flags.parent;
      const current = db.findCard(store.load(), ref);
      const openAcceptance = current && store.load().decisions.find(d => d.cardId === current.id && d.group === 'acceptance' && d.status !== 'ratified');
      const clearsAcceptance = 'needsAcceptance' in patch && !(patch.needsAcceptance === true || patch.needsAcceptance === 'true');
      if (current && ((current.needsAcceptance && patch.phase === 'done' && by === 'owner') || (openAcceptance && clearsAcceptance))) {
        const id = openAcceptance?.id || `D-ACCEPT-${current.num}`;
        store.mutate((s) => db.auditAcceptanceRejection(s, id, 'cli card update', 'owner-verification bypass rejected', by));
        throw new TowerError('E_ACCEPTANCE_OWNER_UI', `card #${current.num} requires the dedicated owner verification UI`);
      }
      const { result, state } = store.mutate((s, cfg) => db.updateCard(s, ref, patch, cfg), { expectRev: flags.expectRev });
      return out(flags, `updated card #${result.num} → ${displayLane(db.laneOf(result, state.decisions, state.cards).lane)}`, result);
    }
    case 'criteria': {
      if (flags.reopen !== undefined) {
        const { result } = store.mutate((s) => db.reopenCriterion(s, ref, flags.reopen, { reason: flags.reason, by }));
        return out(flags, 'criterion #' + result.n + ' reopened on card #' + result.cardNum, result);
      }
      if (flags.proof === true) throw new TowerError('E_USAGE', '--proof takes a jet test command, or "" to clear it');
      if (flags.blockedBy === true) throw new TowerError('E_USAGE', "--blocked-by takes '#M', '#M cK' or a decision id");
      if (flags.unblock !== undefined) {
        const { result } = store.mutate((s) => db.unblockCriterion(s, ref, flags.unblock, { reason: flags.reason, by }));
        return out(flags, `criterion #${result.n} on card #${result.cardNum} no longer waits on ${result.unblockedFrom}`, result);
      }
      if (flags.edit !== undefined) {
        if (typeof flags.text !== 'string' && flags.proof === undefined && flags.blockedBy === undefined)
          throw new TowerError('E_USAGE', 'card criteria --edit n needs --text "new text" --reason "…", --proof "jet test …" or --blocked-by REF');
        const { result } = store.mutate((s) => db.editCriterion(s, ref, flags.edit, { text: flags.text, proof: flags.proof, blockedBy: flags.blockedBy, reason: flags.reason, by }));
        return out(flags, `criterion #${result.n} edited on card #${result.cardNum}${result.reopened ? ' (reopened: its evidence proved the old wording)' : ''}`, result);
      }
      if (flags.remove !== undefined) {
        const { result } = store.mutate((s) => db.removeCriterion(s, ref, flags.remove, { reason: flags.reason, by }));
        return out(flags, `criterion #${result.n} removed from card #${result.cardNum}`, result);
      }
      if (flags.text !== undefined) throw new TowerError('E_USAGE', 'card criteria --text goes with --edit n');
      if (flags.proof !== undefined && flags.add === undefined) throw new TowerError('E_USAGE', 'card criteria --proof goes with --add or --edit n');
      if (flags.blockedBy !== undefined && flags.add === undefined) throw new TowerError('E_USAGE', 'card criteria --blocked-by goes with --add or --edit n');
      if (flags.to !== undefined && flags.move === undefined) throw new TowerError('E_USAGE', 'card criteria --to goes with --move n');
      if (flags.move !== undefined) {
        if (flags.to === undefined) throw new TowerError('E_USAGE', 'card criteria --move n needs --to <card>');
        const { result } = store.mutate((s, cfg) => db.moveCriterion(s, ref, flags.move, { to: flags.to, reason: flags.reason, overLimit: flags.overLimit, by }, cfg));
        return out(flags, `criterion #${result.from.n} moved from card #${result.from.cardNum} to card #${result.to.cardNum} as c${result.to.n}`, result);
      }
      if (flags.add !== undefined) {
        const { result } = store.mutate((s, cfg) => db.addCriterion(s, ref, flags.add, by, { proof: flags.proof, blockedBy: flags.blockedBy, overLimit: flags.overLimit }, cfg));
        return out(flags, `added criterion #${result.n} to card #${result.cardNum}${result.blockedBy ? `, waiting on ${result.blockedBy}` : ''}`, result);
      }
      if (flags.kind !== undefined && flags.meet === undefined && flags.verify === undefined)
        throw new TowerError('E_USAGE', 'card criteria --kind goes with --meet or --verify');
      if (flags.meet !== undefined) {
        const { result } = store.mutate((s, cfg, history) => db.meetCriterion(s, ref, flags.meet, { evidence: flags.evidence, by, kind: flags.kind }, cfg, history));
        return out(flags, `criterion #${result.n} met on card #${result.cardNum}`, result);
      }
      if (flags.verify !== undefined) {
        const { result } = store.mutate((s, cfg, history) => db.verifyCriterion(s, ref, flags.verify, { evidence: flags.evidence, by, kind: flags.kind }, cfg, history));
        return out(flags, `criterion #${result.n} verified on card #${result.cardNum}`, result);
      }
      const s = store.load();
      const found = db.findCard(s, ref) || (() => { throw new TowerError('E_NOT_FOUND', `no card ${ref}`); })();
      if (flags.json) return out(flags, null, found.criteria || []);
      for (const it of found.criteria || []) console.log(criterionLine(it));
      if (!(found.criteria || []).length) console.log('(no criteria — an agent cannot close this card)');
      return;
    }
    case 'claim': {
      const { result } = store.mutate((s) => db.claimCard(s, ref, by));
      return out(flags, `card #${result.num} claimed by ${by}`, result);
    }
    case 'release': {
      const { result } = store.mutate((s) => db.releaseCard(s, ref, by, flags.handoff));
      return out(flags, `card #${result.num} released`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteCard(s, ref, { by }));
      const ballots = result.decisions.length ? ` with ballot${result.decisions.length > 1 ? 's' : ''} ${result.decisions.map(d => `${d.id} (${d.status})`).join(', ')}` : '';
      return out(flags, `deleted card #${result.num}${ballots}`, result);
    }
    case 'split': {
      const { result } = store.mutate((s, cfg) => db.splitCard(s, ref, { criteria: flags.criteria, title: flags.title, overLimit: flags.overLimit, by }, cfg));
      return out(flags, `split ${result.moved.map(m => `c${m.from}`).join(', ')} off card #${result.from.num} into card #${result.card.num} (${result.moved.map(m => `c${m.to}`).join(', ')})`, result);
    }
    case 'log': {
      const { result } = store.mutate((s, cfg) => db.updateCard(s, ref, {
        logEntry: flags.text.trim(), by,
      }, cfg));
      return out(flags, `logged card #${result.num}`, result);
    }
    case 'history': {
      const { state, history } = store.loadPair();
      const found = db.cardHistory(state, history, ref);
      const events = found.events.slice(0, Number(flags.limit || found.events.length));
      if (flags.json) return out(flags, null, { ...found, events });
      console.log(`#${found.card.num} ${found.card.title}${found.card.archived ? '  (archived)' : ''}`);
      for (const e of events) console.log(`${e.at}  ${String(e.by || '').padEnd(10)} ${e.action.padEnd(22)} ${e.note || ''}`);
      if (!events.length) console.log('(no events)');
      return;
    }
    default: throw new TowerError('E_USAGE', `unknown card verb "${verb}" — list/show/add/update/claim/release/delete/criteria/log`);
  }
}

const NAMED_FILE_RE = /(?:^|[\s"'`(])((?:\/|\.\/|(?:docs|examples|Source|crates|tests|Tower)\/)[^\s"'`),;]+)/g;

function citedEvidence(card, root) {
  const refs = Array.isArray(card.refs) ? card.refs : [];
  const named = [...`${card.body || ''}\n${card.plan || ''}`.matchAll(NAMED_FILE_RE)].map(m => m[1]);
  for (const raw of [...refs, ...named]) {
    const name = String(raw || '').trim().replace(/[.,;:!?]+$/, '').split('#', 1)[0];
    if (!name || /^https?:\/\//.test(name)) continue;
    const path = name.startsWith('/') ? name : resolve(root, name);
    try {
      if (!statSync(path).isFile()) continue;
      const text = readFileSync(path, 'utf8').trim();
      if (text) return text.slice(0, 64 * 1024);
    } catch { /* a non-local citation is not scaffold evidence */ }
  }
  return '';
}

function runCardProbe(card, root) {
  const command = typeof card.probe === 'string' ? card.probe.trim() : '';
  if (!command) throw new TowerError('E_PROBE', `card #${card.num} has no probe — add --probe "jet run …" before scaffolding`);
  const result = spawnSync('/bin/sh', ['-c', command], {
    cwd: root,
    env: process.env,
    encoding: 'utf8',
    timeout: 120000,
    maxBuffer: 64 * 1024,
  });
  if (result.error) throw new TowerError('E_PROBE', `card #${card.num} probe failed: ${result.error.message}`);
  return `${result.stdout || ''}${result.stderr || ''}`.trim();
}

function scaffoldDecision(store, ref, flags) {
  const id = flags.id.trim();
  const state = store.load();
  const card = db.findCard(state, ref);
  if (!card) throw new TowerError('E_NOT_FOUND', `no card ${ref}`);
  const root = projectRoot(store.dataDir) || process.cwd();
  const currentCode = runCardProbe(card, root);
  const wildCode = citedEvidence(card, root);
  const emptyRecommendation = () => ({ rec: '', why: '', gains: [], losses: [], whyNot: [], tradeoff: '' });
  // Start loss-free; add only negligible remaining costs as { loss, mitigation }.
  const draft = {
    id, cardId: card.id, title: card.title, group: 'other', ballotMode: 'full',
    shortAuthorizedBy: null, ballotProcessVersion: 5, draft: true, status: 'open',
    situation: '', gist: '', lesson: '', explainer: '', story: '', inWild: '', detail: '', rec: '',
    options: [], comparisons: [], recommendation: emptyRecommendation(), hybrid: null,
    reviewPasses: { adversarial: '' }, checkInstructions: null,
    surface: {
      gist: '', lesson: '',
      trio: {
        current: { note: '', code: currentCode },
        wild: { lang: '', note: '', code: wildCode },
      },
      options: [],
      recommendation: emptyRecommendation(),
    },
  };
  const safeId = id.replace(/[^\w.-]+/g, '-');
  const outputPath = typeof flags.out === 'string' && flags.out.trim()
    ? resolve(flags.out)
    : join(process.env.XDG_CACHE_HOME || join(process.env.HOME || process.cwd(), '.cache'), 'jet-luna', `tower-ballot-${safeId}.json`);
  if (resolve(outputPath) === resolve(store.file))
    throw new TowerError('E_USAGE', 'decision scaffold cannot overwrite tower.json');
  mkdirSync(dirname(outputPath), { recursive: true, mode: 0o700 });
  writeFileSync(outputPath, `${JSON.stringify(draft, null, 2)}\n`, { mode: 0o600 });
  return out(flags, `scaffolded ballot ${id} → ${outputPath}\nKeep recommendation.losses empty, or add {loss, mitigation} with a concrete way to reduce each cost.`, draft);
}

function cmdDecision(store, { pos, flags }) {
  const [verb, id] = pos;
  const by = flags.by;
  switch (verb) {
    case 'list': {
      const s = store.load();
      let ds = s.decisions.filter(d => flags.drafts || !d.draft);
      if (flags.open) ds = ds.filter(d => d.status !== 'ratified');
      if (flags.card) { const c = db.findCard(s, flags.card); ds = ds.filter(d => c && d.cardId === c.id); }
      if (flags.json) return out(flags, null, ds);
      for (const d of ds) console.log(`${String(d.id || '(no id)').padEnd(16)} ${db.decisionStage(d).padEnd(9)} ${String(d.title || '(untitled)').slice(0, 60)}${d.outcome ? ` → ${d.outcome}` : ''}`);
      if (!ds.length) console.log('(no decisions match)');
      return;
    }
    case 'show': {
      const s = store.load();
      const d = s.decisions.find(x => x.id === id);
      if (d) return out(flags, null, d);
      // #461: fall through to history once it's not live any more.
      const arch = store.loadHistory().decisions.find(x => x.id === id);
      if (arch) return out(flags, null, { ...arch, archived: true });
      throw new TowerError('E_NOT_FOUND', `no decision ${id}`);
    }
    case 'scaffold':
      return scaffoldDecision(store, id, flags);
    case 'add': {
      const p = readPayload(flags) || {};
      const payload = { ...p, by };
      for (const f of ['id', 'cardId', 'title', 'situation', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group', 'ballotMode', 'shortAuthorizedBy'])
        if (flags[f] !== undefined) payload[f] = flags[f];
      if (flags.card !== undefined) payload.cardId = flags.card;
      if (flags.draft !== undefined) payload.draft = flags.draft === true || flags.draft === 'true';
      if (flags.reviewedBy !== undefined) payload.reviewedBy = flags.reviewedBy;
      const { result } = store.mutate((s) => db.addDecision(s, payload));
      return out(flags, `added decision ${result.id} on card #${result.cardNum} (${db.decisionStage(result)})`, result);
    }
    case 'update': {
      const p = readPayload(flags) || {};
      for (const f of ['title', 'situation', 'gist', 'lesson', 'story', 'explainer', 'inWild', 'detail', 'rec', 'group', 'ballotMode', 'shortAuthorizedBy'])
        if (flags[f] !== undefined) p[f] = flags[f];
      const { result } = store.mutate((s) => db.updateDecision(s, id, p, by));
      return out(flags, `updated decision ${result.id} (${db.decisionStage(result)})`, result);
    }
    case 'review': {
      const { result } = store.mutate((s) => db.reviewDecision(s, id, { by, reject: flags.reject === true, reason: flags.reason }));
      return out(flags, result.status === 'rejected' ? `rejected ${result.id}: ${result.rejectReason}` : `reviewed ${result.id} — post it with decision post ${result.id}`, result);
    }
    case 'post': {
      const { result } = store.mutate((s) => db.postDecision(s, id, by));
      return out(flags, `posted ${result.id} to the owner (reviewed by ${result.reviewedBy})`, result);
    }
    case 'report': {
      // Link a report (a Tower doc or an https source) to a ballot, or unlink one.
      const s = store.load();
      const d = s.decisions.find(x => x.id === id);
      if (!d) throw new TowerError('E_NOT_FOUND', `no decision ${id}`);
      let reports = [...(d.reports || [])];
      if (typeof flags.remove === 'string') {
        const key = flags.remove.trim();
        if (!reports.some(r => (r.path || r.url) === key)) throw new TowerError('E_NOT_FOUND', `${id} links no report ${key}`);
        reports = reports.filter(r => (r.path || r.url) !== key);
      } else {
        const path = typeof flags.path === 'string' ? flags.path.trim() : '';
        const url = typeof flags.url === 'string' ? flags.url.trim() : '';
        if (typeof flags.title !== 'string' || (!path && !url))
          throw new TowerError('E_USAGE', 'decision report needs --title and one of --path Docs/… or --url https://…, or --remove PATH|URL');
        if (path) {
          const root = projectRoot(store.dataDir) || process.cwd();
          let isFile = false;
          try { isFile = statSync(resolve(root, path)).isFile(); } catch { /* missing */ }
          if (!isFile) throw new TowerError('E_NOT_FOUND', `report ${path} is not a file in this checkout; add it with tower docs add first`);
        }
        const entry = { title: flags.title, ...(path ? { path } : { url }), ...(typeof flags.note === 'string' ? { note: flags.note } : {}) };
        const key = path || url;
        const at = reports.findIndex(r => (r.path || r.url) === key);
        if (at >= 0) reports[at] = entry; else reports.push(entry);
      }
      const { result } = store.mutate((st) => db.updateDecision(st, id, { reports }, by));
      return out(flags, `${id} links ${(result.reports || []).length} report(s)`, result);
    }
    case 'ratify': {
      const decision = store.load().decisions.find(d => d.id === id);
      if (decision && (decision.group === 'acceptance' || decision.id.startsWith('D-ACCEPT-'))) {
        store.mutate((s) => db.auditAcceptanceRejection(s, id, 'cli decision ratify',
          'CLI cannot resolve owner verification', by));
        throw new TowerError('E_ACCEPTANCE_OWNER_UI', `${id} requires the dedicated owner verification UI; --by owner and --quote are not accepted`);
      }
      const { result } = store.mutate((s) => db.ratify(s, id, flags.outcome, flags.comment, by, flags.quote), { expectRev: flags.expectRev });
      return out(flags, `ratified ${result.id} → ${result.outcome}`, result);
    }
    case 'reopen': {
      const { result } = store.mutate((s) => db.reopenDecision(s, id, by));
      return out(flags, `reopened ${result.id}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteDecision(s, id, by));
      return out(flags, `deleted decision ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown decision verb "${verb}" — list/show/scaffold/add/update/review/post/report/ratify/reopen/delete`);
  }
}

function cmdQuestion(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      let qs = s.questions.filter(q => q.kind !== 'message');
      if (flags.open) qs = qs.filter(q => q.status === 'open');
      if (flags.card) { const c = db.findCard(s, flags.card); qs = qs.filter(q => c && q.cardId === c.id); }
      if (flags.json) return out(flags, null, qs);
      for (const q of qs) console.log(`${q.id.padEnd(14)} ${(q.status || '').padEnd(9)} [${q.by}] ${q.text.slice(0, 70)}`);
      if (!qs.length) console.log('(no questions match)');
      return;
    }
    case 'ask': {
      const { result } = store.mutate((s) => db.addQuestion(s, { cardId: id, text: flags.text, by: flags.by || 'owner', kind: flags.kind, decisionId: flags.decision }));
      return out(flags, `asked ${result.id} on card #${result.cardNum}`, result);
    }
    case 'answer': {
      const { result } = store.mutate((s) => db.answerQuestion(s, id, flags.text, flags.by));
      return out(flags, `answered ${result.id}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteQuestion(s, id, flags.by));
      return out(flags, `deleted question ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown question verb "${verb}" — list/ask/answer/delete`);
  }
}

function cmdMessage(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const messages = db.listMessages(store.load(), {
        cardId: flags.card,
        status: flags.all ? null : (flags.status || 'open'),
      });
      if (flags.json) return out(flags, null, messages);
      for (const message of messages)
        console.log(`${message.id.padEnd(14)} ${(message.status || '').padEnd(9)} [${message.by}] ${message.text.slice(0, 70)}`);
      if (!messages.length) console.log('(no messages match)');
      return;
    }
    case 'add': {
      const { result } = store.mutate((s) => db.addMessage(s, {
        cardId: id,
        text: flags.text,
        by: flags.by,
      }));
      return out(flags, `added message ${result.id} on card #${result.cardNum}`, result);
    }
    case 'done': {
      const { result } = store.mutate((s) => db.doneMessage(s, id, flags.by));
      return out(flags, `marked message ${result.id} done`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown message verb "${verb}" — list/add/done`);
  }
}

function cmdIdea(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      const bs = flags.all ? s.ideas : s.ideas.filter(b => b.status !== 'tagged');
      if (flags.json) return out(flags, null, bs);
      for (const b of bs) console.log(`${b.id.padEnd(14)} ${b.text.slice(0, 70)}`);
      if (!bs.length) console.log('(no ideas)');
      return;
    }
    case 'add': {
      const { result } = store.mutate((s) => db.addIdea(s, { text: flags.text ?? pos.slice(1).join(' '), note: flags.note, tags: flags.tags ? String(flags.tags).split(',') : [], by: flags.by }));
      return out(flags, `captured idea ${result.id}`, result);
    }
    case 'promote': {
      const { result } = store.mutate((s, cfg) => db.promoteIdea(s, id, { title: flags.title, body: flags.body, kind: flags.kind, track: flags.track, priority: flags.priority, by: flags.by }, cfg));
      return out(flags, `promoted → card #${result.num}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteIdea(s, id, flags.by));
      return out(flags, `deleted idea ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown idea verb "${verb}" — list/add/promote/delete`);
  }
}

function cmdEpoch(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.load();
      const epochs = sortEpochs(s.epochs);
      if (flags.json) return out(flags, null, epochs);
      for (const e of epochs) {
        console.log(`${(e.status || 'open').padEnd(9)} ${String(e.order ?? '—').padStart(3)}  ${epochLabel(e)}${e.status === 'active' ? '  ← current' : ''}`);
        for (const criterion of e.exitCriteria || []) console.log(`               exit: ${criterion}`);
      }
      if (!epochs.length) console.log('(no epochs — tower epoch add e1 --name "..." --order 1)');
      return;
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const { result } = store.mutate((s) => db.addEpoch(s, {
        ...p, id: id ?? p.id, name: flags.name ?? p.name, goal: flags.goal ?? p.goal, status: flags.status ?? p.status,
        order: flags.order ?? p.order, exitCriteria: flags.exitCriteria ?? p.exitCriteria, by: flags.by,
      }));
      return out(flags, `added epoch ${epochLabel(result)}`, result);
    }
    case 'update': {
      const patch = { ...(readPayload(flags) || {}) };
      for (const f of ['name', 'goal', 'status', 'order', 'exitCriteria']) if (flags[f] !== undefined) patch[f] = flags[f];
      const { result } = store.mutate((s) => db.updateEpoch(s, id, { ...patch, by: flags.by }));
      return out(flags, `updated epoch ${epochLabel(result)}`, result);
    }
    case 'remove': {
      const { result } = store.mutate((s, _config, history) => db.removeEpoch(s, history, id, flags.by));
      return out(flags, `removed epoch ${result.removed}`, result);
    }
    case 'current': {
      const { result } = store.mutate((s) => db.setCurrentEpoch(s, id === 'none' ? null : id, flags.by));
      return out(flags, `current ${store.config.terms.epoch.toLowerCase()}: ${result.active || '—'}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown epoch verb "${verb}" — list/add/update/remove/current`);
  }
}

function cmdMilestone(store, { pos, flags }) {
  const [verb, id] = pos;
  switch (verb) {
    case 'list': {
      const s = store.project();
      const rank = new Map(sortEpochs(s.epochs).map((e, i) => [e.id, i]));
      let ms = sortMilestones(s.milestones).sort((a, b) => ((rank.get(a.epochId) ?? Infinity) - (rank.get(b.epochId) ?? Infinity)) || 0);
      if (flags.epoch) ms = ms.filter(m => m.epochId === flags.epoch);
      if (!flags.retired) ms = ms.filter(m => m.status !== 'retired');
      if (flags.json) return out(flags, null, ms);
      for (const m of ms) console.log(`${m.id.padEnd(12)} ${epochTag(s, m.epochId).padEnd(4)} ${(m.status || 'open').padEnd(8)} ${m.progress.done}/${m.progress.total}  ${m.title.slice(0, 50)}${m.status === 'retired' ? `  [retired: ${m.retired?.reason || '—'}]` : ''}`);
      if (!ms.length) console.log('(no milestones)');
      return;
    }
    case 'add': {
      const p = readPayload(flags) || {};
      const { result } = store.mutate((s) => db.addMilestone(s, {
        ...p, id: flags.id ?? p.id, epochId: flags.epoch ?? p.epochId, title: flags.title ?? p.title, goal: flags.goal ?? p.goal,
        order: flags.order ?? p.order, exitCriteria: flags.exitCriteria ?? p.exitCriteria, by: flags.by,
      }));
      return out(flags, `added milestone ${result.id} in ${result.epochId} — ${result.title}`, result);
    }
    case 'retire': {
      const { result } = store.mutate((s) => db.retireMilestone(s, id, { reason: flags.reason, by: flags.by }));
      return out(flags, `retired milestone ${result.id} — ${result.retired.reason}`, result);
    }
    case 'criteria': {
      if (flags.reopen !== undefined) {
        const { result } = store.mutate((s) => db.reopenMilestoneCriterion(s, id, flags.reopen, { reason: flags.reason, by: flags.by }));
        return out(flags, `criterion #${result.n} reopened on milestone ${id}`, result);
      }
      if (flags.edit !== undefined) {
        if (typeof flags.text !== 'string') throw new TowerError('E_USAGE', 'milestone criteria --edit n needs --text "new text" and --reason "…"');
        const { result } = store.mutate((s) => db.editMilestoneCriterion(s, id, flags.edit, { text: flags.text, reason: flags.reason, by: flags.by }));
        return out(flags, `criterion #${result.n} edited on milestone ${id}${result.reopened ? ' (reopened: its evidence proved the old wording)' : ''}`, result);
      }
      if (flags.remove !== undefined) {
        const { result } = store.mutate((s) => db.removeMilestoneCriterion(s, id, flags.remove, { reason: flags.reason, by: flags.by }));
        return out(flags, `criterion #${result.n} removed from milestone ${id}`, result);
      }
      if (flags.text !== undefined) throw new TowerError('E_USAGE', 'milestone criteria --text goes with --edit n');
      if (flags.add !== undefined) {
        const { result } = store.mutate((s) => db.addMilestoneCriterion(s, id, flags.add, flags.by));
        return out(flags, `added criterion #${result.n} to milestone ${id}`, result);
      }
      if (flags.meet !== undefined) {
        const { result } = store.mutate((s, cfg, history) => db.meetMilestoneCriterion(s, id, flags.meet, { evidence: flags.evidence, by: flags.by, kind: flags.kind }, cfg, history));
        return out(flags, `criterion #${result.n} met on milestone ${id}`, result);
      }
      if (flags.verify !== undefined) {
        const { result } = store.mutate((s, cfg, history) => db.verifyMilestoneCriterion(s, id, flags.verify, { evidence: flags.evidence, by: flags.by, kind: flags.kind }, cfg, history));
        return out(flags, `criterion #${result.n} verified on milestone ${id}`, result);
      }
      const found = store.load().milestones.find(m => m.id === id) || (() => { throw new TowerError('E_NOT_FOUND', `no milestone ${id}`); })();
      return out(flags, null, found.exitCriteria);
    }
    case 'closeout': {
      const { result } = store.mutate(
        (s, cfg, history) => db.beginMilestoneCloseout(s, id, { sourceCommit: flags.commit, by: flags.by }, history.cards),
      );
      return out(flags, `opened closeout for milestone ${result.id} at ${result.closeout.sourceCommit}`, result);
    }
    case 'verify': {
      const { result } = store.mutate((s, cfg, history) => db.verifyMilestone(s, id, { evidence: flags.evidence, by: flags.by }, history.cards), { expectRev: flags.expectRev });
      return out(flags, `verified milestone ${result.id}`, result);
    }
    case 'update': {
      const patch = { ...(readPayload(flags) || {}) };
      for (const [f, k] of [['title', 'title'], ['goal', 'goal'], ['order', 'order'], ['exitCriteria', 'exitCriteria'], ['status', 'status'], ['epoch', 'epochId']])
        if (flags[f] !== undefined) patch[k] = flags[f];
      const { result } = store.mutate((s) => db.updateMilestone(s, id, patch, flags.by));
      return out(flags, `updated milestone ${result.id} in ${result.epochId} — ${result.status}`, result);
    }
    case 'delete': {
      const { result } = store.mutate((s) => db.deleteMilestone(s, id, flags.by));
      return out(flags, `deleted milestone ${id}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown milestone verb "${verb}" — list/add/update/retire/criteria/closeout/verify/delete`);
  }
}

function cmdNext(store, { flags }) {
  const s = store.load();
  const readyAcross = flags.readyAcrossEpochs || flags.parallel;
  const scope = readyAcross ? 'ready-across' : flags.burndown ? 'burndown' : undefined;
  const defaultLimit = readyAcross ? 50 : 5;   // the parallel view is a survey, not a single pick
  const epoch = flags.epoch === undefined ? undefined : epochArg(s, flags.epoch);
  const picks = db.nextCards(s, { epoch, track: flags.track, agent: flags.agent, limit: Number(flags.limit || defaultLimit), scope });
  const proj = db.project(s);
  const rich = picks.map(p => proj.cards.find(c => c.id === p.id));
  if (flags.json) return out(flags, null, rich);
  if (!rich.length) return console.log('(nothing agent-workable — board is either empty, blocked on the owner, or done)');
  const chain = epochChain(s.epochs);
  const [active, following] = ['active', 'next'].map(role => chain.find(e => e.role === role));
  const order = active ? ` — ${active.label} first (${active.staff} agents)${following ? `, then ${following.label} (${following.staff} agent)` : ''}` : '';
  const header = scope === 'ready-across' ? 'ready across all epochs — every card with no unfinished blocker (the parallel-safe set):'
    : scope === 'burndown' ? `next up — burndown scope (${active ? active.label : 'active epoch'} + sidequests):`
    : `next up${order}; review > building > implement > plan → workOrder:`;
  console.log(header);
  for (const c of rich) console.log(` · ${cardLine(c, { epoch: epochTag(s, c.epoch) })}`);
}

// An --epoch value names an epoch by id, case-insensitively (`E18` or `e18`).
function epochArg(s, raw) {
  const found = s.epochs.find(e => e.id.toLowerCase() === String(raw).toLowerCase());
  if (!found) throw new TowerError('E_NOT_FOUND', `no epoch ${raw}`);
  return found.id;
}

// The epoch chain: every epoch in order with its role, staffing, open cards
// and open criteria, headed by the active epoch and the next one.
function cmdChain(store, { flags }) {
  const proj = db.project(store.load());
  const today = proj.counts.today.epochs;
  const chain = epochChain(proj.epochs, proj.cards).map(e => ({
    ...e, openCardsDelta: today[e.id]?.openCardsDelta ?? 0, openCriteriaDelta: today[e.id]?.openCriteriaDelta ?? 0,
  }));
  if (flags.json) return out(flags, null, chain);
  const [active, following] = ['active', 'next'].map(role => chain.find(e => e.role === role));
  console.log(active
    ? `active ${active.label}: ${active.openCriteria} open criteria${active.blockedCriteria ? ` (+${active.blockedCriteria} blocked)` : ''} on ${active.openCards} cards · next ${following ? following.label : '—'}`
    : '(no unfinished epoch)');
  const todayText = (e) => {
    const parts = [[e.openCardsDelta, 'cards'], [e.openCriteriaDelta, 'open']].filter(([d]) => d).map(([d, what]) => `${signed(d)} ${what}`);
    return parts.length ? `  (today ${parts.join(', ')})` : '';
  };
  for (const e of chain) {
    const role = e.role === 'finished' ? `finished (${e.status})` : e.staff ? `${e.role} · ${e.staff} agent${e.staff === 1 ? '' : 's'}` : e.role;
    console.log(` ${e.label.padEnd(5)} ${role.padEnd(18)} ${String(e.openCards).padStart(4)} cards ${String(e.openCriteria).padStart(5)} open${e.blockedCriteria ? ` ${e.blockedCriteria} blocked` : ''}  ${e.name}${todayText(e)}`);
  }
}

const signed = (n) => `${n > 0 ? '+' : ''}${n}`;

// --sort KEY[:asc|desc] takes the card sorts the board offers.
function sortArg(flags) {
  if (flags.sort === undefined) return null;
  const sort = parseSort(flags.sort, CARD_SORTS);
  if (!sort) throw new TowerError('E_USAGE', `--sort takes one of ${CARD_SORTS.map(s => s.key).join(', ')}, optionally with :asc or :desc`);
  return sort;
}

// Open cards nearest to done: fewest workable open criteria first; blocked
// criteria are counted apart and do not count as progress. --sort orders the
// same cards another way.
function cmdClosest(store, { flags }) {
  const proj = db.project(store.load());
  const limit = flags.limit === undefined ? 20 : Number(flags.limit);
  if (!Number.isInteger(limit) || limit < 1) throw new TowerError('E_USAGE', '--limit must be a positive whole number');
  const epoch = flags.epoch === undefined ? null : epochArg(proj, flags.epoch);
  const sort = sortArg(flags);
  const rows = closestCards(proj.cards, proj.epochs, { epoch, limit, sort, priorities: store.config.priorities }).map(({ card, crit }) => ({
    num: card.num, id: card.id, title: card.title, epoch: card.epoch, phase: card.phase,
    open: crit.open, blocked: crit.blocked, settled: crit.settled, total: crit.total,
  }));
  if (flags.json) return out(flags, null, rows);
  if (!rows.length) return console.log('(no open cards with criteria)');
  for (const r of rows)
    console.log(` #${String(r.num).padEnd(5)} ${epochTag(proj, r.epoch).padEnd(4)} ${(r.open ? `${r.open} open` : 'ready').padEnd(8)} ${(r.blocked ? `${r.blocked} blocked` : '').padEnd(10)} ${r.title}`);
}

// Every authored ballot by stage, with the cards and criteria waiting on it.
function cmdBallots(store, { flags }) {
  const stages = db.ballotsReport(store.load(), store.config);
  if (flags.json) return out(flags, null, stages);
  for (const { stage, count, ballots } of stages) {
    console.log(`${stage} (${count}${stage === 'ratified' && ballots.length < count ? `, ${ballots.length} recent shown` : ''})`);
    for (const b of ballots) {
      const who = stage === 'drafted' ? b.draftedBy : stage === 'ratified' ? `→ ${b.outcome}` : b.reviewedBy;
      const waits = [...b.waiting.cards.map(n => `#${n}`), ...b.waiting.criteria];
      console.log(`  ${b.id.padEnd(22)} #${String(b.card ?? '—').padEnd(5)} ${b.title.slice(0, 56)}${who ? `  (${who})` : ''}`
        + (b.rejectReason ? `\n    rejected: ${b.rejectReason}` : '')
        + (waits.length ? `\n    waiting: ${waits.join(', ')}` : ''));
    }
  }
}

// #457 — durability sweeper: rule-based lint over the live board, optionally
// extended with a Docs/Spec/** reference scan (--docs). Read-only; exit 1 on
// any finding so it is CI/pre-flight friendly, 0 clean. `--proofs --tree DIR`
// checks only that every proof filter matches a test file in that checkout.
function cmdLint(store, { flags }) {
  const s = store.load();
  if (flags.tree !== undefined && !flags.proofs) throw new TowerError('E_USAGE', 'lint --tree goes with --proofs');
  if (flags.proofs && typeof flags.tree !== 'string') throw new TowerError('E_USAGE', 'lint --proofs needs --tree DIR, a checkout holding Tests and Examples');
  if ([flags.proofs, flags.evidence, flags.limits].filter(Boolean).length > 1)
    throw new TowerError('E_USAGE', 'lint --proofs, --evidence and --limits are separate sweeps; run one at a time');
  const findings = flags.proofs ? unmatchedProofs(s, resolve(flags.tree))
    : flags.evidence ? evidenceProvenance(s, store.config.durability)
    : flags.limits ? criteriaOverLimit(s, store.config)
    : lint(s, store.loadHistory());
  // --evidence and --limits warn: old evidence stays readable, and a card may
  // stay over the limit for a logged reason, so their findings never fail the run.
  process.exitCode = findings.length && !flags.evidence && !flags.limits ? 1 : 0;
  if (flags.json) return out(flags, null, findings);
  for (const f of findings) console.log(`${f.rule}  ${f.ref}  ${f.msg}`);
  if (!findings.length) console.log('(clean)');
}

// Run the proofs of a card's or an epoch's open criteria and meet the ones
// that pass. Without --yes it prints the plan; with --dry-run it runs the tests
// and reports, but meets nothing.
function cmdProve(store, { flags }) {
  const s = store.load();
  if ((flags.card === undefined) === (flags.epoch === undefined)) throw new TowerError('E_USAGE', 'prove needs one of --card REF or --epoch E');
  const card = flags.card === undefined ? null : db.findCard(s, flags.card) || (() => { throw new TowerError('E_NOT_FOUND', `no card ${flags.card}`); })();
  const epoch = flags.epoch === undefined ? null : epochArg(s, flags.epoch);
  const runs = proveRuns(proveTargets(s, { card, epoch }));
  const scope = card ? `card #${card.num}` : epochTag(s, epoch);
  const planned = runs.map(run => ({ command: run.command, criteria: run.targets.map(({ card: c, item }) => ({ card: c.num, n: item.n, proof: proofCommand(item.proof) })) }));
  if (!flags.yes) {
    if (flags.json) return out(flags, null, { scope, runs: planned });
    console.log(`plan for ${scope}: ${runs.length} run${runs.length === 1 ? '' : 's'} for ${planned.reduce((sum, run) => sum + run.criteria.length, 0)} open criteria with a proof; pass --yes to run them`);
    for (const run of planned) {
      console.log(run.command);
      for (const row of run.criteria) console.log(`  #${row.card} c${row.n}  ${row.proof}`);
    }
    return;
  }
  for (const key of ['jet', 'tree', 'commit']) if (typeof flags[key] !== 'string' || !flags[key]) throw new TowerError('E_USAGE', `prove --yes needs --${key}`);
  if (!flags.dryRun && (typeof flags.by !== 'string' || !flags.by)) throw new TowerError('E_USAGE', 'prove --yes needs --by <agent> to meet the passing criteria');
  const resolver = store.config.durability;
  if (!resolver.active()) throw new TowerError('E_EVIDENCE', 'prove needs a project git work tree with a dev or master branch to check --commit against');
  const [found] = resolver.commits(flags.commit);
  if (!found) throw new TowerError('E_EVIDENCE', `--commit ${flags.commit} names no commit in this repository`);
  if (!found.ref) throw new TowerError('E_EVIDENCE', `commit ${flags.commit} is not on ${resolver.refs().join(' or ')}; land it, then prove the landed commit`);
  const tree = resolve(flags.tree);
  const head = spawnSync('git', ['-C', tree, 'rev-parse', 'HEAD'], { encoding: 'utf8' });
  if (head.status === 0) {
    if (head.stdout.trim() !== found.sha) throw new TowerError('E_EVIDENCE', `--tree ${flags.tree} is at ${head.stdout.trim().slice(0, 10)}, not ${found.sha.slice(0, 10)}; check out the commit you prove`);
    const dirty = spawnSync('git', ['-C', tree, 'status', '--porcelain', '--untracked-files=no'], { encoding: 'utf8' });
    if (dirty.stdout.trim()) throw new TowerError('E_EVIDENCE', `--tree ${flags.tree} has uncommitted changes; prove a clean checkout of ${found.sha.slice(0, 10)}`);
  }
  const sha = found.sha.slice(0, 10);
  const verdicts = executeRuns(runs, { jet: resolve(flags.jet), tree });
  const passing = verdicts.filter(v => v.pass);
  if (passing.length && !flags.dryRun) {
    store.mutate((st, cfg, history) => {
      for (const v of passing) db.meetCriterion(st, v.target.card.id, v.target.item.n, { evidence: proveEvidence(sha, v), by: flags.by }, cfg, history);
    });
  }
  const report = verdicts.map(v => ({ card: v.target.card.num, n: v.target.item.n, pass: v.pass, passed: v.passed, total: v.total, detail: v.detail, evidence: v.pass ? proveEvidence(sha, v) : null }));
  process.exitCode = verdicts.length === passing.length ? 0 : 1;
  if (flags.json) return console.log(JSON.stringify({ scope, commit: found.sha, met: flags.dryRun ? 0 : passing.length, results: report }, null, 2));
  for (const row of report) console.log(`${row.pass ? 'PASS' : 'FAIL'} #${row.card} c${row.n}  ${row.pass ? row.evidence : row.detail}`);
  console.log(flags.dryRun ? `dry run: ${passing.length} of ${report.length} would be met` : `met ${passing.length} of ${report.length} criteria at ${sha}`);
}

function cmdDocs(store, { pos, flags }) {
  const dir = store.dataDir;
  const [verb, ref] = pos;
  switch (verb) {
    case 'list': {
      const index = docs.listDocs(dir);
      if (flags.json) return out(flags, null, index);
      console.log(`scratchpad  ${index.scratch.updated?.slice(0, 10) || 'unsaved'}  ${index.scratch.title}`);
      for (const sec of index.sections) {
        if (!sec.files.length) continue;
        console.log(`\n${sec.label}:`);
        for (const f of sec.files) console.log(`  ${f.path.padEnd(52)} ${f.updated.slice(0, 10)}  ${f.title}`);
      }
      return;
    }
    case 'show': {
      const path = ref || flags.path;
      if (!path && !flags.scratch) throw new TowerError('E_USAGE', 'docs show needs a path or --scratch');
      const n = flags.scratch ? docs.showScratchPad(dir) : docs.showDoc(dir, path);
      return out(flags, n.body, n);
    }
    case 'add': {
      const body = flags.file ? readFileSync(flags.file === '-' ? 0 : flags.file, 'utf8') : (flags.body || '');
      if (!flags.section && !flags.path) throw new TowerError('E_USAGE', 'docs add needs --section spec|audits|research|proposals or --path');
      const n = docs.addDoc(dir, {
        section: flags.section,
        title: flags.title,
        body,
        path: flags.path,
        id: flags.id,
      });
      return out(flags, `added ${n.path}`, n);
    }
    case 'update': {
      const path = ref || flags.path;
      const patch = {};
      if (flags.title !== undefined) patch.title = flags.title;
      if (flags.file) patch.body = readFileSync(flags.file === '-' ? 0 : flags.file, 'utf8');
      else if (flags.body !== undefined) patch.body = flags.body;
      if (flags.scratch) {
        const n = docs.updateScratchPad(dir, patch);
        return out(flags, `updated scratchpad`, n);
      }
      if (!path) throw new TowerError('E_USAGE', 'docs update needs a path or --scratch');
      const n = docs.updateDoc(dir, path, patch);
      return out(flags, `updated ${n.path}`, n);
    }
    case 'delete': {
      const path = ref || flags.path;
      if (!path) throw new TowerError('E_USAGE', 'docs delete needs a path');
      const r = docs.deleteDoc(dir, path);
      return out(flags, `deleted ${path}`, r);
    }
    default: throw new TowerError('E_USAGE', `unknown docs verb "${verb}" — list/show/add/update/delete`);
  }
}

// #462 — tower brief: one-shot agent work packet. No ref → pick the top
// card via the canonical nextCards() picker. --agent + not --no-claim →
// claim it (E_CLAIMED if someone else already holds it; a no-op if the same
// agent already does). No --agent → read-only, never claims.
function cmdBrief(store, { pos, flags }) {
  const [ref] = pos;
  let s = store.load();
  let card;
  if (ref) {
    card = db.findCard(s, ref);
    if (!card) throw new TowerError('E_NOT_FOUND', `no card ${ref}`);
  } else {
    const picks = db.nextCards(s, { agent: flags.agent, limit: 1 });
    if (!picks.length) throw new TowerError('E_NOT_FOUND', 'nothing agent-workable — board is either empty, blocked on the owner, or done');
    card = db.findCard(s, picks[0].id);
  }
  db.assertClosureBarrier(s);
  if (flags.agent && !flags.noClaim) {
    const { state } = store.mutate((s2) => db.claimCard(s2, card.id, flags.agent));
    s = state;
    card = db.findCard(s, card.id);
  }
  const packet = db.buildBrief(s, card.id);
  if (flags.json) return out(flags, null, packet);
  console.log(renderBrief(packet, Theme(flags)));
}

// Compact, one-screen-target human render — sections omitted when empty.
function renderBrief(p, t) {
  const c = p.card;
  const L = [];
  const heading = (text) => t.accent(text);
  L.push(`${t.invert(`#${c.num}`)} ${t.accent(c.title)}`);
  L.push(`  ${t.warn(c.phase)}${c.priority ? ` ${t.border('·')} ${t.warn(c.priority)}` : ''}${c.track ? ` ${t.border('·')} ${t.dim(c.track)}` : ''}${c.workOrder != null ? ` ${t.border('·')} ${t.dim(`workOrder ${c.workOrder}`)}` : ''}`);
  if (c.epoch) L.push(`  ${t.dim(`${c.epoch.label}${c.epoch.role ? ` (${c.epoch.role} in the chain)` : ''}${c.epoch.goal ? `: ${c.epoch.goal}` : ''}`)}`);
  if (c.milestone) L.push(`  ${t.dim(`milestone ${c.milestone.title}${c.milestone.goal ? ` — ${c.milestone.goal}` : ''} [${c.milestone.status || 'open'}]`)}`);
  if (c.body) { L.push('', heading('BODY'), c.body); }
  if (c.plan) { L.push('', heading('PLAN'), c.plan); }
  if (p.blockers.length) {
    L.push('', heading('BLOCKED BY'));
    for (const b of p.blockers) {
      const ref = b.kind === 'card' && b.num != null ? `#${b.num}` : b.id;
      L.push(`  ${(b.done ? t.success : t.error)(b.done ? '✓' : '✗')} ${ref} ${t.dim(`(${b.kind})`)} ${b.title || ''}${b.kind === 'card' ? ` ${t.dim(`[${b.phase}]`)}` : b.kind === 'decision' ? ` ${t.dim(`[${b.status}]`)}` : ''}`);
    }
  }
  const items = p.criteria.items;
  if (items.length) {
    L.push('', heading(`CRITERIA${p.criteria.needsAcceptance ? '  (needsAcceptance — owner visual/UX ballot on close)' : ''}`));
    L.push(`  ${t.dim('Workers return a receipt. After integrated proof, the orchestrator marks rows met and closes the card.')}`);
    for (const it of items) {
      const state = (it.status === 'verified' ? t.success : it.status === 'met' ? t.warn : t.dim)(`[${it.status}]`);
      const moved = `${it.proof ? `  ${t.dim(`proof: ${proofCommand(it.proof)}`)}` : ''}${it.blockedBy ? `  ${t.warn(`blocked by ${it.blockedBy}`)}` : ''}${it.unblockedFrom ? `  ${t.dim(`unblocked: ${it.unblockedFrom} resolved`)}` : ''}${it.movedFrom ? `  ${t.dim(`← from #${it.movedFrom.card} c${it.movedFrom.n}`)}` : ''}${it.movedTo ? `  ${t.dim(`→ moved to #${it.movedTo.card} c${it.movedTo.n}`)}` : ''}`;
      L.push(`  ${t.border(`#${it.n}`)} ${state} ${it.text}${moved}${it.metBy ? `  ${t.dim(`met:${it.metBy}`)}` : ''}${it.verifiedBy ? `  ${t.dim(`verified:${it.verifiedBy}`)}` : ''}${it.evidence ? `  ${t.border('—')} ${it.evidence}` : ''}`);
    }
  } else {
    L.push('', heading(`CRITERIA  (none — an agent cannot close this card${p.criteria.needsAcceptance ? '; owner visual review still applies' : ''})`));
  }
  if (p.decisions.length) {
    L.push('', heading('DECISIONS'));
    for (const d of p.decisions) {
      const decisionState = (d.status === 'ratified' ? t.success : t.warn)(`[${d.status}${d.draft ? ' draft' : ''}]`);
      L.push(`  ${d.id} ${decisionState} ${d.title}`);
      if (d.situation) L.push(`    ${d.situation}`);
      if (d.gist) L.push(`    ${d.gist}`);
      if (d.status === 'ratified') {
        L.push(`    ${t.success('→')} ${d.outcome}${d.comment ? `  ${t.border('—')} ${d.comment}` : ''}`);
      } else {
        if (d.ballotMode) L.push(`    ${t.dim('profile:')} ${d.ballotMode} ballot`);
        if (d.lesson) L.push(`    ${t.dim('learn first:')} ${d.lesson}`);
        if (d.story) L.push(`    ${t.dim('story:')} ${d.story}`);
        if (d.inWild) L.push(`    ${t.dim('in the wild:')} ${d.inWild}`);
        for (const o of d.options || []) L.push(`    [${o.key}] ${o.name}${o.detail ? ` — ${o.detail}` : ''}${o.technical ? `\n      technical: ${String(o.technical).split('\n').join('\n      ')}` : ''}${o.code ? `\n      ${String(o.code).split('\n').join('\n      ')}` : ''}`);
        for (const [key, label] of [
          ['base', '● base pass'],
          ['boilOcean', '◎ boil-the-ocean pass'],
          ['hybrid', '◇ hybrid pass'],
          ['cooperative', '＋ cooperative pass'],
          ['beginner', '◌ beginner pass'],
          ['adversarial', '⚑ adversarial pass'],
        ]) if (d.reviewPasses?.[key]) L.push(`    ${t.dim(`${label}:`)} ${d.reviewPasses[key]}`);
        if (d.rec) L.push(`    ${t.warn('rec:')} ${d.rec}`);
        if (d.recommendation?.why) L.push(`    ${t.dim('why:')} ${d.recommendation.why}`);
        for (const loss of d.recommendation?.losses || []) {
          L.push(`    ${t.dim('remaining cost:')} ${typeof loss === 'string' ? loss : loss.loss}`);
          if (loss?.mitigation) L.push(`    ${t.dim('how we reduce it:')} ${loss.mitigation}`);
        }
        for (const rejected of d.recommendation?.whyNot || []) L.push(`    ${t.dim(`why not ${rejected.key}:`)} ${rejected.reason}`);
        if (d.recommendation?.tradeoff) L.push(`    ${t.dim('accepted tradeoff:')} ${d.recommendation.tradeoff}`);
        if (!d.reviewPasses && d.hybrid?.synthesis) L.push(`    ${t.dim(`◇ hybrid result ${d.hybrid.result}:`)} ${d.hybrid.synthesis}`);
        if (!d.reviewPasses) for (const item of d.hybrid?.harvest || []) L.push(`    ${t.dim(`harvest ${item.key}:`)} ${item.aspect} ${t.border('—')} ${item.use}`);
      }
    }
  }
  if (p.questions.length) {
    L.push('', heading('OPEN QUESTIONS'));
    for (const q of p.questions) L.push(`  ${t.warn(q.id)} ${t.dim(`[${q.by}]`)} ${q.text}`);
  }
  if (p.refs.length) L.push('', heading('REFS'), `  ${p.refs.join(t.border(', '))}`);
  if (p.log.length) {
    L.push('', heading('RECENT LOG'));
    for (const l of p.log) L.push(`  ${t.dim(`${l.at}  ${l.by ? `[${l.by}] ` : ''}`)}${l.text}`);
  }
  L.push('', heading('RULES'));
  for (const r of p.rules) L.push(`  ${t.border('·')} ${r}`);
  return L.join('\n');
}

// tower verdict '#N' --outcome "..." --supersedes D-ID [--title "…"] --by owner — mints an
// ALREADY-ratified decision recording an owner verdict, so it can never be
// mis-filed as a mere log note (D-TWRGUARD1=C #458). Owner-only, no --quote
// escape: this command IS the owner speaking.
function cmdVerdict(store, { pos, flags }) {
  const [ref] = pos;
  const { result } = store.mutate((s) => db.mintVerdict(s, ref, flags.outcome, flags.title, flags.by, flags.supersedes));
  return out(flags, `verdict ${result.id} recorded on card #${result.cardNum} → ${result.outcome}`, result);
}

// #461 — tower archive status|show|restore|shard. Cards/decisions retire to
// history.json on their own (store.mutate's chokepoint); this surface reads
// it back and lets the owner walk one back. `shard` (#6564 c7) moves the
// archive into size-bounded files once (history-shards.mjs); without --apply
// it is a dry run that writes nothing.
function cmdArchive(store, { pos, flags }) {
  const [verb, ref] = pos;
  const mib = (bytes) => `${(bytes / 1048576).toFixed(1)} MiB`;
  const shardLines = (shards) => shards.map(shard => `  history/${shard.file}: ${shard.items} items, ${mib(shard.bytes)}`);
  const largest = (shards) => Math.max(0, ...shards.map(shard => shard.bytes));
  switch (verb) {
    case 'status': {
      const h = store.loadHistory();
      const files = historyFiles(store.dataDir);
      const historyBytes = files.bytes;
      const liveBytes = existsSync(store.file) ? statSync(store.file).size : 0;
      const info = {
        cards: h.cards.length, decisions: h.decisions.length, events: h.events.length, historyBytes, liveBytes,
        layout: files.layout, shards: files.shards.length, largestShardBytes: largest(files.shards),
      };
      const where = files.layout === 'shards'
        ? ` in ${info.shards} shards under history/ (largest ${(info.largestShardBytes / 1024).toFixed(1)} KB)`
        : '';
      return out(flags, `history: ${info.cards} cards, ${info.decisions} decisions, ${info.events} events — ${(historyBytes / 1024).toFixed(1)} KB${where} (live tower.json ${(liveBytes / 1024).toFixed(1)} KB)`, info);
    }
    case 'show': {
      const h = store.loadHistory();
      const c = db.findInHistory(h, ref);
      if (c) return out(flags, null, { ...c, archived: true });
      const d = h.decisions.find(x => x.id === ref);
      if (d) return out(flags, null, { ...d, archived: true });
      throw new TowerError('E_NOT_FOUND', `no archived card or decision ${ref}`);
    }
    case 'restore': {
      const { result } = store.restoreArchived(ref, flags.by);
      return out(flags, `restored ${result.kind} ${result.id}${result.num ? ` (#${result.num})` : ''} from archive`, result);
    }
    case 'shard': {
      const files = historyFiles(store.dataDir);
      if (files.layout === 'shards')
        return out(flags, [
          `history is already sharded: ${files.shards.length} shards under history/, ${mib(files.bytes)} in all, largest ${mib(largest(files.shards))}`,
          ...shardLines(files.shards),
        ].join('\n'), { applied: false, layout: 'shards', bytes: files.bytes, shards: files.shards });
      if (!flags.apply) {
        const plan = planShards(store.loadPair().history);
        const shards = plan.files.map(({ kind, file, items, bytes, sha256 }) => ({ kind, file, items, bytes, sha256 }));
        return out(flags, [
          `dry run: history.json (${mib(files.bytes)}) would become a manifest and ${shards.length} shards under history/, each at most ${mib(SHARD_BYTES)} (largest ${mib(largest(shards))}); rerun with --apply --by X to write`,
          ...shardLines(shards),
        ].join('\n'), { applied: false, layout: files.layout, bytes: files.bytes, bound: SHARD_BYTES, shards });
      }
      if (!flags.by || flags.by === true) throw new TowerError('E_USAGE', 'archive shard --apply needs --by <agent>');
      const { state } = store.mutate((s) => db.logEvent(s, {
        by: flags.by, action: 'archive.shard', ref: 'history.json',
        note: `the archive moved from history.json to shards under history/, each at most ${mib(SHARD_BYTES)}`,
      }), { expectRev: flags.expectRev, historyLayout: 'shards' });
      const after = historyFiles(store.dataDir);
      return out(flags, [
        `sharded at rev ${state.meta.rev}: ${after.shards.length} shards under history/, ${mib(after.bytes)} in all, largest ${mib(largest(after.shards))}`,
        ...shardLines(after.shards),
      ].join('\n'), { applied: true, rev: state.meta.rev, layout: after.layout, bytes: after.bytes, shards: after.shards });
    }
    default: throw new TowerError('E_USAGE', `unknown archive verb "${verb}" — status/show/restore/shard`);
  }
}

function cmdRepair(store, { pos, flags }) {
  const [verb] = pos;
  if (verb !== 'apply')
    throw new TowerError('E_USAGE', 'repair supports only: apply --manifest FILE --expect-rev N --by X [--dry-run]');
  let manifest;
  try { manifest = JSON.parse(readFileSync(resolve(flags.manifest), 'utf8')); }
  catch (error) { throw new TowerError('E_MANIFEST', `cannot read repair manifest: ${error.message}`); }
  const result = applyRepairManifest(store.dataDir, manifest, {
    expectRev: flags.expectRev,
    by: flags.by,
    dryRun: !!flags.dryRun,
    backups: store.config.backups,
  });
  const mode = result.dryRun ? 'dry-run valid' : `applied at rev ${result.rev}`;
  return out(flags,
    `repair ${mode}: ${result.fields} fields, ${result.substitutions} substitutions, manifest ${result.manifestHash}`,
    result);
}

// #6564 c5: without --apply this is a dry run that changes nothing. The rule
// table comes from this machine (personal-info.mjs, the table the guard reads);
// run it where the board's text was written, with JET_SCRATCH_ROOT set, so
// every rule maps as shown. --apply on a board with nothing to redact writes
// nothing, so the rev and the event log stay as they were.
function cmdRedact(store, { flags }) {
  const matcher = machinePersonalInfo();
  const table = matcher.rules.map(rule => `  ${rule.label}: ${rule.kind === 'word' ? 'word ' : ''}${rule.find} → ${rule.replace}`);
  const show = (report, head) => [
    head,
    `rule table ${report.ruleTable}:`, ...table,
    'spans per file:', ...Object.entries(report.files).map(([name, n]) => `  ${name}: ${n}`),
    'spans per section:', ...Object.entries(report.sections).map(([name, n]) => `  ${name}: ${n}`),
    'spans per rule:', ...Object.entries(report.rules).map(([label, n]) => `  ${label}: ${n}`),
  ].join('\n');
  if (flags.apply && (!flags.by || flags.by === true)) throw new TowerError('E_USAGE', 'redact --apply needs --by <agent>');
  const { state, history } = store.loadPair();
  const report = db.redactBoardCopy(state, history, matcher);
  if (!flags.apply)
    return out(flags, show(report, `dry run: ${report.total} spans would be redacted; rerun with --apply --by X to write`), { ...report, applied: false });
  if (!report.total)
    return out(flags, show(report, `nothing to redact; the board is unchanged at rev ${state.meta.rev}`), { ...report, applied: false, rev: state.meta.rev });
  const { result, state: after } = store.mutate((s, cfg, h) => db.applyRedaction(s, h, flags.by, matcher), { expectRev: flags.expectRev });
  return out(flags, show(result, `redacted ${result.total} spans at rev ${after.meta.rev}`), { ...result, applied: true, rev: after.meta.rev });
}

function cmdEvents(store, { flags }) {
  const s = store.load();
  const es = s.events.slice(0, Number(flags.limit || 30));
  if (flags.json) return out(flags, null, es);
  const cardRef = (ref) => {
    const c = s.cards.find(x => x.id === ref);
    return c ? `#${c.num}` : ref || '';
  };
  for (const e of es) console.log(`${e.at}  ${String(e.by || '').padEnd(10)} ${e.action.padEnd(16)} ${cardRef(e.ref)}  ${e.note || ''}`);
  if (!es.length) console.log('(no events yet)');
}

function cmdImport({ pos, flags }) {
  const [src] = pos;
  const old = readJSON(resolve(src));
  if (!old) throw new TowerError('E_NOT_FOUND', `cannot read ${src}`);
  const dir = resolve(flags.dir || '.', '.tower');
  const file = join(dir, 'tower.json');
  if (existsSync(file) && !flags.force) throw new TowerError('E_EXISTS', `${file} exists — pass --force to overwrite`);
  mkdirSync(dir, { recursive: true });
  const s = migrate(old, { project: flags.name || 'Project' });
  writeJSON(file, s);
  if (!existsSync(join(dir, 'config.json'))) writeJSON(join(dir, 'config.json'), { project: s.meta.project });
  ensureDataIgnores(dir);
  return out(flags, `imported ${s.cards.length} cards, ${s.decisions.length} decisions, ${s.questions.length} questions, ${s.ideas.length} ideas → ${file}`,
    { file, cards: s.cards.length, decisions: s.decisions.length, questions: s.questions.length, ideas: s.ideas.length });
}

// ---- undo, git hook -------------------------------------------------------

function cmdUndo(store, { flags }) {
  const bdir = join(store.dataDir, 'backups');
  const cur = store.load();
  const prev = readLatestJSON(bdir, 'tower-', null);
  if (!prev) throw new TowerError('E_INVALID', 'nothing to undo (no backups yet)');
  store.restore(prev, { expectRev: flags.expectRev ?? cur.meta.rev, by: flags.by });
  return out(flags, `undid last write — board back to rev ${prev.meta?.rev ?? '?'} content (now rev ${cur.meta.rev + 1})`, { ok: true });
}

// #6563 c4 (board-git.mjs): `commit` closes an operation batch by committing
// the board pair now; `check` fails when the committed board trails the
// working copy by more than one batch.
function cmdBoard(store, { pos, flags }) {
  const [verb] = pos;
  switch (verb) {
    case 'commit': {
      const result = store.commitBoard(flags.by);
      if (result.committed) return out(flags, `committed board rev ${result.rev} to ${result.branch} as ${result.committed.slice(0, 10)} (${result.paths.join(', ')})`, result);
      if (/board is current/.test(result.reason)) return out(flags, `board unchanged: ${result.reason}`, result);
      throw new TowerError('E_BOARD_COMMIT', `board not committed: ${result.reason}`);
    }
    case 'check': {
      const result = store.checkBoard();
      if (result.reason) throw new TowerError('E_BOARD_COMMIT', `board check: ${result.reason}`);
      const line = `committed board rev ${result.committedRev} at ${result.branch} ${result.head}; working copy rev ${result.workingRev}: ${result.behind} write${result.behind === 1 ? '' : 's'} behind (batch ${result.batch})`;
      if (!result.ok) throw new TowerError('E_BOARD_STALE', `${line} — run \`tower board commit --by <agent>\``);
      return out(flags, `OK ${line}`, result);
    }
    default: throw new TowerError('E_USAGE', `unknown board verb "${verb}" — commit/check`);
  }
}

// `tower apply --file plan.json`: a plan of board edits as one all-or-nothing
// write (apply.mjs). --dry-run runs it on a copy and writes nothing.
function cmdApply(store, { flags }) {
  const plan = readPayload(flags);
  if (flags.dryRun) {
    const pair = store.loadPair();
    const result = applyPlan(structuredClone(pair.state), plan, flags.by, store.config, structuredClone(pair.history));
    return out(flags, `dry run: ${result.applied} op${result.applied === 1 ? '' : 's'} would apply (${result.summary}); nothing written`, { ...result, dryRun: true });
  }
  const { result, state } = store.mutate((s, cfg, history) => applyPlan(s, plan, flags.by, cfg, history), { expectRev: flags.expectRev });
  return out(flags, `applied ${result.applied} op${result.applied === 1 ? '' : 's'} in one write at rev ${state.meta.rev} (${result.summary})`, { ...result, rev: state.meta.rev });
}

// `tower publish`: post the briefing, commit the board, push the board branch
// and confirm origin holds that very commit; anything less exits non-zero.
function cmdPublish(store, { flags }) {
  const body = readFileSync(flags.file === '-' ? 0 : flags.file, 'utf8');
  const { result: briefing } = store.mutate(s => db.postBriefing(s, { title: flags.title, body, sections: [], by: flags.by }));
  const pushed = store.publishBoard(flags.by);
  if (!pushed.ok) throw new TowerError('E_PUBLISH', `posted briefing ${briefing.id}, but the board is not published: ${pushed.reason}`);
  return out(flags, `published briefing ${briefing.id} — ${briefing.title}; origin/${pushed.branch} = ${pushed.head.slice(0, 10)}`,
    { briefing: briefing.id, branch: pushed.branch, commit: pushed.head, remote: pushed.remote });
}

// #6834: the gauntlet table the Gauntlet page shows, as text: every entry's
// grid from the entries' manifests with the table's measured rows. --table
// names another table file than the machine's
// ($JET_STATE_ROOT/gauntlet/table.json).
async function cmdGauntlet(store, { pos, flags }) {
  const [verb] = pos;
  if (verb !== 'standings') throw new TowerError('E_USAGE', `unknown gauntlet verb "${verb}" — standings`);
  if (flags.table === true) throw new TowerError('E_USAGE', 'gauntlet standings --table needs a PATH');
  const root = projectRoot(store.dataDir);
  const view = projectGauntletStandings(await readGauntletStandings(root, flags.table === undefined ? undefined : resolve(String(flags.table))));
  return out(flags, gauntletStandingsText(view), view);
}

// Installs a post-commit hook that appends any commit mentioning #<num> to
// that card's log. Silent + always exit 0, so it can never break a commit.
function cmdGithookInstall(store, flags) {
  const projectRoot = dirname(store.dataDir);
  const gitDir = (() => {
    let d = projectRoot;
    for (;;) { if (existsSync(join(d, '.git'))) return join(d, '.git'); const p = dirname(d); if (p === d) return null; d = p; }
  })();
  if (!gitDir) throw new TowerError('E_NOT_FOUND', 'no .git found at or above the project root');
  const hooksDir = join(gitDir, 'hooks');
  mkdirSync(hooksDir, { recursive: true });
  const hookPath = join(hooksDir, 'post-commit');
  const towerBin = join(dirname(new URL(import.meta.url).pathname), '..', 'tower.mjs');
  const line = `node "${towerBin}" githook post-commit >/dev/null 2>&1 || true   # tower card-link`;
  let cur = existsSync(hookPath) ? readFileSync(hookPath, 'utf8') : '#!/bin/sh\n';
  if (!cur.includes('tower card-link')) {
    cur = cur.trimEnd() + '\n' + line + '\n';
    writeFileSync(hookPath, cur);
    chmodSync(hookPath, 0o755);
  }
  return out(flags, `installed post-commit hook → ${hookPath}\ncommits mentioning #<num> now append to that card's log`, { hook: hookPath });
}

// The real post-commit worker: read HEAD, find #N refs, log them. Outside a
// git work tree it logs nothing.
async function githookPostCommit(store, flags) {
  const git = (...args) => spawnSync('git', args, { cwd: dirname(store.dataDir), encoding: 'utf8' });
  const head = git('rev-parse', '--short', 'HEAD');
  const hash = head.status === 0 ? head.stdout.trim() : '';
  const subject = hash ? git('log', '-1', '--format=%s').stdout.trim() : '';
  const nums = [...new Set([...subject.matchAll(/#(\d+)\b/g)].map(m => Number(m[1])))];
  const s = nums.length ? store.load() : null;
  const logged = [];
  for (const n of nums) {
    const c = s.cards.find(x => x.num === n);
    if (!c) continue;
    try {
      store.mutate((s2, cfg) => db.updateCard(s2, c.id, { logEntry: `commit ${hash}: ${subject.slice(0, 110)}`, by: 'git' }, cfg));
      logged.push(n);
    } catch { /* never break a commit */ }
  }
  if (!flags.json && !logged.length) return;
  return out(flags, `logged commit ${hash} on ${logged.map(n => `#${n}`).join(', ')}`, { commit: hash || null, cards: logged });
}

const HELP = `tower — file-backed project board for an owner + AI agents

${HELP_SURFACE}

  Shared flags: --json, --help, --data DIR.
  Complex payloads: --file FILE or --stdin (writes only where shown above).
  Cards accept #num or id. Writers pass --by X; --expect-rev N is rev-guarded.
  Phases: ${PHASE_IDS.map(id => id === 'verify' ? 'review (phase id verify)' : id).join(' ')}

  Guards (agent-hard, owner-soft — --by owner bypasses card closure; see plugin AGENTS.md):
    ballot validation (full/short profile, simple prose, ordered reviews; E_BALLOT), owner-only ratify (E_OWNER_ONLY),
    frozen write guard (E_OWNER_LANE), ratified-decision delete guard (E_HAS_RATIFIED),
    building-release handoff (E_HANDOFF), nonempty card exit criteria (E_CRITERIA),
    durable criterion evidence: a commit on dev or master, or --kind decision|duplicate|obsolete (E_EVIDENCE),
    no personal or machine information in new text; \`tower redact\` cleans old text (E_PERSONAL_INFO),
    and explicit milestone review (E_MILESTONE).
`;

function helpContract() {
  const mismatches = [];
  if (!HELP.includes(HELP_SURFACE)) mismatches.push('generated command surface is missing from help');
  const documentedGlobals = new Set((HELP.match(/--[a-z0-9-]+/g) || []));
  for (const key of GLOBAL_FLAGS) {
    if (!documentedGlobals.has(spellFlag(key))) mismatches.push(`global ${spellFlag(key)} is not documented`);
  }
  for (const entry of HELP_ENTRIES) {
    const firstLine = entry.split('\n')[0];
    if (/^  tower \S+\s{2,}\S/.test(firstLine)) continue;
    const path = firstLine.trim().split(' [')[0].replace(/^tower /, '');
    const parts = path.split(/\s+/);
    const flags = parts.length > 1
      ? commandFlags(parts[0], parts[1])
      : commandFlags(parts[0]);
    const expected = (flags || []).map(spellFlag);
    if (!HELP.includes(entry)) {
      mismatches.push(`${path}: command line is missing from help`);
      for (const flag of expected) mismatches.push(`${path}: ${flag} is accepted but not documented`);
      continue;
    }
    const declared = [...entry.matchAll(/--[a-z0-9-]+/g)].map((m) => m[0]);
    for (const flag of expected) {
      if (!declared.includes(flag)) mismatches.push(`${path}: ${flag} is accepted but not documented`);
    }
    for (const flag of declared) {
      if (!expected.includes(flag)) mismatches.push(`${path}: ${flag} is documented but not accepted`);
    }
  }
  return mismatches;
}

function cmdHelpCheck(flags) {
  const mismatches = helpContract();
  if (mismatches.length) throw new TowerError('E_HELP', `help/parser mismatch: ${mismatches.join('; ')}`);
  const result = { ok: true, mismatches: [], checked: Object.keys(COMMAND_FLAGS).length };
  out(flags, JSON.stringify(result), result);
}

// `tower help --json`: every command path with its argument, required flags
// and accepted flags, from the same tables the parser enforces.
function helpJSON() {
  const entry = (path, flags) => [path, {
    arg: REQUIRED[path]?.arg ?? null,
    required: (REQUIRED[path]?.flags || []).map(spellFlag),
    flags: flags.map(spellFlag),
  }];
  return {
    commands: Object.fromEntries(Object.entries(COMMAND_FLAGS).flatMap(([cmd, spec]) => [
      ...(spec.flags ? [entry(cmd, spec.flags)] : []),
      ...Object.entries(spec.verbs || {}).map(([verb, flags]) => entry(`${cmd} ${verb}`, flags)),
    ])),
    globalFlags: GLOBAL_FLAGS.map(spellFlag),
  };
}
const printHelp = (flags) => out(flags, HELP, helpJSON());

// ---- dispatch ----------------------------------------------------------------

export async function run(argv) {
  const { pos, flags } = parseArgs(argv);
  const [cmd, ...rest] = pos;
  const sub = { pos: rest, flags };
  try {
    if (!cmd) return printHelp(flags);
    if (cmd === 'help') {
      checkFlags(cmd, undefined, flags);
      return flags.check ? cmdHelpCheck(flags) : printHelp(flags);
    }
    checkFlags(cmd, rest[0], flags);
    if (flags.help) return printHelp(flags);
    checkRequired(cmd, rest[0], COMMAND_FLAGS[cmd].verbs ? rest.slice(1) : rest, flags);
    if (cmd === 'init') return cmdInit(sub);
    if (cmd === 'import') return cmdImport(sub);

    const dataDir = flags.data
      ? (String(flags.data).endsWith('.json') ? dirname(resolve(flags.data)) : resolve(flags.data))
      : findDataDir();
    const store = openStore(dataDir);

    switch (cmd) {
      case 'serve': {
        // #522 — the server loads all routes/db code once at start. Plain
        // `tower serve` is a supervisor that restarts its `--worker` server
        // when App/*.mjs changes (restart.mjs); --no-watch serves in this
        // process with no supervisor (tests, embedding).
        if (!flags.noWatch && !flags.worker) {
          const { superviseServe } = await import('./restart.mjs');
          return superviseServe({
            towerRoot: join(dirname(fileURLToPath(import.meta.url)), '..'),
            argv: process.argv.slice(2),
            // Under --json stdout carries only each worker's listening record.
            ...(flags.json ? { log: (...a) => console.error(...a) } : {}),
          });
        }
        const { serve } = await import('./server.mjs');
        const port = Number(flags.port || store.config.port);
        if (!flags.worker) return serve(store, port, !!flags.open, { json: !!flags.json });
        const { serveWorker } = await import('./restart.mjs');
        return serveWorker({ serveFn: (open) => serve(store, port, open, { json: !!flags.json }), open: !!flags.open })();
      }
      case 'status':    return sub.pos.length ? cmdStatusReport(store, sub) : cmdStatus(store, sub);
      case 'briefing':  return cmdBriefing(store, sub);
      case 'state':     return out(flags, null, store.project());
      case 'card':      return cmdCard(store, sub);
      case 'decision':  return cmdDecision(store, sub);
      case 'question':  return cmdQuestion(store, sub);
      case 'message':   return cmdMessage(store, sub);
      case 'idea':      return cmdIdea(store, sub);
      case 'epoch':     return cmdEpoch(store, sub);
      case 'milestone': return cmdMilestone(store, sub);
      case 'next':      return cmdNext(store, sub);
      case 'chain':     return cmdChain(store, sub);
      case 'closest':   return cmdClosest(store, sub);
      case 'ballots':   return cmdBallots(store, sub);
      case 'brief':     return cmdBrief(store, sub);
      case 'lint':      return cmdLint(store, sub);
      case 'prove':     return cmdProve(store, sub);
      case 'docs':      return cmdDocs(store, sub);
      case 'verdict':   return cmdVerdict(store, sub);
      case 'archive':   return cmdArchive(store, sub);
      case 'repair':    return cmdRepair(store, sub);
      case 'redact':    return cmdRedact(store, sub);
      case 'events':    return cmdEvents(store, sub);
      case 'undo':      return cmdUndo(store, sub);
      case 'board':     return cmdBoard(store, sub);
      case 'apply':     return cmdApply(store, sub);
      case 'publish':   return cmdPublish(store, sub);
      case 'gauntlet':  return await cmdGauntlet(store, sub);
      case 'githook':
        if (sub.pos[0] === 'post-commit') return await githookPostCommit(store, flags);
        return cmdGithookInstall(store, flags);
    }
  } catch (e) {
    // Under --json every failure is one `{ error: { code, message } }` object
    // on stdout, whatever threw it.
    const known = e instanceof TowerError || e instanceof ConfigError;
    if (!known && !flags.json) throw e;
    const code = known ? e.code : 'E_INTERNAL';
    console.error(`tower: ${e.message}`);
    process.exitCode = code === 'E_CONFLICT' ? 2 : 1;
    if (flags.json) console.log(JSON.stringify({ error: { code, message: e.message } }));
  }
}
