import assert from 'node:assert/strict';
import test from 'node:test';
import { personalInfoMatcher, personalInfoRules } from '../tower/app/personal-info.mjs';
import { parseArgs, personalSpans } from './publish-release.mjs';

// publish moved to Tools/release (Jet); its tests are Tools/release/Tests.jet.
test('build takes its own options and refuses any other step', () => {
    const build = parseArgs(['build', 'abc123', '--seed', 'seed', '--out', 'out', '--threads', '4']);
    assert.deepEqual([build.command, build.target, build.seed, build.out, build.threads, build.errors], ['build', 'abc123', 'seed', 'out', 4, []]);
    assert.deepEqual(parseArgs(['build', 'abc123']).errors, ['--seed is required', '--out is required']);
    assert.deepEqual(parseArgs(['build', 'a', 'b', '--seed', 's', '--out', 'o']).errors, ['one commit, not a and b']);
    assert.deepEqual(parseArgs(['build', 'a', '--seed', 's', '--out', 'o', '--threads', '0']).errors, ['--threads needs a positive whole number, not 0']);
    assert.deepEqual(parseArgs(['build', 'a', '--seed', 's', '--out', 'o', '--dry-run']).errors, ['unknown option --dry-run']);
    const gateOnly = parseArgs(['build', 'a', '--seed', 's', '--check-gate']);
    assert.deepEqual([gateOnly.checkGate, gateOnly.errors], [true, []]);
    assert.deepEqual(parseArgs(['publish', 'out']).errors, ['unknown command publish: want build (Tools/release/Main.jet gates and publishes)']);
    assert.deepEqual(parseArgs([]).errors, ['unknown command (none): want build (Tools/release/Main.jet gates and publishes)']);
});

test('shipped bytes naming personal information or a build directory are found and neutral text is not', () => {
    const matcher = personalInfoMatcher(personalInfoRules({ home: '/home/ada', user: 'ada', host: 'lovelace', scratchRoot: '/mnt/scratch', mounts: ['/mnt/scratch'] }));
    const dirs = ['/srv/build/out'];
    assert.deepEqual(personalSpans(Buffer.from('dir\0/mnt/scratch/snap/Compiler/JetCli\0'), matcher, dirs), ['/mnt/scratch']);
    assert.deepEqual(personalSpans(Buffer.from('built on lovelace in /srv/build/out/jet by /home/ada'), matcher, dirs), ['/home/ada', 'lovelace', 'ada', '/srv/build/out']);
    // `ada` inside a longer word (`canadian`) is not the user name.
    assert.deepEqual(personalSpans(Buffer.from('Compiler/JetCli\0<corelib>/Core/canadian.jet'), matcher, dirs), []);
});
