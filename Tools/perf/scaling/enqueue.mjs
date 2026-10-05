#!/usr/bin/env node
// Counter correctness can share a machine; only the small timing sample needs quiet.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { options } from './harness.mjs';
import { SHAPES } from './corpora.mjs';

export function queuePlan(config) {
  const groupSize = Math.ceil(config.shapes.length / 3);
  const plan = [];
  for (let offset = 0; offset < config.shapes.length; offset += groupSize) {
    plan.push({ name: `ScalingGate-counters-${plan.length + 1}`, shared: true, config: { ...config,
      shapes: config.shapes.slice(offset, offset + groupSize), mode: 'counters', jobs: 2,
      timeoutS: Math.min(config.timeoutS, 25), memoryMax: '8G', profileTop: false,
      workDir: path.join(config.workDir, `counter-${plan.length + 1}`) } });
  }
  plan.push({ name: 'PerfScalingGate-timing', shared: false, config: { ...config,
    shapes: config.shapes.filter(shape => SHAPES[shape].kind === 'domain' || ['huge-fns', 'fan-in'].includes(shape)),
    mode: 'timing', jobs: 1, timeoutS: Math.min(config.timeoutS, 20), memoryMax: '24G',
    steps: 2, ladders: {}, domainLadder: [125000, 1000000], stopAfterS: Infinity, profileTop: false,
    workDir: path.join(config.workDir, 'timing') } });
  return plan.filter(job => job.config.shapes.length);
}

function gateArgs(config) {
  const args = config.compilers.flatMap(c => ['--compiler', `${c.label}=${c.kind}:${c.path ?? c.template}`]);
  for (const shape of config.shapes) args.push('--shape', shape);
  args.push('--steps', config.steps, '--domain-ladder', config.domainLadder.join(','),
    '--memory-max', config.memoryMax, '--timeout', config.timeoutS, '--stop-after', config.stopAfterS,
    '--seed', config.seed, '--jet-command', config.jetCommand, '--work-dir', config.workDir,
    '--build-root', config.buildRoot, '--mode', config.mode, '--jobs', config.jobs);
  for (const layout of config.layouts) args.push('--layout', layout);
  for (const shape of config.shapes) {
    if (config.bases[shape]) args.push('--base', `${shape}=${config.bases[shape]}`);
    if (config.ladders[shape]) args.push('--ladder', `${shape}=${config.ladders[shape].join(',')}`);
  }
  return args;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const config = options(process.argv.slice(2));
  if (!config) {
    console.log('Usage: JET_SCALING_QUEUE_STAMP=EXISTING_JOB node Tools/perf/scaling/enqueue.mjs --compiler LABEL=KIND:PATH [harness options]\nQueues three counter groups (2 parallel rungs, 25 s cap, 8G each) and one serial timing sample (20 s cap, 24G): huge-fns, fan-in and all domains at 125k/1M.');
  } else {
    const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
    const queue = path.join(os.homedir(), '.cache/jet-dev/proofq/queue');
    const stamp = fs.statSync(process.env.JET_SCALING_QUEUE_STAMP).mtime;
    const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;
    const plan = queuePlan(config);
    fs.mkdirSync(config.workDir, { recursive: true });
    fs.mkdirSync(queue, { recursive: true });
    fs.writeFileSync(path.join(config.workDir, 'queue-plan.json'), JSON.stringify(plan, null, 2));
    for (const job of plan) {
      const file = path.join(queue, `${job.name}.sh`);
      const pending = `${file}.hold`;
      const lock = path.join(os.homedir(), '.cache/jet-dev/scaling-gate.lock');
      const shell = `#!/usr/bin/env bash\n# proofq: mem=${job.shared ? 18 : 25}G time=50M kind=${job.shared ? 'test' : 'measure'}\n# #4601: ${job.config.mode}; rc=1 is gate evidence, not an invalid run.\nset -u\ncd ${quote(repo)} || exit 2\nTools/agent/jet-env bash "$HOME/.cache/jet-dev/disk-guard.sh" --check 30 ${quote(job.config.buildRoot)} || exit $?\nflock ${job.shared ? '--shared ' : ''}${quote(lock)} Tools/agent/jet-env node Tools/perf/scaling/run-gate.mjs ${gateArgs(job.config).map(quote).join(' ')}\nrc=$?\necho "gate rc=$rc (1 = failures recorded in receipt.json)"\n[ "$rc" -le 1 ]\n`;
      if (fs.existsSync(file)) throw new Error(`Job already exists: ${file}`);
      fs.writeFileSync(pending, shell, { flag: 'wx', mode: 0o755 });
      const when = new Date(stamp.getTime() + (job.shared ? 0 : 1000));
      fs.utimesSync(pending, when, when);
      fs.renameSync(pending, file);
      console.log(file);
    }
  }
}
