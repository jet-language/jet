import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

// A standalone bounded model, not Jet compiler or runtime evidence.
// The reference recomputes from rows. The candidate updates group state.
const regions = ['North', 'South'];
const values = [null, ...regions.flatMap(region => [-1, 0, 1].map(amount => ({ region, amount })))];
const rows = new Map();
let stateCount = 0;
let transitionCount = 0;
const mutantWitnesses = {};

function reference(input) {
  const result = new Map();
  for (const row of input.values()) {
    result.set(row.region, (result.get(row.region) ?? 0n) + BigInt(row.amount));
  }
  return [...result].sort(([a], [b]) => a.localeCompare(b));
}

function maintained(input) {
  const state = new Map();
  for (const row of input.values()) {
    const group = state.get(row.region) ?? { count: 0, total: 0n };
    group.count++;
    group.total += BigInt(row.amount);
    state.set(row.region, group);
  }
  return state;
}

function update(state, oldRow, newRow, mutant) {
  const result = new Map([...state].map(([key, value]) => [key, { ...value }]));
  if (oldRow && !(mutant === 'skip_old_group' && newRow && oldRow.region !== newRow.region)) {
    const group = result.get(oldRow.region);
    group.count--;
    group.total -= BigInt(oldRow.amount);
    if (group.count === 0 && mutant !== 'retain_empty_group') result.delete(oldRow.region);
  }
  if (newRow) {
    const group = result.get(newRow.region) ?? { count: 0, total: 0n };
    group.count++;
    group.total += BigInt(newRow.amount);
    result.set(newRow.region, group);
  }
  return [...result].map(([key, value]) => [key, value.total]).sort(([a], [b]) => a.localeCompare(b));
}

const json = value => JSON.stringify(value, (_, item) => typeof item === 'bigint' ? item.toString() : item);

function checkState() {
  stateCount++;
  const state = maintained(rows);
  for (let id = 0; id < 3; id++) {
    const oldRow = rows.get(id);
    // Insert requires absence; replace/remove require presence. No upsert ambiguity.
    for (const newRow of values) {
      if (!oldRow && !newRow) continue;
      const next = new Map(rows);
      if (newRow) next.set(id, newRow);
      else next.delete(id);
      const expected = reference(next);
      assert.deepEqual(update(state, oldRow, newRow), expected);
      transitionCount++;
      for (const mutant of ['skip_old_group', 'retain_empty_group']) {
        const actual = update(state, oldRow, newRow, mutant);
        if (!mutantWitnesses[mutant] && json(actual) !== json(expected)) {
          mutantWitnesses[mutant] = {
            before: [...rows].map(([key, row]) => ({ id: key, ...row })),
            action: { kind: !oldRow ? 'insert' : newRow ? 'replace' : 'remove', id, row: newRow },
            expected,
            actual,
          };
        }
      }
    }
  }
}

function enumerate(id = 0) {
  if (id === 3) return checkState();
  for (const row of values) {
    if (row) rows.set(id, row);
    else rows.delete(id);
    enumerate(id + 1);
  }
  rows.delete(id);
}

enumerate();
assert.equal(stateCount, 343);
assert.equal(Object.keys(mutantWitnesses).length, 2);

// Exact integer algebra does not justify the same rewrite for IEEE binary64.
const oldFloatRows = [1e16, 1, -1e16];
const newFloatRows = [1e16, 2, -1e16];
const sum = xs => xs.reduce((a, b) => a + b, 0);
const floatCandidate = (sum(oldFloatRows) - 1) + 2;
const floatReference = sum(newFloatRows);
assert.notEqual(floatCandidate, floatReference);

const scriptPath = fileURLToPath(import.meta.url);
const result = {
  schema: 'jet.refounding.incremental-model/v1',
  status: 'bounded model passed; two mutants rejected; Float generalization refuted',
  runtime: process.version,
  script_sha256: createHash('sha256').update(readFileSync(scriptPath)).digest('hex'),
  domain: { ids: [0, 1, 2], regions, amounts: [-1, 0, 1], arithmetic: 'BigInt exact integers' },
  states: stateCount,
  valid_transitions: transitionCount,
  mutant_witnesses: mutantWitnesses,
  float_counterexample: { before: oldFloatRows, after: newFloatRows, candidate: floatCandidate, reference: floatReference },
  limits: [
    'No Jet parser, sema, MIR, backend, or runtime was executed.',
    'No performance measurement or unbounded correctness proof is claimed.',
    'No joins, source failures, output ordering law, concurrency, or publication semantics are modeled.',
    'Group-key sorting is a comparison normalization, not a proposed Jet ordering rule.',
  ],
};
const rendered = JSON.stringify(result, (_, item) => typeof item === 'bigint' ? item.toString() : item, 2) + '\n';
if (process.argv[2]) writeFileSync(process.argv[2], rendered);
process.stdout.write(rendered);
