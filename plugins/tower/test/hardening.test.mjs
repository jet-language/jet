import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const TOWER = join(ROOT, 'plugins/tower/tower.mjs');
const SCRATCH_ROOT = process.env.JET_TEST_SCRATCH || '/home/nate/.cache/jet-test-scratch';
mkdirSync(SCRATCH_ROOT, { recursive: true });

function board(name) {
  const root = mkdtempSync(join(SCRATCH_ROOT, 'tower-hardening-'));
  execFileSync(process.execPath, [TOWER, 'init', '--dir', root, '--name', name], {
    cwd: ROOT, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' },
  });
  return { root, data: join(root, '.tower') };
}

function cli(data, args, input = null, ok = true) {
  try {
    return {
      out: execFileSync(process.execPath, [TOWER, '--data', data, ...args], {
        cwd: ROOT, encoding: 'utf8', input, env: { ...process.env, TOWER_DATA: '' },
      }),
      code: 0,
    };
  } catch (error) {
    if (ok) throw error;
    return { out: (error.stdout || '') + (error.stderr || ''), code: error.status };
  }
}

function finding(overrides = {}) {
  const seed = overrides.seed || 'seed-1';
  const payload = {
    title: overrides.title || 'Hardening finding',
    hardeningDedupKey: overrides.hardeningDedupKey || `raw-${seed}`,
    hardeningSeam: overrides.hardeningSeam || 'packed-int',
    hardeningRelation: overrides.hardeningRelation || 'value relation holds',
    hardeningWrongTierMask: overrides.hardeningWrongTierMask || ['aot', 'jet_run'],
    hardeningInputPartition: overrides.hardeningInputPartition || 'minimum',
    source: overrides.source || `fn run() { print("${seed}") }`,
    commands: overrides.commands || ['scripts/agent/jet-env jet run repro.jet', 'scripts/agent/jet-env jet run --release repro.jet'],
    expectedRelation: overrides.expectedRelation || 'equal',
    actualRelation: overrides.actualRelation || 'different',
    seed,
    targetCommit: overrides.targetCommit || 'commit-abc',
    bundleDigest: overrides.bundleDigest || `sha256:${seed}`,
    ...overrides,
  };
  delete payload.bundleDigest;
  delete payload.bundle_digest;
  return payload;
}

function add(data, payload, extra = [], ok = true) {
  return cli(data, ['card', 'add', '--stdin', '--json', '--by', 'hardening-rig', ...extra], JSON.stringify(payload), ok);
}

function addAsync(data, payload) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [TOWER, '--data', data, 'card', 'add', '--stdin', '--json', '--by', 'hardening-rig'], {
      cwd: ROOT, env: { ...process.env, TOWER_DATA: '' },
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    let out = '';
    let err = '';
    child.stdout.on('data', chunk => { out += chunk; });
    child.stderr.on('data', chunk => { err += chunk; });
    child.on('close', code => resolve({ code, out, err }));
    child.stdin.end(JSON.stringify(payload));
  });
}

function cards(data) {
  return JSON.parse(cli(data, ['card', 'list', '--json']).out);
}

function removeBoard(root) {
  rmSync(root, { recursive: true, force: true });
}

test('concurrent hardening adds are atomic and append evidence to one stable card', async () => {
  const b = board('Concurrent hardening');
  try {
    const payload = finding({
      hardeningDedupKey: 'runner-key',
      bundleDigest: 'sha256:runner-bundle',
    });
    const results = await Promise.all(Array.from({ length: 8 }, () => addAsync(b.data, payload)));
    assert.deepEqual(results.map(result => result.code), Array(8).fill(0));
    const saved = results.map(result => JSON.parse(result.out));
    assert.ok(saved.every(result => result.id === saved[0].id));
    const list = cards(b.data);
    assert.equal(list.length, 1);
    assert.equal(list[0].id, saved[0].id);
    assert.match(list[0].body, /Minimized reproducer:/);
    assert.match(list[0].body, /Exact commands:/);
    assert.match(list[0].body, /Expected relation: equal/);
    assert.match(list[0].body, /Actual relation: different/);
    assert.match(list[0].body, /Seed: seed-1/);
    assert.match(list[0].body, /Target commit: commit-abc/);
    assert.match(list[0].body, /Bundle digest: sha256:[0-9a-f]{64}/);
    const events = JSON.parse(cli(b.data, ['events', '--json']).out);
    assert.ok(events.filter(event => event.action === 'hardening.card-upsert').every(event => event.by === 'hardening-rig'));
  } finally {
    removeBoard(b.root);
  }
});

test('hardening evidence rejects a forged bundle digest', () => {
  const b = board('Hardening digest');
  try {
    const rejected = add(b.data, {
      ...finding({ hardeningDedupKey: 'digest-key' }),
      bundleDigest: 'sha256:forged',
    }, [], false);
    assert.equal(rejected.code, 1);
    assert.match(rejected.out, /bundle digest does not match its canonical evidence/);
    assert.equal(cards(b.data).length, 0);
  } finally {
    removeBoard(b.root);
  }
});

test('canonical root seam joins map, JSON, and Codable evidence but separates other seams', () => {
  const b = board('Hardening identity');
  try {
    const base = { hardeningRelation: 'packed value agrees', hardeningWrongTierMask: ['jet_run'], hardeningInputPartition: 'extreme' };
    const map = JSON.parse(add(b.data, finding({ ...base, title: 'Map output', hardeningDedupKey: 'map-key', seed: 'map', bundleDigest: 'sha256:map' })).out);
    const json = JSON.parse(add(b.data, finding({ ...base, title: 'JSON output', hardeningDedupKey: 'json-key', seed: 'json', bundleDigest: 'sha256:json' })).out);
    const codable = JSON.parse(add(b.data, finding({ ...base, title: 'Codable output', hardeningDedupKey: 'codable-key', seed: 'codable', bundleDigest: 'sha256:codable' })).out);
    const other = JSON.parse(add(b.data, finding({ ...base, title: 'Equality output', hardeningSeam: 'interpreter-equality', hardeningDedupKey: 'equality-key', seed: 'equality', bundleDigest: 'sha256:equality' })).out);
    assert.equal(map.id, json.id);
    assert.equal(map.id, codable.id);
    assert.notEqual(map.id, other.id);
    assert.equal(cards(b.data).length, 2);
    assert.deepEqual(new Set(json.hardeningDedupAliases), new Set(['map-key', 'json-key']));
    assert.equal(other.hardeningSeam, 'interpreter-equality');
  } finally {
    removeBoard(b.root);
  }
});

test('unknown seam aliases converge through triage and survive archive reuse', () => {
  const b = board('Hardening aliases');
  try {
    const first = JSON.parse(add(b.data, finding({
      hardeningDedupKey: 'legacy-finding-key', hardeningSeam: 'future-unknown-seam',
      hardeningFindingId: 'F-ARCHIVE', bundleDigest: 'sha256:archive-1', seed: 'archive-1',
    })).out);
    assert.equal(first.hardeningSeam, 'unclassified.semantic-primitive');
    const triaged = JSON.parse(cli(b.data, [
      'card', 'update', '#1', '--hardening-seam', 'packed-int', '--json', '--by', 'hardening-rig',
    ]).out);
    assert.equal(triaged.hardeningSeam, 'packed-int-representation');
    assert.ok(triaged.hardeningDedupAliases.includes('legacy-finding-key'));
    const oldLookup = JSON.parse(cli(b.data, ['card', 'show', '--hardening-dedup-key', 'legacy-finding-key', '--json']).out);
    const newLookup = JSON.parse(cli(b.data, ['card', 'show', '--hardening-dedup-key', triaged.hardeningDedupKey, '--json']).out);
    assert.equal(oldLookup.id, triaged.id);
    assert.equal(newLookup.id, triaged.id);

    const fixture = join(b.root, 'tests', 'conformance', 'corpus', 'F-ARCHIVE.jet');
    mkdirSync(dirname(fixture), { recursive: true });
    writeFileSync(fixture, '// F-ARCHIVE permanent regression fixture\n');
    cli(b.data, ['card', 'update', '#1', '--hardening-fixture', 'tests/conformance/corpus/F-ARCHIVE.jet', '--by', 'hardening-rig']);
    cli(b.data, ['card', 'criteria', '#1', '--add', 'fixture is present', '--by', 'builder']);
    cli(b.data, ['card', 'criteria', '#1', '--meet', '1', '--evidence', 'fixture checked in', '--by', 'builder']);
    cli(b.data, ['card', 'update', '#1', '--phase', 'done', '--by', 'builder']);
    cli(b.data, ['card', 'update', '#1', '--log', 'completion acknowledged', '--by', 'builder']);
    const statePath = join(b.data, 'tower.json');
    const state = JSON.parse(readFileSync(statePath, 'utf8'));
    state.cards[0].updated = '2000-01-01';
    state.cards[0].completedAt = '2000-01-01T00:00:00.000Z';
    state.meta.completionCursor = '2099-01-01T00:00:00.000Z';
    writeFileSync(statePath, JSON.stringify(state, null, 2) + '\n');
    cli(b.data, ['card', 'add', '--title', 'retirement trigger', '--by', 'builder']);
    const archived = JSON.parse(cli(b.data, ['card', 'show', '--hardening-dedup-key', triaged.hardeningDedupKey, '--json']).out);
    assert.equal(archived.id, first.id);
    assert.equal(archived.archived, true);
    const recurrence = JSON.parse(add(b.data, finding({
      hardeningDedupKey: 'legacy-finding-key', hardeningSeam: 'packed-int',
      hardeningFindingId: 'F-ARCHIVE', bundleDigest: 'sha256:archive-2', seed: 'archive-2',
    })).out);
    assert.equal(recurrence.id, first.id);
    assert.equal(recurrence.phase, 'building');
    assert.equal(JSON.parse(cli(b.data, ['card', 'list', '--json']).out).length, 2);
  } finally {
    removeBoard(b.root);
  }
});

test('severity defaults P0 for silent/default divergence and P1 for loud non-default failure', () => {
  const b = board('Hardening severity');
  try {
    const silent = JSON.parse(add(b.data, finding({
      hardeningDedupKey: 'silent', classification: 'silent-data', seed: 'silent', bundleDigest: 'sha256:silent',
    })).out);
    const defaultRun = JSON.parse(add(b.data, finding({
      hardeningDedupKey: 'default-run', defaultJetRunDivergence: true, tier: 'jet_run', seed: 'default', bundleDigest: 'sha256:default',
    })).out);
    const loud = JSON.parse(add(b.data, finding({
      hardeningDedupKey: 'loud-aot', hardeningSeam: 'aot-emission', loudFailure: true, exit: 1,
      tier: 'aot', seed: 'loud', bundleDigest: 'sha256:loud',
    })).out);
    assert.equal(silent.priority, 'P0');
    assert.equal(defaultRun.priority, 'P0');
    assert.equal(loud.priority, 'P1');
  } finally {
    removeBoard(b.root);
  }
});

test('hardening card cannot close without a fixture naming its finding and recurrence reopens it', () => {
  const b = board('Hardening fixture gate');
  try {
    const payload = finding({ hardeningDedupKey: 'fixture-key', hardeningFindingId: 'F-FIXTURE', seed: 'fixture', bundleDigest: 'sha256:fixture' });
    const first = JSON.parse(add(b.data, payload).out);
    cli(b.data, ['card', 'criteria', '#1', '--add', 'fixture is checked in', '--by', 'builder']);
    cli(b.data, ['card', 'criteria', '#1', '--meet', '1', '--evidence', 'waiting for file', '--by', 'builder']);
    const refused = cli(b.data, ['card', 'update', '#1', '--phase', 'done', '--by', 'builder'], null, false);
    assert.equal(refused.code, 1);
    assert.match(refused.out, /E_HARDENING_FIXTURE|permanent corpus fixture/);

    const fixture = join(b.root, 'tests', 'conformance', 'corpus', 'F-FIXTURE.jet');
    mkdirSync(dirname(fixture), { recursive: true });
    writeFileSync(fixture, '// finding F-FIXTURE\n');
    cli(b.data, ['card', 'update', '#1', '--hardening-fixture', 'tests/conformance/corpus/F-FIXTURE.jet', '--by', 'hardening-rig']);
    cli(b.data, ['card', 'update', '#1', '--body', 'human triage note', '--by', 'builder']);
    const retained = JSON.parse(cli(b.data, ['card', 'show', '#1', '--json']).out);
    assert.match(retained.body, /Minimized reproducer:/);
    assert.match(retained.body, /human triage note/);
    cli(b.data, ['card', 'update', '#1', '--phase', 'done', '--by', 'builder']);
    const fixed = JSON.parse(cli(b.data, ['card', 'show', '#1', '--json']).out);
    assert.equal(fixed.hardeningState, 'fixed');
    const reopened = JSON.parse(add(b.data, {
      ...payload,
      source: 'fn run() { print("recurrence") }',
      seed: 'recurrence',
    }).out);
    assert.equal(reopened.id, first.id);
    assert.equal(reopened.phase, 'building');
    assert.equal(reopened.hardeningState, 'open');
  } finally {
    removeBoard(b.root);
  }
});

test('hardening path rejects force and never uses fuzzy duplicate matching', () => {
  const b = board('Hardening write path');
  try {
    add(b.data, finding({ hardeningDedupKey: 'strict-key', title: 'same symptom', body: 'same body', seed: 'one', bundleDigest: 'sha256:one' }));
    const forced = add(b.data, finding({ hardeningDedupKey: 'different-key', title: 'same symptom', body: 'same body', seed: 'two', bundleDigest: 'sha256:two' }), ['--force'], false);
    assert.equal(forced.code, 1);
    assert.match(forced.out, /cannot use --force/);
    add(b.data, finding({
      hardeningDedupKey: 'different-key', hardeningSeam: 'input-transport', title: 'same symptom', body: 'same body',
      seed: 'two', bundleDigest: 'sha256:two',
    }));
    assert.equal(cards(b.data).length, 2);
  } finally {
    removeBoard(b.root);
  }
});
