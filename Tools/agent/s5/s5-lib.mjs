// Shared collect-all recorder for the RuntimeLead12 S5 proof phases.
// Every step runs; a step whose input is missing is BLOCKED; the phase exits
// nonzero when any step failed or was blocked. Receipts: <D>/steps.json and
// <D>/verdict.txt (one line).
import fs from 'node:fs';
import path from 'node:path';
import {spawn, spawnSync} from 'node:child_process';
import {assertPinnedSource} from '../proof-source.mjs';
import {phaseSchema, readPrerequisite, writePhaseReceipt} from './receipts.mjs';

export const W = process.cwd();

export function phase(name) {
  const expect = process.env.S5_EXPECT_HEAD, D = process.env.S5_OUT;
  if (!expect || !D) throw Error('S5_EXPECT_HEAD and S5_OUT are required');
  const head = assertPinnedSource(W, expect).commit;
  if (fs.existsSync(D) && fs.readdirSync(D).length) throw Error(`refusing to reuse ${D}`);
  fs.mkdirSync(D, {recursive: true});
  const steps = [], runners = [];
  const runId = process.env.S5_RUN_ID;
  let complete = false, verdict = 'RUNNING', detail = '';
  const record = () => writePhaseReceipt(D, {schema: phaseSchema, W, head, phase: name, runId, complete, verdict, detail, steps, runners});
  const env = {...process.env, RUST_MIN_STACK: String(256 * 1024 * 1024)};
  const row = (name, args, result, start, out, err, ok) => {
    const r = {name, argv: args, code: result.status ?? result.code ?? null, signal: result.signal ?? null, error: result.error?.message ?? null, wall_ms: Date.now() - start, stdout: out, stderr: err};
    r.verdict = !result.error && ok(r.code, r) ? 'PASS' : 'FAIL';
    steps.push(r); record();
    console.log(JSON.stringify({name: r.name, verdict: r.verdict, code: r.code, wall_ms: r.wall_ms}));
    return r.verdict === 'PASS';
  };
  const files = name => [path.join(D, name + '.stdout'), path.join(D, name + '.stderr')];
  const recorder = {
    D, head, env, steps,
    run(name, args, extra = {}, ok = code => code === 0, cwd = W) {
      const [out, err] = files(name);
      const a = fs.openSync(out, 'w'), b = fs.openSync(err, 'w'), start = Date.now();
      const result = spawnSync(args[0], args.slice(1), {cwd, env: {...env, ...extra}, stdio: ['ignore', a, b]});
      fs.closeSync(a); fs.closeSync(b);
      return row(name, args, result, start, out, err, ok);
    },
    // Concurrent steps: resolves to PASS/FAIL like run().
    start(name, args, extra = {}, ok = code => code === 0, cwd = W) {
      const [out, err] = files(name);
      const a = fs.openSync(out, 'w'), b = fs.openSync(err, 'w'), begin = Date.now();
      return new Promise(resolve => {
        const child = spawn(args[0], args.slice(1), {cwd, env: {...env, ...extra}, stdio: ['ignore', a, b]});
        let error = null;
        child.on('error', e => { error = e; });
        child.on('close', (code, signal) => {
          fs.closeSync(a); fs.closeSync(b);
          resolve(row(name, args, {status: code, signal, error}, begin, out, err, ok));
        });
      });
    },
    blocked(name, because) {
      steps.push({name, verdict: 'BLOCKED', because}); record();
      console.log(JSON.stringify({name, verdict: 'BLOCKED', because}));
      return false;
    },
    prerequisite(name, directory, requiredSteps = [], expectedPhase = null) {
      const {receipt, problem} = readPrerequisite(directory, head, requiredSteps, expectedPhase);
      if (problem) return this.blocked(name, problem);
      this.receipt(name, 'PASS', {directory, phase: receipt.phase, head: receipt.head});
      return true;
    },
    receipt(name, verdict, evidence) {
      if (!['PASS', 'FAIL', 'BLOCKED', 'UNCOVERED'].includes(verdict)) throw Error(`invalid verdict ${verdict}`);
      steps.push({name, verdict, ...evidence}); record();
    },
    runner(name, nativeSteps, parkedModes = [], reference = null) {
      const actual = nativeSteps.map(name => steps.find(step => step.name === name));
      const verdict = !nativeSteps.length ? 'UNCOVERED' :
        actual.some(step => !step || step.verdict === 'BLOCKED') ? 'BLOCKED' :
        actual.some(step => step.verdict !== 'PASS') ? 'FAIL' : 'PASS';
      runners.push({name, verdict, nativeSteps, parkedModes, reference});
      record();
    },
    finish() {
      const failed = [...steps, ...runners].filter(s => !['PASS', 'UNCOVERED'].includes(s.verdict));
      verdict = failed.some(s => s.verdict === 'FAIL') ? 'FAIL' : failed.length ? 'BLOCKED' : 'PASS';
      detail = failed.length ? failed.map(s => `${s.name}:${s.verdict}`).join(' ') : 'all steps';
      complete = true; record();
      console.log(`${verdict} ${detail}`);
      process.exitCode = verdict === 'PASS' ? 0 : verdict === 'BLOCKED' ? 2 : 1;
    },
  };
  const prerequisites = {
    runtime: [['build', process.env.S5_BUILD, ['shared-build-artifacts', 'shared-native-cli']]],
    boundary: [['build', process.env.S5_BUILD, ['shared-build-artifacts', 'shared-native-cli']], ['gates', process.env.S5_GATES, []]],
    fixtures: [['build', process.env.S5_BUILD, ['shared-build-artifacts', 'shared-native-cli']], ['runtime', process.env.S5_RUNTIME, ['runtime-pack', 'pack-publication', 'pack-exports', 'abi-witness', 'pack-roots']]],
  };
  const results = (prerequisites[name] ?? []).map(([phase, directory, decisive]) => recorder.prerequisite(`${phase}-prerequisite`, directory, decisive, phase));
  recorder.ready = results.every(Boolean);
  return recorder;
}

export function tail(file, n = 2 * 1024 * 1024) {
  const fd = fs.openSync(file, 'r'), size = fs.fstatSync(fd).size, b = Buffer.alloc(Math.min(size, n));
  fs.readSync(fd, b, 0, b.length, size - b.length); fs.closeSync(fd);
  return b.toString();
}
