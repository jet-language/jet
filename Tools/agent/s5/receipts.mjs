import fs from 'node:fs';
import path from 'node:path';

export const phaseSchema = 'jet-s5-phase/v1';
export function atomicWrite(file, content) {
  const temporary = `${file}.tmp-${process.pid}`;
  fs.writeFileSync(temporary, content); fs.renameSync(temporary, file);
}
export function writePhaseReceipt(directory, receipt) {
  fs.mkdirSync(directory, {recursive: true});
  atomicWrite(path.join(directory, 'steps.json'), JSON.stringify(receipt, null, 2) + '\n');
  if (receipt.complete) atomicWrite(path.join(directory, 'verdict.txt'), `${receipt.verdict} ${receipt.detail ?? 'phase completed'}\n`);
}
export function readPrerequisite(directory, head, requiredSteps = [], expectedPhase = null) {
  try {
    if (!directory) throw Error('prerequisite directory absent');
    if (fs.readFileSync(path.join(directory, 'driver.rc'), 'utf8').trim() !== '0') throw Error('prerequisite real driver did not pass');
    const receipt = JSON.parse(fs.readFileSync(path.join(directory, 'steps.json'), 'utf8'));
    if (receipt.schema !== phaseSchema || receipt.head !== head || receipt.complete !== true || receipt.verdict !== 'PASS') throw Error('prerequisite is not a completed same-pin PASS');
    if (expectedPhase && receipt.phase !== expectedPhase) throw Error('prerequisite phase differs');
    if (!Array.isArray(receipt.steps) || !Array.isArray(receipt.runners) || [...receipt.steps, ...receipt.runners].some(row => !['PASS', 'UNCOVERED'].includes(row.verdict))) throw Error('prerequisite contains failed, blocked or malformed rows');
    for (const name of requiredSteps) if (receipt.steps.filter(row => row.name === name && row.verdict === 'PASS').length !== 1) throw Error(`decisive prerequisite step absent or not PASS: ${name}`);
    if (!fs.readFileSync(path.join(directory, 'verdict.txt'), 'utf8').startsWith('PASS ')) throw Error('prerequisite verdict differs');
    return {receipt, problem: null};
  } catch (error) { return {receipt: null, problem: error.message}; }
}
export function blockedCompletion(directory, {head, phase, runId, because}) {
  const receipt = {schema: phaseSchema, head, phase, runId, complete: true, verdict: 'BLOCKED', detail: because, steps: [{name: 'preflight', verdict: 'BLOCKED', because}], runners: []};
  writePhaseReceipt(directory, receipt); atomicWrite(path.join(directory, 'driver.rc'), '2\n');
}
export function finalizeDriver(directory, {head, phase, runId, code}) {
  fs.mkdirSync(directory, {recursive: true});
  const file = path.join(directory, 'steps.json');
  let receipt = null;
  try { receipt = JSON.parse(fs.readFileSync(file, 'utf8')); } catch {}
  const current = receipt?.schema === phaseSchema && receipt.head === head && receipt.phase === phase && receipt.runId === runId;
  const expected = receipt?.verdict === 'PASS' ? 0 : receipt?.verdict === 'BLOCKED' ? 2 : 1;
  if (!current || receipt.complete !== true || code !== expected) {
    let partial = null;
    if (fs.existsSync(file)) { partial = path.join(directory, `steps.incomplete-${runId}.json`); fs.renameSync(file, partial); }
    receipt = {schema: phaseSchema, head, phase, runId, complete: true, verdict: 'FAIL', detail: `driver exited ${code} without its complete matching receipt`, steps: [...(current && Array.isArray(receipt.steps) ? receipt.steps : []), {name: 'driver', verdict: 'FAIL', code, because: 'driver did not publish its complete matching receipt', partial}], runners: current && Array.isArray(receipt.runners) ? receipt.runners : []};
    writePhaseReceipt(directory, receipt);
  }
  atomicWrite(path.join(directory, 'driver.rc'), `${code}\n`);
  return receipt;
}
