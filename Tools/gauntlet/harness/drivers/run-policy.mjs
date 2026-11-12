// The gauntlet driver of the performance-law exports of
// Tools/gauntlet/harness/run.mjs (#5072 c8): one command per export.
// Tools/gauntlet/entries/run-policy/driver/run.jet is the twin on the kit's
// Policy module.
//
//   node run-policy.mjs policy | issues MATRIX | references MATRIX REFERENCES
//                      | peers MATRIX | applicability CASES | ratio VALUES
//
// MATRIX is a gauntlet matrix.json and REFERENCES an asm-parity cells.json,
// whose hand-written references are looked up beside the repository's own
// cells.json. CASES is a JSON array of {entry, language, metric} and VALUES
// a JSON array of Jet/peer ratios.
import fs from 'node:fs';
import {
  CEILING_POLICY,
  LOSS_OWNER_CATEGORY_BY_METRIC,
  STRICT_PERFORMANCE_POLICY,
  ceilingReferenceIssues,
  matrixPeerLanguages,
  metricApplicability,
  ratioVerdict,
  strictPerformancePolicyIssues,
} from '../run.mjs';

const usage = 'usage: node run-policy.mjs policy | issues MATRIX | references MATRIX REFERENCES | peers MATRIX | applicability CASES | ratio VALUES';
const [command, first, second] = process.argv.slice(2);
const read = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));
const list = (issues) => {
  for (const issue of issues) console.log(issue);
  console.log(`${issues.length} issues`);
};

if (command === 'policy') {
  console.log(JSON.stringify({ strict_performance_policy: STRICT_PERFORMANCE_POLICY, ceiling_policy: CEILING_POLICY, loss_owner_category_by_metric: LOSS_OWNER_CATEGORY_BY_METRIC }, null, 2));
} else if (command === 'issues') {
  list(strictPerformancePolicyIssues(read(first)));
} else if (command === 'references') {
  list(await ceilingReferenceIssues(read(first), read(second)));
} else if (command === 'peers') {
  const matrix = read(first);
  console.log(`*\t${matrixPeerLanguages(matrix).join(',')}`);
  for (const cell of matrix.cells ?? []) console.log(`${cell.id}\t${matrixPeerLanguages(matrix, cell).join(',')}`);
} else if (command === 'applicability') {
  for (const item of read(first)) console.log(`${item.language} ${item.metric}\t${JSON.stringify(metricApplicability(item.entry, item.language, item.metric))}`);
} else if (command === 'ratio') {
  for (const value of read(first)) console.log(`${JSON.stringify(value)}\t${ratioVerdict(value) ?? 'none'}`);
} else {
  console.error(usage);
  process.exit(64);
}
