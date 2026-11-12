// The gauntlet driver of Tools/gauntlet/harness/compile-throughput.mjs
// (#5072 c8): one command per export.
// Tools/gauntlet/entries/compile-throughput/driver/run.jet is the twin on the
// kit's CompileThroughput module.
//
//   node compile-throughput.mjs contract AXIS | adapt AXIS JET
//
// AXIS is a JSON file holding an axes.compile_throughput cell (`null` for a
// missing one). `adapt` runs the adapter in the working directory: the run
// directory is `run` and the repository the working directory itself.
import fs from 'node:fs';
import { compileThroughputContractIssues, runCompileThroughputAxisAdapter } from '../compile-throughput.mjs';

const [command, file, jet] = process.argv.slice(2);
const axis = JSON.parse(fs.readFileSync(file, 'utf8'));
if (command === 'contract') {
  const issues = compileThroughputContractIssues(axis);
  for (const issue of issues) console.log(issue);
  console.log(`${issues.length} issues`);
} else if (command === 'adapt') {
  const report = await runCompileThroughputAxisAdapter(axis, { runDir: 'run', jetBin: jet, repoDir: process.cwd() });
  console.log(JSON.stringify(report, null, 2));
} else {
  console.error('usage: node compile-throughput.mjs contract AXIS | adapt AXIS JET');
  process.exit(64);
}
