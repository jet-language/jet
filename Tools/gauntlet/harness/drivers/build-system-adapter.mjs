// The gauntlet driver of the contract and adapter exports of
// Tools/gauntlet/harness/build-system.mjs (#5072 c8): one command per export.
// Tools/gauntlet/entries/build-system-adapter/driver/run.jet is the twin on
// the kit's BuildSystem module.
//
//   node build-system-adapter.mjs contract AXIS | adapt AXIS JET
//
// AXIS is a JSON file holding an axes.build_system cell (`null` for a missing
// one). `adapt` runs the adapter in the working directory: the run directory
// is `run` and the repository the working directory itself.
import fs from 'node:fs';
import { buildSystemContractIssues, runBuildSystemAxisAdapter } from '../build-system.mjs';

const [command, file, jet] = process.argv.slice(2);
const axis = JSON.parse(fs.readFileSync(file, 'utf8'));
if (command === 'contract') {
  const issues = buildSystemContractIssues(axis);
  for (const issue of issues) console.log(issue);
  console.log(`${issues.length} issues`);
} else if (command === 'adapt') {
  const report = await runBuildSystemAxisAdapter(axis, { runDir: 'run', jetBin: jet, repoDir: process.cwd() });
  console.log(JSON.stringify(report, null, 2));
} else {
  console.error('usage: node build-system-adapter.mjs contract AXIS | adapt AXIS JET');
  process.exit(64);
}
