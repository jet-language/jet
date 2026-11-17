// The CLI contract: every registered verb answers --json with exactly one JSON
// document, and every failure is `{ error: { code, message } }` with a
// non-zero exit. Mistyped commands, verbs and flags get a suggestion, a
// missing argument or flag is named, and --by is recorded on epoch writes.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile, spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import * as db from '../App/store.mjs';
import { writeJSON } from '../App/paths.mjs';
import { COMMAND_FLAGS } from '../App/cli.mjs';
import { evidenceProvenance } from '../App/lint.mjs';

const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const EVAL = ['--input-type=module', '--eval', `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--'];
const ENV = { ...process.env, TOWER_DATA: '' };
const cli = (data, ...args) => spawnSync(process.execPath, [...EVAL, '--data', data, ...args], { encoding: 'utf8', env: ENV, input: '' });

// A small board with one of each record, so every verb has something to act on.
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'tower-contract-'));
  const data = join(root, '.tower');
  mkdirSync(data);
  writeJSON(join(data, 'tower.json'), db.empty('Contract'));
  writeJSON(join(data, 'config.json'), { project: 'Contract' });
  const st = db.openStore(data);
  const ids = {};
  st.mutate((s, cfg) => {
    db.addEpoch(s, { id: 'e2', name: 'Two', status: 'planned', by: 'seed' });
    db.addCard(s, { title: 'One', epoch: 'e1', criteria: ['first', 'second'], by: 'seed' }, cfg);
    db.addCard(s, { title: 'Two', epoch: 'e1', by: 'seed' }, cfg);
    db.addMilestone(s, { id: 'm-1', epochId: 'e1', title: 'MVP', exitCriteria: ['ships'], by: 'seed' });
    db.addDecision(s, { cardId: '#1', id: 'D-T1', title: 'Which way?', draft: true, ballotMode: 'short', by: 'seed' });
    ids.question = db.addQuestion(s, { cardId: '#1', text: 'Why?', by: 'owner' }).id;
    ids.message = db.addMessage(s, { cardId: '#2', text: 'Heads up', by: 'agent-a' }).id;
    ids.idea = db.addIdea(s, { text: 'Maybe later', by: 'seed' }).id;
  });
  writeFileSync(join(root, 'brief.md'), '# Day\n\nWork landed.\n');
  writeFileSync(join(root, 'plan.json'), JSON.stringify({ ops: [{ op: 'add', card: '#1', text: 'third' }] }));
  return { root, data, st, ids };
}

// Arguments for each registered command path; `--json` (and `--by` where the
// verb takes it) are added. A path the CLI registers without an entry here
// fails the test, so a new verb cannot skip the contract.
const ARGS = ({ root, ids }) => ({
  init: ['--dir', join(root, 'fresh')],
  import: [join(root, '.tower', 'tower.json'), '--dir', join(root, 'imported')],
  serve: null, // a long-running process: its own test below
  status: [],
  'status post': ['--file', join(root, 'brief.md')],
  'status show': [],
  'briefing post': ['--file', join(root, 'brief.md'), '--title', 'Day'],
  'briefing list': [],
  'briefing show': ['missing'],
  state: [],
  help: [],
  'card list': [],
  'card show': ['#1'],
  'card add': ['--title', 'Three'],
  'card update': ['#1', '--priority', 'P1'],
  'card claim': ['#1'],
  'card release': ['#1'],
  'card delete': ['#2'],
  'card criteria': ['#1', '--add', 'third'],
  'card log': ['#1', '--text', 'noted'],
  'card history': ['#1'],
  'card split': ['#1', '--criteria', '1', '--title', 'Split'],
  'decision list': [],
  'decision show': ['D-T1'],
  'decision scaffold': ['#1', '--id', 'D-T2', '--out', join(root, 'scaffold.json')],
  'decision add': ['--card', '#1', '--title', 'Another?', '--draft'],
  'decision update': ['D-T1', '--title', 'Which way now?'],
  'decision review': ['D-T1'],
  'decision post': ['D-T1'],
  'decision report': ['D-T1', '--url', 'https://example.com/report'],
  'decision ratify': ['D-T1', '--outcome', 'A'],
  'decision reopen': ['D-T1'],
  'decision delete': ['D-T1'],
  'question list': [],
  'question ask': ['#1', '--text', 'How?'],
  'question answer': [ids.question, '--text', 'Like this.'],
  'question delete': [ids.question],
  'message list': [],
  'message add': ['#2', '--text', 'Another'],
  'message done': [ids.message],
  'idea list': [],
  'idea add': ['A', 'new', 'idea'],
  'idea promote': [ids.idea],
  'idea delete': [ids.idea],
  'epoch list': [],
  'epoch add': ['e9', '--name', 'Nine'],
  'epoch update': ['e2', '--goal', 'Grow'],
  'epoch current': ['none'],
  'epoch remove': ['e2'],
  'milestone list': [],
  'milestone add': ['--epoch', 'e1', '--title', 'Beta'],
  'milestone update': ['m-1', '--title', 'MVP 2'],
  'milestone criteria': ['m-1', '--list'],
  'milestone retire': ['m-1', '--reason', 'scope cut'],
  'milestone closeout': ['m-1'],
  'milestone verify': ['m-1'],
  'milestone delete': ['m-1'],
  next: [],
  chain: [],
  closest: [],
  ballots: [],
  brief: ['--no-claim'],
  lint: [],
  prove: ['--card', '#1', '--dry-run'],
  'docs list': [],
  'docs show': ['--scratch'],
  'docs add': ['--section', 'research', '--title', 'Notes'],
  'docs update': ['--scratch', '--body', 'scratch text'],
  'docs delete': ['--path', 'Docs/Research/none.md'],
  verdict: ['#1', '--outcome', 'pass'],
  'archive status': [],
  'archive show': ['c-none'],
  'archive restore': ['c-none'],
  'archive shard': [],
  'repair apply': ['--manifest', join(root, 'no-manifest.json')],
  redact: [],
  events: [],
  undo: [],
  'githook install': [],
  'githook post-commit': [],
  'board commit': [],
  'board check': [],
  apply: ['--file', join(root, 'plan.json')],
  publish: ['--title', 'Day', '--file', join(root, 'brief.md')],
  'gauntlet standings': [],
});

const registered = () => Object.entries(COMMAND_FLAGS).flatMap(([cmd, spec]) => [
  ...(spec.flags ? [[cmd, spec.flags]] : []),
  ...Object.entries(spec.verbs || {}).map(([verb, flags]) => [`${cmd} ${verb}`, flags]),
]);

test('every registered verb answers --json with one JSON document, or one error object and a non-zero exit', async () => {
  const base = fixture();
  const table = ARGS(base);
  const paths = registered();
  assert.deepEqual(paths.map(([path]) => path).filter(path => !Object.hasOwn(table, path)), [], 'every registered path has contract arguments');
  assert.deepEqual(Object.keys(table).filter(path => !paths.some(([p]) => p === path)), [], 'every contract entry is a registered path');
  const run = promisify(execFile);
  const results = [];
  const queue = paths.filter(([path]) => table[path] !== null);
  const worker = async () => {
    for (let next = queue.shift(); next; next = queue.shift()) {
      const [path, flags] = next;
      // Each verb gets its own copy of the board, so no verb sees another's writes.
      const root = mkdtempSync(join(tmpdir(), 'tower-contract-run-'));
      cpSync(base.root, root, { recursive: true });
      const args = ARGS({ root, ids: base.ids })[path];
      const argv = [...EVAL, '--data', join(root, '.tower'), ...path.split(' '), ...args, '--json', ...(flags.includes('by') ? ['--by', 'tester'] : [])];
      const child = run(process.execPath, argv, { encoding: 'utf8', env: ENV, cwd: root, timeout: 60_000 });
      child.child.stdin.end();
      const result = await child.then(r => ({ ...r, code: 0 }), e => ({ stdout: e.stdout, stderr: e.stderr, code: e.code }));
      results.push({ path, ...result });
    }
  };
  await Promise.all(Array.from({ length: 6 }, worker));
  for (const { path, stdout, stderr, code } of results) {
    let doc;
    assert.doesNotThrow(() => { doc = JSON.parse(stdout); }, `${path}: stdout is one JSON document — got ${JSON.stringify(stdout.slice(0, 200))} (stderr ${stderr.slice(0, 200)})`);
    if (code === 0) {
      assert.ok(doc === null || typeof doc !== 'object' || !Object.hasOwn(doc, 'error'), `${path}: a success carries no error`);
    } else {
      assert.equal(typeof code, 'number', `${path}: exits with a status, not a signal`);
      assert.match(doc?.error?.code ?? '', /^E_[A-Z_]+$/, `${path}: error.code — got ${stdout.slice(0, 200)}`);
      assert.ok(typeof doc.error.message === 'string' && doc.error.message.length, `${path}: error.message`);
      // gauntlet standings loads Tools/Gauntlet, which a Tower-only checkout lacks.
      if (path !== 'gauntlet standings') assert.notEqual(doc.error.code, 'E_INTERNAL', `${path}: refused, not crashed — ${doc.error.message}`);
    }
  }
  // The table is meant to drive real work, not a wall of refusals.
  const ok = results.filter(r => r.code === 0).map(r => r.path);
  for (const path of ['card add', 'card history', 'epoch current', 'apply', 'help', 'init', 'import', 'githook post-commit'])
    assert.ok(ok.includes(path), `${path} succeeds — ${JSON.stringify(results.find(r => r.path === path)?.stdout?.slice(0, 300))}`);
});

test('serve --json prints one listening record', async () => {
  const { data } = fixture();
  const probe = createServer();
  await new Promise(resolve => probe.listen(0, resolve));
  const port = probe.address().port;
  await new Promise(resolve => probe.close(resolve));
  const child = spawn(process.execPath, [...EVAL, '--data', data, 'serve', '--no-watch', '--port', String(port), '--json'], { env: ENV });
  try {
    const line = await new Promise((resolve, reject) => {
      let text = '';
      child.stdout.on('data', (chunk) => { text += chunk; if (text.includes('\n')) resolve(text.split('\n')[0]); });
      child.on('exit', () => reject(new Error(`serve exited before listening: ${text}`)));
    });
    const doc = JSON.parse(line);
    assert.equal(doc.port, port);
    assert.equal(doc.url, `http://localhost:${port}`);
    assert.equal(doc.project, 'Contract');
  } finally {
    child.kill();
  }
});

test('a mistyped command, verb or flag names the nearest real one', () => {
  const { data } = fixture();
  const cases = [
    [['crad', 'list'], /unknown command "crad" — did you mean `tower card`\?/],
    [['card', 'lsit'], /unknown card verb "lsit" — did you mean `tower card list`\? — one of list\|show\|/],
    [['card'], /`tower card` needs a verb: list\|show\|add/],
    [['card', 'add', '--titel', 'X', '--by', 't'], /unknown flag for `tower card add`: --titel \(did you mean --title\?\)/],
    [['epoch', 'current', 'e2', '--bye', 't'], /unknown flag for `tower epoch current`: --bye \(did you mean --by\?\)/],
    [['card', 'list', '--zzzz'], /unknown flag for `tower card list`: --zzzz — run `tower help`/],
  ];
  for (const [args, message] of cases) {
    const r = cli(data, ...args);
    assert.equal(r.status, 1, args.join(' '));
    assert.match(r.stderr, message);
  }
});

test('a missing argument or required flag is named, and blank text counts as missing', () => {
  const { data, st } = fixture();
  const rev = st.load().meta.rev;
  const cases = [
    [['card', 'log', '#1', '--by', 't'], /tower card log needs --text "…"/],
    [['card', 'log', '--by', 't'], /tower card log needs REF and --text "…"/],
    [['card', 'log', '#1', '--text', '   ', '--by', 't'], /tower card log needs --text "…"/],
    [['card', 'show'], /tower card show needs REF/],
    [['epoch', 'current', '--by', 't'], /tower epoch current needs E\|none/],
    [['milestone', 'retire', 'm-1', '--by', 't'], /tower milestone retire needs --reason "…"/],
    [['question', 'ask', '#1', '--by', 't'], /tower question ask needs --text "…"/],
    [['decision', 'ratify', 'D-T1', '--by', 't'], /tower decision ratify needs --outcome/],
    [['publish', '--by', 't'], /tower publish needs --title "…" and --file FILE/],
    [['apply', '--by', 't'], /tower apply needs --file FILE/],
  ];
  for (const [args, message] of cases) {
    const r = cli(data, ...args, '--json');
    assert.equal(r.status, 1, args.join(' '));
    assert.match(r.stderr, message);
    assert.equal(JSON.parse(r.stdout).error.code, 'E_USAGE');
  }
  assert.equal(st.load().meta.rev, rev, 'a refused command writes nothing');
  // A payload file stands in for the flags it carries.
  assert.equal(cli(data, 'card', 'add', '--stdin', '--by', 't').status, 1, 'an empty payload is still refused, by the payload reader');
});

test('--by is recorded on epoch add, update and current, and on undo', () => {
  const { data, st } = fixture();
  const steps = [['epoch', 'add', 'e3', '--name', 'Three'], ['epoch', 'update', 'e3', '--goal', 'Grow'], ['epoch', 'current', 'none'], ['epoch', 'current', 'e3']];
  for (const args of steps) {
    const r = cli(data, ...args, '--by', 'planner-1');
    assert.equal(r.status, 0, r.stderr);
  }
  const events = st.load().events.filter(e => e.ref === 'e3' || e.ref === 'e1');
  assert.deepEqual(events.map(e => [e.action, e.ref, e.by, e.note]).slice(0, 4), [
    ['epoch.update', 'e3', 'planner-1', 'status'],
    ['epoch.current', 'e1', 'planner-1', 'none active'],
    ['epoch.update', 'e3', 'planner-1', 'goal'],
    ['epoch.add', 'e3', 'planner-1', 'Three'],
  ]);
  const none = cli(data, 'epoch', 'current', 'none', '--by', 'planner-2');
  assert.equal(none.status, 0, none.stderr);
  assert.deepEqual(Object.values(st.load().events[0]).slice(1, 4), ['planner-2', 'epoch.current', 'e3']);
  const rev = st.load().meta.rev;
  const undo = cli(data, 'undo', '--by', 'planner-3', '--expect-rev', String(rev));
  assert.equal(undo.status, 0, undo.stderr);
  assert.deepEqual([st.load().events[0].action, st.load().events[0].by], ['board.undo', 'planner-3']);
});

test('card history lists the card, its decision and question, and plans that touched it, newest first', () => {
  const { data, root } = fixture();
  assert.equal(cli(data, 'apply', '--file', join(root, 'plan.json'), '--by', 'planner').status, 0);
  const r = cli(data, 'card', 'history', '#1', '--json');
  assert.equal(r.status, 0, r.stderr);
  const found = JSON.parse(r.stdout);
  assert.deepEqual([found.card.num, found.card.title], [1, 'One']);
  const actions = found.events.map(e => e.action);
  assert.equal(actions[0], 'board.apply');
  for (const action of ['card.add', 'decision.add', 'question.add']) assert.ok(actions.includes(action), action);
  assert.ok(!actions.includes('message.add'), 'a message on another card is not this card\'s history');
  assert.deepEqual([...found.events].sort((a, b) => b.at.localeCompare(a.at)).map(e => e.at), found.events.map(e => e.at));
  const text = cli(data, 'card', 'history', '1', '--limit', '1');
  assert.match(text.stdout, /^#1 One\n\S+ +planner +board\.apply +1 op: 1 add\n$/);
  assert.match(cli(data, 'card', 'history', '#99', '--json').stdout, /"code":"E_NOT_FOUND"/);
});

test('lint --evidence warns on transient evidence unless it also cites a durable commit', () => {
  const resolver = {
    active: () => true,
    refs: () => ['dev', 'master'],
    commits: (text) => text.includes('abc1234') ? [{ token: 'abc1234', sha: 'abc1234', ref: 'dev' }] : [],
  };
  const row = (n, evidence, status = 'met') => ({ n, text: `row ${n}`, status, evidence });
  const s = {
    cards: [{ num: 7, title: 'Seven', criteria: [
      row(1, 'ran /tmp/run-3/out.log'),
      row(2, 'green in the wave 12 tree'),
      row(3, 'cargo test passes'),
      row(4, 'cargo test passes; landed abc1234'),
      row(5, 'landed abc1234'),
      row(6, 'cargo test passes', 'open'),
    ] }],
    milestones: [{ id: 'm-1', exitCriteria: [row(1, 'rustc built it', 'verified')] }],
  };
  const findings = evidenceProvenance(s, resolver);
  assert.deepEqual(findings.map(f => [f.rule, f.ref, /criterion #(\d+)/.exec(f.msg)[1]]), [
    ['evidence-provenance', '#7', '1'], ['evidence-provenance', '#7', '2'], ['evidence-provenance', '#7', '3'], ['evidence-provenance', 'm-1', '1'],
  ]);
  assert.match(findings[0].msg, /cites a scratch path and no commit on dev or master/);
  assert.match(findings[1].msg, /cites a wave tree/);
  assert.match(findings[2].msg, /cites a Rust-era command/);

  const { data, st } = fixture();
  st.mutate((s2) => { Object.assign(s2.cards[0].criteria[0], { status: 'met', metBy: 'seed', evidence: 'cargo test green' }); });
  const r = cli(data, 'lint', '--evidence', '--json');
  assert.equal(r.status, 0, 'evidence findings are warnings');
  assert.deepEqual(JSON.parse(r.stdout).map(f => f.rule), ['evidence-provenance']);
});
