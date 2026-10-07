import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, main, merge, render as renderResult, sample, seconds, vague } from './check_coverage.mjs';

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

test('summary mode caps listed errors per rule but keeps complete counts', () => {
  const files = run(); files.claims.claims.forEach(row => { row.scale = 'wide'; });
  const text = renderResult(check(files), { perRule: 3 });
  assert.match(text, /\[schema\] \.\.\. 7 more/);
  assert.match(text, /by rule: schema 10, scale 2\nFAIL\n$/);
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

// Shards of the fixture: A holds the talk's first 5 minutes, B the rest of the talk plus the article.
function shards() {
  const full = run(); const [talkSource, article] = full.coverage.sources;
  const pick = (ids, sources) => ({
    coverage: { ...full.coverage, sources },
    ledger: { schema: full.ledger.schema, segments: Object.fromEntries(ids.map(id => [id, full.ledger.segments[id]])) },
    claims: { claims: full.claims.claims.filter(row => ids.includes(row.segment)) },
  });
  const a = pick(['example-talk/01'], [{ ...talkSource, range: { start_s: 0, end_s: 300 }, segments: [talkSource.segments[0]] }]);
  const b = pick(['example-talk/02', 'example-article/01', 'example-article/02'],
    [{ ...talkSource, range: { start_s: 300, end_s: 420 }, segments: [talkSource.segments[1]] }, article]);
  return { a: { dir: 'shard-a', ...a }, b: { dir: 'shard-b', ...b } };
}
const shape = (files, ignore = []) => rules(files).filter(line => !ignore.some(rule => line.startsWith(`${rule}|`)));

test('a ranged shard checks contiguity and densities against its absolute range', () => {
  const { a, b } = shards();
  assert.deepEqual(rules(a), []);
  const report = check(a).report.sources[0];
  assert.equal(report.size, 300);
  assert.match(renderResult(check(a)), /source example-talk \(video, range 00:00-05:00, 5\.0 min, 1 segments\)/);
  // B's talk half alone has only micro claims; everything else is clean.
  assert.deepEqual(shape(b, ['scale']), []);
  const short = shards().a; short.coverage.sources[0].range.end_s = 360;
  expectError(short, /^segment-gap\|source example-talk\|segments end at 05:00 but the range ends at 06:00/);
});

test('a shard must use absolute source times for segments, ledger and claim locators', () => {
  const { b } = shards(); const seg = b.coverage.sources[0].segments[0];
  seg.start = '00:00'; seg.end = '02:00';
  expectError(b, /^segment-gap\|segment example-talk\/02\|starts at 00:00 but the previous segment ended at 05:00/);
  expectError(b, /^ledger\|.*02\.L1\|at 05:20 is outside 00:00-02:00/);
  const relative = shards().b; relative.claims.claims.find(row => row.id === 'C5').locator = '00:20';
  expectError(relative, /^claim-segment\|claims\.json C5\|locator time 00:20 is outside the range 05:00-07:00; use absolute source times/);
  const whole = run(); claim(whole, 'C2').locator = '12:00';
  expectError(whole, /^claim-segment\|claims\.json C2\|locator time 12:00 is outside the source 00:00-07:00/);
  const past = shards().b; past.coverage.sources[0].range.end_s = 500;
  expectError(past, /^schema\|source example-talk\|range ends at 08:20, past the source duration 07:00/);
  const textRange = shards().b; textRange.coverage.sources[1].range = { start_s: 0, end_s: 10 };
  expectError(textRange, /^schema\|source example-article\|range applies only to video\/audio sources/);
  const bad = shards().a; bad.coverage.sources[0].range = { start_s: 300, end_s: 0 };
  expectError(bad, /^schema\|source example-talk\|range needs absolute start_s < end_s/);
});

test('merge joins shards into one contiguous source and re-checks', () => {
  const { a, b } = shards();
  const { errors, files } = merge([b, a]);
  assert.deepEqual(errors, []);
  const talkSource = files.coverage.sources.find(source => source.source_id === 'example-talk');
  assert.equal(talkSource.range, undefined);
  assert.deepEqual(talkSource.segments.map(row => row.id), ['example-talk/01', 'example-talk/02']);
  assert.deepEqual(files.coverage.merged_from, ['shard-b', 'shard-a']);
  assert.deepEqual(rules(files), []);
  // Shards that stop short keep a range, so the merged check still knows the covered span.
  const partial = shards(); partial.b.coverage.sources[0].range.end_s = 360;
  partial.b.coverage.sources[0].segments[0].end = '06:00';
  const joined = merge([partial.a, partial.b]).files;
  assert.deepEqual(joined.coverage.sources[0].range, { start_s: 0, end_s: 360 });
  const conflict = shards(); conflict.b.coverage.sources[0].title = 'Another talk';
  conflict.b.ledger.segments['example-talk/01'] = [];
  delete conflict.a.coverage.sources[0].range;
  const problems = merge([conflict.a, conflict.b]).errors.join('\n');
  assert.match(problems, /shard-b: source example-talk title differs from shard-a/);
  assert.match(problems, /shard-b: ledger segment example-talk\/01 is already in another shard/);
  assert.match(problems, /every shard needs a range/);
});

test('--merge writes the joined run, never overwrites, and exits by the re-check', t => {
  const scratch = process.env.JET_SCRATCH_ROOT || join(homedir(), '.cache/jet-dev/scratch');
  mkdirSync(scratch, { recursive: true });
  const root = mkdtempSync(join(scratch, 'check-coverage-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const shard of Object.values(shards())) {
    mkdirSync(join(root, shard.dir));
    for (const name of ['coverage', 'ledger', 'claims']) writeFileSync(join(root, shard.dir, `${name}.json`), JSON.stringify(shard[name]));
  }
  const args = ['--merge', join(root, 'shard-a'), join(root, 'shard-b'), '--out', join(root, 'merged')];
  const out = sink();
  assert.equal(main(args, out, sink()), 0);
  assert.match(out.data, /merged 2 shards into .*\nsource example-talk \(video, 7\.0 min, 2 segments\)[\s\S]*PASS\n$/);
  assert.ok(existsSync(join(root, 'merged', 'ledger.json')));
  assert.equal(main([join(root, 'merged')], sink(), sink()), 0);
  const again = sink();
  assert.equal(main(args, sink(), again), 1);
  assert.match(again.data, /already exists; merge never overwrites/);
  assert.equal(main(['--merge', join(root, 'shard-a'), '--out', join(root, 'x')], sink(), sink()), 2);
  assert.equal(main(['--merge', join(root, 'shard-a'), join(root, 'shard-b')], sink(), sink()), 2);
});
