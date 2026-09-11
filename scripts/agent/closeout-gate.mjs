#!/usr/bin/env node

import { existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, resolve, join } from 'node:path';

import { fileURLToPath } from 'node:url';
import { readCandidateRecord } from './hardening-manifest.mjs';
const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '../..');
const TOWER = resolve(REPO, 'plugins/tower/tower.mjs');
const TOWER_STATE_PREFIX = 'plugins/tower/.tower/';

const run = (program, args) => execFileSync(program, args, {
  cwd: REPO,
  encoding: 'utf8',
  env: process.env,
  maxBuffer: 1 << 26,
}).trim();

export function nonTowerChanges(status) {
  return String(status || '')
    .split('\n')
    .filter(Boolean)
    .filter(line => {
      const path = line.slice(3).replace(/^"|"$/g, '');
      return !path.startsWith(TOWER_STATE_PREFIX);
    });
}

export function selectCloseout(milestones, head, requested) {
  const eligible = milestones.filter(m =>
    m.closeout?.sourceCommit === head
    && m.progress?.reviewReady === true
    && (m.status === 'review-ready' || m.status === 'met'));
  if (requested) {
    const milestone = milestones.find(m => m.id === requested);
    if (!milestone) throw new Error(`unknown milestone ${requested}`);
    if (!eligible.includes(milestone))
      throw new Error(`milestone ${requested} has no valid closeout token for source commit ${head}`);
    return milestone;
  }
  if (eligible.length === 1) return eligible[0];
  if (!eligible.length) throw new Error(`no review-ready milestone has a closeout token for source commit ${head}`);
  throw new Error(`multiple closeout tokens match ${head}; set JET_CLOSEOUT_MILESTONE to one of: ${eligible.map(m => m.id).join(', ')}`);
}
function candidateRecordPath() {
  const configured = process.env.JET_CANDIDATE_RECORD;
  if (configured) return resolve(REPO, configured);
  const candidates = [
    join(REPO, '.jet/candidate.json'),
    join(REPO, 'candidate.json'),
    join(REPO, '.cache/jet-hardening/v1/candidate.json'),
  ];
  return candidates.find(existsSync) || candidates[0];
}

function candidateAtHead(head) {
  const path = candidateRecordPath();
  if (!existsSync(path)) throw new Error(`candidate record is missing: ${path}`);
  try {
    return readCandidateRecord(path, { sourceCommit: head, requireQualified: true });
  } catch (error) {
    throw new Error(`candidate record blocked: ${error.message}`);
  }
}


function frozenHead() {
  const changed = nonTowerChanges(run('git', ['status', '--porcelain=v1', '--untracked-files=all']));
  if (changed.length) {
    throw new Error(`source tree is not frozen; commit or remove these changes before broad proof:\n${changed.slice(0, 20).join('\n')}`);
  }
  return run('git', ['rev-parse', 'HEAD']);
}

function milestones() {
  return JSON.parse(run(process.execPath, [TOWER, 'milestone', 'list', '--archived', '--json']));
}

function check(requested = process.env.JET_CLOSEOUT_MILESTONE) {
  const head = frozenHead();
  const candidate = candidateAtHead(head);
  const milestone = selectCloseout(milestones(), head, requested);
  console.log(`CLOSEOUT TOKEN OK ${milestone.id} ${head} candidate ${candidate.candidate_identity.id}`);
  return milestone;
}

function open(args) {
  const id = args[0];
  const byIndex = args.indexOf('--by');
  const by = byIndex >= 0 ? args[byIndex + 1] : '';
  if (!id || !by) throw new Error('usage: closeout-gate.mjs open MILESTONE --by AGENT');
  const head = frozenHead();
  run(process.execPath, [TOWER, 'milestone', 'closeout', id, '--commit', head, '--by', by]);
  return check(id);
}

function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === 'open') return open(args);
  if (command === 'check') return check(args[0]);
  throw new Error('usage: closeout-gate.mjs open MILESTONE --by AGENT | check [MILESTONE]');
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(`closeout gate: ${error.message}`);
    process.exitCode = 2;
  }
}
