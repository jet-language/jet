import assert from 'node:assert/strict';
import test from 'node:test';
import { personalInfoMatcher, personalInfoRules } from '../tower/app/personal-info.mjs';
import { admissionProblems, evidenceProblems, githubSlug, parseArgs, personalSpans, releaseNotes, remoteTags } from './publish-release.mjs';

test('build and publish take their own options and refuse the other step\'s', () => {
    const build = parseArgs(['build', 'abc123', '--seed', 'seed', '--out', 'out', '--threads', '4']);
    assert.deepEqual([build.command, build.target, build.seed, build.out, build.threads, build.errors], ['build', 'abc123', 'seed', 'out', 4, []]);
    const publish = parseArgs(['publish', 'out', '--evidence', 'ev', '--dry-run']);
    assert.deepEqual([publish.command, publish.target, publish.evidence, publish.dryRun, publish.errors], ['publish', 'out', 'ev', true, []]);
    assert.deepEqual(parseArgs(['build', 'abc123']).errors, ['--seed is required', '--out is required']);
    assert.deepEqual(parseArgs(['publish', 'out', '--seed', 's', '--evidence', 'ev']).errors, ['--seed, --out and --threads belong to build']);
    assert.deepEqual(parseArgs(['build', 'a', 'b', '--seed', 's', '--out', 'o']).errors, ['one commit, not a and b']);
    assert.deepEqual(parseArgs(['build', 'a', '--seed', 's', '--out', 'o', '--threads', '0']).errors, ['--threads needs a positive whole number, not 0']);
    assert.deepEqual(parseArgs(['release']).errors, ['unknown command release: want build or publish']);
    assert.deepEqual(parseArgs([]).errors, ['unknown command (none): want build or publish']);
});

test('shipped bytes naming personal information or a build directory are found and neutral text is not', () => {
    const matcher = personalInfoMatcher(personalInfoRules({ home: '/home/ada', user: 'ada', host: 'lovelace', scratchRoot: '/mnt/scratch', mounts: ['/mnt/scratch'] }));
    const dirs = ['/srv/build/out'];
    assert.deepEqual(personalSpans(Buffer.from('dir\0/mnt/scratch/snap/Compiler/JetCli\0'), matcher, dirs), ['/mnt/scratch']);
    assert.deepEqual(personalSpans(Buffer.from('built on lovelace in /srv/build/out/jet by /home/ada'), matcher, dirs), ['/home/ada', 'lovelace', 'ada', '/srv/build/out']);
    // `ada` inside a longer word (`canadian`) is not the user name.
    assert.deepEqual(personalSpans(Buffer.from('Compiler/JetCli\0<corelib>/Core/canadian.jet'), matcher, dirs), []);
});

test('the GitHub slug comes from an ssh or https remote', () => {
    assert.equal(githubSlug('git@github.com:jet-language/jet'), 'jet-language/jet');
    assert.equal(githubSlug('https://github.com/jet-language/jet.git\n'), 'jet-language/jet');
    assert.equal(githubSlug('https://gitlab.com/jet-language/jet.git'), '');
});

test('a remote tag names the commit its peeled row gives', () => {
    const text = 'aaa\trefs/tags/a0_021\nbbb\trefs/tags/a0_021^{}\nccc\trefs/tags/a0_025\n';
    assert.deepEqual([...remoteTags(text)], [['a0_021', 'bbb'], ['a0_025', 'ccc']]);
});

test('an admitted release needs a full passing RELEASE.json of its own identity', () => {
    const identity = `jet@0.0.1#${'a'.repeat(64)}`;
    const good = { schema: 'jet-release/v1', name: 'a0_026', commit: 'c'.repeat(40), verdict: 'PASS', scope: 'full', identity };
    assert.deepEqual(admissionProblems(good, identity), []);
    assert.deepEqual(admissionProblems({ ...good, verdict: 'FAIL', scope: 'filtered: io', identity: 'jet@0.0.1#b' }, identity), [
        'verdict is FAIL, not PASS',
        'scope is filtered: io, not full: a filtered gate is not a release',
        `admitted jet@0.0.1#b, not ${identity}`,
    ]);
    assert.deepEqual(admissionProblems({ ...good, name: '' }, identity), ['name or commit is missing']);
});

test('publish needs a PASS gate pinned to the commit and run with the release\'s jet', () => {
    const shaOf = path => ({ '/out/jet': 'f'.repeat(64), '/other/jet': 'e'.repeat(64) })[path] ?? '';
    const good = { status: 'PASS', head: 'c0ffee', jet: '/out/jet', rows: [{ scope: 'closeout-gate', status: 'PASS' }] };
    assert.deepEqual(evidenceProblems(good, 'c0ffee', 'f'.repeat(64), shaOf), []);
    const bad = { status: 'FAIL', head: 'beef', jet: '/other/jet', rows: [{ scope: 'cases', status: 'FAIL' }] };
    assert.deepEqual(evidenceProblems(bad, 'c0ffee', 'f'.repeat(64), shaOf), [
        'status is FAIL, not PASS',
        '1 row(s) not PASS: cases FAIL',
        'pinned to beef, not c0ffee',
        `gated /other/jet (${'e'.repeat(16)}), not the release's jet (${'f'.repeat(16)})`,
    ]);
    assert.deepEqual(evidenceProblems({ status: 'PASS', head: 'c0ffee', jet: '/out/jet', rows: [] }, 'c0ffee', 'f'.repeat(64), shaOf), ['no rows: the gate ran nothing']);
});

test('the release notes carry the full identity, the commit, the seed and the fetch command', () => {
    const identity = `jet@0.0.1#${'a'.repeat(64)}`;
    const notes = releaseNotes({ name: 'a0_026', identity, commit: 'c'.repeat(40), seed: `jet@0.0.1#${'b'.repeat(64)}`, gate: 'verify-full.sh passed (7 checks)', slug: 'jet-language/jet', asset: 'jet-a0_026-linux-x64.tar.gz' });
    assert.ok(notes.includes(`Identity: \`${identity}\``));
    assert.ok(notes.includes(`Commit: ${'c'.repeat(40)}`));
    assert.ok(notes.includes(`Seed: \`jet@0.0.1#${'b'.repeat(64)}\``));
    assert.ok(notes.includes('- Gate: verify-full.sh passed (7 checks).'));
    assert.ok(notes.includes("gh release download -R jet-language/jet -p 'jet-a0_*-linux-x64.tar.gz*'"));
});
