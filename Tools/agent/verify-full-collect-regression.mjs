#!/usr/bin/env node
// Real native verifier with tiny fixture children; this is runner behavior,
// never compiler or full-suite proof. No Cargo test adapter is involved.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const base = process.env.JET_TEST_SCRATCH_DIR ?? join(homedir(), '.cache/jet-dev/scratch');
mkdirSync(base, { recursive: true });
const container = mkdtempSync(join(base, 'verify-native-fixture-'));
const sandbox = join(container, 'source');
mkdirSync(sandbox);
const external = mkdtempSync(join(base, 'verify-native-evidence-'));
const put = (name, content) => { const path = join(sandbox, name); mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, content, { mode: 0o755 }); };
try {
    for (const path of ['Tools/agent/verify-full.sh', 'Tools/agent/verify-full.mjs', 'Tools/agent/native-suite.mjs']) put(path, readFileSync(join(repo, path)));
    for (const gate of ['closeout-gate', 'check-unsafe-ratchet', 'check-agent-doc-flags', 'decision-index', 'core-coverage-census']) put(`Tools/agent/${gate}.mjs`, `if (process.env.FIXTURE_MODE === 'closeout' && ${JSON.stringify(gate)} === 'closeout-gate') process.exit(3);\n`);
    put('Tools/ci/test-shards.sh', '#!/usr/bin/env bash\nprintf "%s\\n" golden ui cases registries\n');
    put('Tools/agent/proof-source.mjs', `import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
export function assertPinnedSource(source, commit) {if(existsSync(source+'/.corrupted'))throw Error('pinned source changed');return {commit};}
export function proofTargetDirectory(commit) {return '/mnt/jetscratch/targets/source-'+commit;}
if(process.argv[1]===fileURLToPath(import.meta.url))console.log(process.argv[3]);
`);
    // The fake compiler answers only `jet test <dir>/Tools/testkit "suite <name>" ...`, as the
    // fixture gate does: the case verdicts go to $JET_CI_EVIDENCE_DIR/<name>.json.
    put('bin/jet', `#!/usr/bin/env node
const {writeFileSync}=require('node:fs');
const args=process.argv.slice(2),suite=(args[2]??'').split(' ')[1],mode=process.env.FIXTURE_MODE;
if(args[0]!=='test'||!args[1].endsWith('/Tools/testkit'))process.exit(64);
if(suite==='golden'&&mode==='signal')process.kill(process.pid,'SIGTERM');
if(suite==='golden'&&mode==='ordinary')process.exit(17);
const results=[{case:suite,suite,tier:'o0',status:suite==='ui'&&mode==='refused'?'REFUSED':'PASS'}];
if(suite==='ui'&&mode==='corrupt')writeFileSync('.corrupted','changed');
writeFileSync(process.env.JET_CI_EVIDENCE_DIR+'/'+suite+'.json',JSON.stringify({jet:process.env.JET,results}));
`);
    put('Tools/agent/compiler-registries.mjs', `import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
const args=process.argv.slice(2);
if(args[0]==='manifest'){const dir=args[args.indexOf('--out')+1];mkdirSync(dir,{recursive:true});writeFileSync(dir+'/manifest.json',JSON.stringify({head:args[1],repo:process.cwd(),completeCompilerRegistries:true,entries:[{id:'fixture'}]}));}
else {if(process.env.FIXTURE_MODE==='registry')process.exit(1);const manifest=JSON.parse(readFileSync(args[args.indexOf('--manifest')+1]));writeFileSync(args[args.indexOf('--receipt')+1],JSON.stringify({status:'PASS',head:manifest.head,jet:process.env.JET,total:1,passed:1,blocked:0,failed:0,entries:[{id:'fixture',status:'PASS'}]}));}
`);
    put('bin/git', '#!/usr/bin/env bash\nif [[ "$FIXTURE_MODE" == pinned ]]; then exit 19; fi\nprintf "0123456789012345678901234567890123456789\\n"\n');
    const outcomes = [];
    for (const mode of ['pass', 'ordinary', 'refused', 'signal', 'registry', 'corrupt', 'closeout', 'pinned']) {
        rmSync(join(sandbox, '.corrupted'), { force: true });
        if (mode === 'pinned') writeFileSync(join(container, 'source.manifest.json'), JSON.stringify({ commit: '0123456789012345678901234567890123456789' }));
        const evidence = join(external, mode);
        // The timeout only catches a hang; a loaded machine (release preflights run beside builds) needs headroom.
        const ran = spawnSync('bash', [join(sandbox, 'Tools/agent/verify-full.sh')], { cwd: sandbox, env: { ...process.env, PATH: `${sandbox}/bin:${process.env.PATH}`, JET: join(sandbox, 'bin/jet'), FIXTURE_MODE: mode, JET_VERIFY_TMPDIR: external, JET_CI_EVIDENCE_DIR: evidence, JET_TEST_SHARD: '0', JET_TEST_SHARD_COUNT: '1' }, encoding: 'utf8', timeout: 300000 });
        assert.equal(ran.error, undefined); assert.equal(ran.signal, null);
        assert.equal(ran.status, ['pass', 'pinned'].includes(mode) ? 0 : 1, ran.stderr);
        const report = JSON.parse(readFileSync(join(evidence, 'verify-full.json'), 'utf8'));
        const rows = Object.fromEntries(report.rows.map(row => [row.scope, row]));
        if (mode === 'ordinary') { assert.equal(rows.golden.code, 17); for (const suite of ['ui', 'cases', 'registries']) assert.equal(rows[suite].status, 'PASS'); }
        if (mode === 'refused') { assert.equal(rows.ui.status, 'FAIL'); assert.match(rows.ui.reason, /non-PASS/); assert.equal(rows.cases.status, 'PASS'); }
        if (mode === 'signal') { assert.equal(rows.golden.status, 'FAIL'); for (const suite of ['ui', 'cases', 'registries']) assert.equal(rows[suite].status, 'BLOCKED'); }
        if (mode === 'registry') assert.equal(rows.registries.status, 'FAIL');
        if (mode === 'corrupt') assert.equal(rows['source-integrity-after'].status, 'FAIL');
        if (mode === 'pinned') { assert.equal(rows['source-integrity-before'].status, 'PASS'); assert.equal(rows['source-export'], undefined); }
        if (mode === 'closeout') assert.equal(report.rows.length, 1);
        if (mode === 'pass') assert.ok(report.rows.every(row => row.status === 'PASS'));
        if (process.env.JET_COLLECT_REGRESSION_OUTPUT) cpSync(evidence, join(process.env.JET_COLLECT_REGRESSION_OUTPUT, mode), { recursive: true });
        outcomes.push({ mode, code: ran.status });
    }
    console.log(JSON.stringify({ scope: 'native runner control flow only', outcomes }));
    console.log(`CHECK OK: native verify-full collect-all (${outcomes.length} scenarios)`);
} finally { rmSync(container, { recursive: true, force: true }); rmSync(external, { recursive: true, force: true }); }
