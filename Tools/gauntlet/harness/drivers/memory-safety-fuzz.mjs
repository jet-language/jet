// The gauntlet driver of Tools/gauntlet/harness/memory-safety-fuzz.mjs
// (#5072 c8): it runs the axis adapter on the axis in AXIS and prints the
// report. Tools/gauntlet/entries/memory-safety-fuzz/driver/run.jet is the
// twin on the kit's MemorySafetyFuzz module.
//
//   node memory-safety-fuzz.mjs AXIS JET
//
// The adapter runs in the working directory: the run directory is `run` and
// the repository the working directory itself. A runner's declared files are
// copied from the repository, a tool is available when a file of its name is
// there (and fails its probe when `<tool>.broken` is), a declared command has
// JET in place of `{jet_bin}`, and a command runs as it is declared, in the
// runner's directory. An adapter error prints as `error: <message>`.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { runMemorySafetyFuzzAxis } from '../memory-safety-fuzz.mjs';

const [file, jet] = process.argv.slice(2);
const axis = JSON.parse(fs.readFileSync(file, 'utf8'));
const repoDir = process.cwd();

const context = {
  runDir: path.resolve('run'),
  repoDir,
  jetBin: jet,
  stageAxisFiles(dir, declared) {
    const staged = [];
    for (const item of declared ?? []) {
      const source = path.resolve(repoDir, item.source);
      if (!fs.existsSync(source)) throw new Error(`axis file is missing: ${item.source}`);
      const target = path.resolve(dir, item.target);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.copyFileSync(source, target);
      staged.push({ source: item.source, target: item.target });
    }
    return staged;
  },
  axisStagePath: (dir, relative) => path.resolve(dir, relative),
  probeAxisTools(dir, tools) {
    return (tools ?? []).map((tool) => {
      if (fs.existsSync(path.join(repoDir, tool))) return { tool, status: 'available' };
      if (fs.existsSync(path.join(repoDir, `${tool}.broken`))) return { tool, status: 'probe_failed', reason: `${tool} probe failed` };
      return { tool, status: 'unavailable', reason: `${tool} is not installed` };
    });
  },
  axisCommand: (declared, values) => declared.map((part) => String(part).replaceAll('{jet_bin}', values.jet_bin)),
  runMemoryCommand(dir, command) {
    const done = spawnSync(command[0], command.slice(1), { cwd: dir, encoding: 'utf8' });
    return { command, process: { code: done.status, signal: done.signal, timed_out: false, resource_exceeded: null, stdout: done.stdout, stderr: done.stderr } };
  },
  fileSha256: (target) => createHash('sha256').update(fs.readFileSync(target)).digest('hex'),
};

try {
  console.log(JSON.stringify(await runMemorySafetyFuzzAxis(axis, context), null, 2));
} catch (error) {
  console.log(`error: ${error.message}`);
  process.exit(1);
}
