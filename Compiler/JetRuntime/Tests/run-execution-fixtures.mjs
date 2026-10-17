#!/usr/bin/env node
// BIN must be the C witness linked to the fresh UNIT1 + Host-tail runtime pack.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
const [binary] = process.argv.slice(2);
if (!binary || process.argv.length !== 3) throw Error('usage: run-execution-fixtures.mjs BIN');
const result = spawnSync(resolve(binary), [], { encoding: 'utf8', timeout: 30000 });
if (result.error) throw result.error;
assert.equal(result.signal, null);
assert.equal(result.status, 0, result.stderr);
assert.equal(result.stderr, '');
assert.equal(result.stdout, 'CHECK OK — native execution: budget byte equality, modes, MAX sites, stop goldens, nonconsuming, stream flush\n');
console.log('native execution witness: all cases passed');
