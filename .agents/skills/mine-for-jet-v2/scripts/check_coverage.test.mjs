import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, main, sample, seconds, vague } from './check_coverage.mjs';

const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), 'fixtures', 'coverage-pass');
const read = name => JSON.parse(readFileSync(join(FIXTURE, name), 'utf8'));
const run = () => ({ coverage: read('coverage.json'), ledger: read('ledger.json'), claims: read('claims.json'), audit: read('recall-audit.json') });
const talk = files => files.coverage.sources[0];
const segment = (files, id) => files.coverage.sources.flatMap(source => source.segments).find(row => row.id === id);
const claim = (files, id) => files.claims.claims.find(row => row.id === id);
const rules = (files, options) => check(files, options).errors.map(error => `${error.rule}|${error.where}|${error.message}`);
const expectError = (files, pattern, options) => {
  const found = rules(files, options);
  assert.ok(found.some(line => pattern.test(line)), `expected ${pattern} in:\n${found.join('\n')}`);
};
const sink = () => { let data = ''; return { write: chunk => { data += chunk; }, get data() { return data; } }; };

test('passing fixture is clean with and without the recall audit', () => {
  assert.deepEqual(rules(run()), []);
  assert.deepEqual(rules(run(), { requireAudit: true }), []);
  const out = sink();
  assert.equal(main([FIXTURE, '--audit'], out, sink()), 0);
  assert.match(out.data, /example-talk\/01 00:00-05:00: ledger 6 \(1\.20\/min\), claims 4, floor 5 - ok/);
  assert.match(out.data, /PASS\n$/);
});

test('missing run files fail with exit 1', () => {
  const out = sink();
  assert.equal(main([join(FIXTURE, 'absent')], out, sink()), 1);
  assert.match(out.data, /coverage\.json: missing/);
  assert.match(out.data, /ledger\.json: missing/);
  assert.equal(main([], sink(), sink()), 2);
});

test('video segments are bounded and contiguous', () => {
  const long = run(); segment(long, 'example-talk/01').end = '05:30'; segment(long, 'example-talk/02').start = '05:30';
  segment(long, 'example-talk/02').end = '07:00';
  expectError(long, /^segment-bound\|segment example-talk\/01\|05:30 long/);
  const gap = run(); segment(gap, 'example-talk/02').start = '05:10'; segment(gap, 'example-talk/02').end = '07:00';
  expectError(gap, /^segment-gap\|segment example-talk\/02\|starts at 05:10/);
  const short = run(); talk(short).duration_s = 600;
  expectError(short, /^segment-gap\|source example-talk\|segments end at 07:00 but the source runs 10:00/);
  const blocked = run(); segment(blocked, 'example-talk/02').body_review = 'partial';
  expectError(blocked, /^body\|segment example-talk\/02\|body_review is "partial"/);
});

test('text segments stay under the word bound and add up to the source', () => {
  const files = run(); segment(files, 'example-article/01').words = 1600; files.coverage.sources[1].words = 2000;
  expectError(files, /^segment-bound\|segment example-article\/01\|1600 words/);
  const sum = run(); sum.coverage.sources[1].words = 1400;
  expectError(sum, /^segment-gap\|source example-article\|segments hold 1000 of 1400 words/);
});

test('repository segments are single modules covering the inventory', () => {
  const files = run();
  files.coverage.sources.push({ source_id: 'example-repo', kind: 'repo', title: 'Example build tool', ref: 'https://example.org/repo',
    modules: ['src/cache', 'src/cli', 'docs'], excluded: [{ path: 'docs', reason: 'not applicable' }],
    lanes_not_run: { 'audience-corrections': 'No issue tracker sample was requested for the example build tool.' },
    segments: [{ id: 'example-repo/cache', title: 'Cache key module', path: 'src/cache,src/cli', retrieval: 'captured', body_review: 'full' }] });
  expectError(files, /^segment-bound\|segment example-repo\/cache\|repository segment is exactly one module/);
  expectError(files, /^segment-gap\|source example-repo\|module src\/cli is neither a segment nor excluded/);
  expectError(files, /^reason\|source example-repo excluded docs\|/);
  expectError(files, /^ledger\|ledger example-repo\/cache\|no concept ledger/);
});

test('every ledger item is mapped and every claim is traced to the ledger', () => {
  const files = run(); delete files.ledger.segments['example-talk/01'][5].out_of_scope;
  expectError(files, /^ledger-unmapped\|ledger example-talk\/01 example-talk\/01\.L6\|unmapped/);
  const both = run(); both.ledger.segments['example-talk/01'][0].out_of_scope = 'Duplicate mapping for the arena item.';
  expectError(both, /^ledger-unmapped\|.*L1\|maps to claims and out_of_scope/);
  const foreign = run(); foreign.ledger.segments['example-talk/01'][0].claims = ['C5'];
  expectError(foreign, /^ledger\|.*L1\|claim C5 belongs to segment example-talk\/02/);
  const orphan = run(); orphan.ledger.segments['example-talk/01'][3].claims = ['C2'];
  expectError(orphan, /^ledger\|ledger example-talk\/01\|claim C3 is not traced/);
  const outside = run(); outside.ledger.segments['example-talk/02'][0].at = '04:00';
  expectError(outside, /^ledger\|.*02\.L1\|at 04:00 is outside 05:00-07:00/);
  const empty = run(); delete empty.ledger.segments['example-article/02'];
  expectError(empty, /^ledger\|ledger example-article\/02\|no concept ledger/);
  const generic = run(); generic.ledger.segments['example-talk/01'][5].out_of_scope = 'out of scope, not relevant';
  expectError(generic, /^reason\|.*L6 out_of_scope\|reason too short/);
  const noRef = run(); delete noRef.ledger.segments['example-talk/02'][2].covered.ref;
  expectError(noRef, /^ledger-unmapped\|.*02\.L3\|covered needs the Jet ref/);
});

test('every segment x lane row has claims or a specific, segment-specific reason', () => {
  const missing = run(); delete segment(missing, 'example-talk/02').lanes['proven-experience'];
  expectError(missing, /^lane\|segment example-talk\/02 lane proven-experience\|missing row/);
  for (const reason of ['not applicable', 'Nothing relevant in this segment.', 'N/A: no finding, no delta, nothing new for Jet here at all']) {
    const files = run(); segment(files, 'example-talk/02').lanes['proven-experience'] = { reason };
    expectError(files, /^reason\|segment example-talk\/02 lane proven-experience\|(reason too short|generic reason)/);
  }
  const unrelated = run();
  segment(unrelated, 'example-talk/02').lanes['proven-experience'] = { reason: 'Quarterly revenue projections for shipping containers dominate here.' };
  expectError(unrelated, /^reason\|segment example-talk\/02 lane proven-experience\|reason names nothing from this segment/);
  const copied = run();
  segment(copied, 'example-talk/02').lanes['interop-and-portability'] = { ...segment(copied, 'example-talk/01').lanes['interop-and-portability'] };
  expectError(copied, /^reason\|segment example-talk\/02 lane interop-and-portability\|same reason as segment example-talk\/01/);
  const lost = run(); segment(lost, 'example-talk/01').lanes['cost-and-measurement'] = { reason: 'The arena benchmark numbers are claimed elsewhere in this arena walkthrough.' };
  expectError(lost, /^lane\|segment example-talk\/01 lane cost-and-measurement\|claim C2 is missing from its own lane row/);
  const conditional = run(); talk(conditional).lanes_not_run['expert-descent'] = 'The expert lane was not requested for this arena talk.';
  expectError(conditional, /^lane\|source example-talk\|lane expert-descent is not conditional/);
});

test('low-density segments need a specific written justification', () => {
  const files = run(); files.ledger.segments['example-talk/01'].splice(5, 1);
  files.ledger.segments['example-talk/01'].splice(1, 1);
  expectError(files, /^density\|segment example-talk\/01\|4 ledger items \/ 4 claims \(floor 5 \/ 1\): low density needs density_justification/);
  segment(files, 'example-talk/01').density_justification = 'The arena walkthrough spends two minutes on one benchmark table, repeated twice.';
  assert.deepEqual(rules(files), []);
  assert.equal(check(files).report.sources[0].segments[0].status, 'LOW, justified');
});

test('claims are schema-checked, segment-linked and cover both scales', () => {
  const files = run(); claim(files, 'C2').observed = 'registry'; claim(files, 'C3').review_disposition = 'no-action';
  delete claim(files, 'C4').segment; delete claim(files, 'C4').topic;
  expectError(files, /^schema\|claims\.json C2\|observed must be one of/);
  expectError(files, /^schema\|claims\.json C3\|review_disposition must be one of/);
  expectError(files, /^claim-segment\|claims\.json C4\|primary claim has no segment id/);
  expectError(files, /^schema\|claims\.json C4\|missing topic/);
  const scale = run(); claim(scale, 'C1').scale = 'micro';
  expectError(scale, /^scale\|source example-talk\|no macro claims: needs scale_justification/);
  const local = run(); local.claims.claims.push({ ...claim(local, 'C1'), id: 'L1', source_id: 'local', source_kind: 'local-evidence', segment: undefined });
  assert.deepEqual(rules(local), []);
});

test('recall audit sample is deterministic, sized max(2, 10%) and fails on misses', () => {
  assert.equal(seconds('1:27:00'), 5220); assert.equal(seconds('05:40'), 340); assert.ok(Number.isNaN(seconds('5m')));
  const many = { sources: [{ source_id: 's', segments: Array.from({ length: 31 }, (_, i) => ({ id: `s/${i}` })) },
    { source_id: 't', segments: [{ id: 't/0' }] }] };
  const [first, second] = sample(many, 'seed-a');
  assert.equal(first.segments.length, 4); assert.deepEqual(second.segments, ['t/0']);
  assert.deepEqual(sample(many, 'seed-a'), sample(many, 'seed-a'));
  assert.notDeepEqual(sample(many, 'seed-a')[0].segments, sample(many, 'seed-b')[0].segments);
  const missed = run(); missed.audit.segments[0].concepts.push({ at: '01:00', concept: 'Arena reset ordering', ledger: null });
  expectError(missed, /^recall\|recall-audit example-talk\/01\|missed concept at 01:00: Arena reset ordering; batch returns/, { requireAudit: true });
  const skipped = run(); skipped.audit.segments.pop();
  expectError(skipped, /^recall\|recall-audit example-article\/02\|sampled segment was not re-mined/, { requireAudit: true });
  const disputed = run(); disputed.audit.segments[1].disputes = [{ ledger: 'example-talk/02.L3', reason: 'profiling advice is a distinct claim' }];
  expectError(disputed, /^recall\|recall-audit example-talk\/02\|disputed mapping of example-talk\/02\.L3/, { requireAudit: true });
  const none = run(); delete none.audit;
  expectError(none, /^recall\|recall-audit\.json\|missing/, { requireAudit: true });
});

test('vague accepts specific reasons and rejects boilerplate', () => {
  assert.equal(vague('Sponsor read for a VPN service with no language or tooling mechanism', 'sponsor VPN'), null);
  assert.match(vague('not applicable'), /too short/);
  assert.match(vague('no finding here, nothing new, already covered by Jet lanes'), /generic/);
});
