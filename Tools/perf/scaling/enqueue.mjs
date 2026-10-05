#!/usr/bin/env node
// Split a gate invocation into bounded, sequential proofq jobs, one per shape.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { options } from './harness.mjs';
import { SHAPES } from './corpora.mjs';

const config = options(process.argv.slice(2));
if (!config) {
  console.log('Usage: JET_SCALING_QUEUE_STAMP=EXISTING_JOB node Tools/perf/scaling/enqueue.mjs --compiler LABEL=KIND:PATH [harness options]\nUse --timeout 120 --stop-after 60 --profile to bound each shape below one hour.');
} else {
  const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
  const queue = path.join(os.homedir(), '.cache/jet-dev/proofq/queue');
  const stamp = fs.statSync(process.env.JET_SCALING_QUEUE_STAMP).mtime;
  const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;
  fs.mkdirSync(queue, { recursive: true });
  for (const shape of config.shapes) {
    const name = `PerfScalingGate-${SHAPES[shape].kind}-${shape}`;
    const file = path.join(queue, `${name}.sh`);
    const pending = `${file}.hold`;
    const compilers = config.compilers.flatMap(c => ['--compiler', `${c.label}=${c.kind}:${c.path ?? c.template}`]);
    const args = [...compilers, '--shape', shape, '--steps', config.steps, '--domain-ladder', config.domainLadder.join(','),
      '--memory-max', config.memoryMax, '--timeout', config.timeoutS, '--stop-after', config.stopAfterS,
      '--seed', config.seed, '--jet-command', config.jetCommand, '--work-dir', config.workDir, '--build-root', config.buildRoot];
    for (const layout of config.layouts) args.push('--layout', layout);
    if (config.bases[shape]) args.push('--base', `${shape}=${config.bases[shape]}`);
    if (config.ladders[shape]) args.push('--ladder', `${shape}=${config.ladders[shape].join(',')}`);
    if (config.profileTop) args.push('--profile');
    const shell = `#!/usr/bin/env bash\n# #4601: one shape, resumable; rc=1 is recorded gate evidence, not an invalid run.\nset -u\ncd ${quote(repo)} || exit 2\nflock ${quote(path.join(os.homedir(), '.cache/jet-dev/scaling-gate.lock'))} Tools/agent/jet-env node Tools/perf/scaling/run-gate.mjs ${args.map(quote).join(' ')}\nrc=$?\necho "gate rc=$rc (1 = failures recorded in receipt.json)"\n[ "$rc" -le 1 ]\n`;
    if (fs.existsSync(file)) throw new Error(`Job already exists: ${file}`);
    fs.writeFileSync(pending, shell, { flag: 'wx', mode: 0o755 });
    fs.utimesSync(pending, stamp, stamp);
    fs.renameSync(pending, file);
    console.log(file);
  }
}
