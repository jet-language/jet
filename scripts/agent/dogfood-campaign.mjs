#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, relative as relativePath, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const DEFAULT_MANIFEST = resolve(SCRIPT_ROOT, 'docs/audits/raw/2393-r1/manifest.json');
const DEFAULT_PROTOCOL = resolve(SCRIPT_ROOT, 'docs/proposals/dogfood-jet-experience-5-of-5.md');
const DEFAULT_LEDGER = DEFAULT_PROTOCOL;
const DEFAULT_RERUN_REPORT = resolve(SCRIPT_ROOT, 'docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md');

const PARTICIPANTS = Object.freeze(Array.from({ length: 10 }, (_, index) => `A${String(index + 1).padStart(2, '0')}`));
const TASKS = Object.freeze(['T1', 'T2', 'T3', 'T4']);
const ARMS = Object.freeze(['Jet', 'Rust']);
const CATEGORIES = Object.freeze([
  'reading',
  'writing',
  'reasoning',
  'creating',
  'modifying',
  'diagnostics',
  'tooling_docs',
]);
const EXPECTED_FINDINGS = Object.freeze(Array.from({ length: 53 }, (_, index) => `F${String(index + 1).padStart(2, '0')}`));
const MANIFEST_SCHEMA = /^2393-r\d+-manifest-v\d+$/u;
const CAMPAIGN_SCHEMA = 'jet.dogfood-campaign.v1';

export class UsageError extends Error {}
export class SetupError extends Error {}

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(String(left), 'utf8'), Buffer.from(String(right), 'utf8'));
}

function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.keys(value).sort(utf8Compare).map((key) => [key, canonical(value[key])]));
  }
  return value;
}

export function stableJson(value) {
  return JSON.stringify(canonical(value));
}

function prettyJson(value) {
  return `${JSON.stringify(canonical(value), null, 2)}\n`;
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function nonEmptyString(value) {
  return typeof value === 'string' && value.trim().length > 0;
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function displayPath(file) {
  const absolute = resolve(file);
  const rel = relativePath(SCRIPT_ROOT, absolute).replaceAll('\\', '/');
  return rel && !rel.startsWith('../') && rel !== '..' ? rel : absolute;
}

function resolveInput(value) {
  if (typeof value !== 'string' || !value.trim()) throw new UsageError('path must be non-empty');
  return resolve(process.cwd(), value);
}

function readBytes(file) {
  try {
    if (!statSync(file).isFile()) return null;
    return readFileSync(file);
  } catch {
    return null;
  }
}

function readText(file) {
  const bytes = readBytes(file);
  return bytes === null ? null : bytes.toString('utf8');
}

function readJson(file, label) {
  const bytes = readBytes(file);
  if (bytes === null) throw new SetupError(`${label} is missing: ${displayPath(file)}`);
  try {
    return { value: JSON.parse(bytes.toString('utf8')), bytes };
  } catch (error) {
    throw new SetupError(`${label} is not valid JSON: ${error.message}`);
  }
}

function pathFromManifest(value) {
  if (!nonEmptyString(value)) return null;
  return resolve(SCRIPT_ROOT, value);
}

function expectedKeys() {
  return PARTICIPANTS.flatMap((participant) => TASKS.flatMap((task) => ARMS.map((arm) => `${participant}/${task}/${arm}`)));
}

function entryKey(entry) {
  return `${entry?.participant ?? '?'}/${entry?.task ?? '?'}/${entry?.arm ?? '?'}`;
}

function compareEntries(left, right) {
  return utf8Compare(entryKey(left), entryKey(right));
}

function scoreValue(value) {
  return typeof value === 'number' && Number.isInteger(value) && value >= 1 && value <= 5;
}

export function median(values) {
  if (!Array.isArray(values) || values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

function parseArgs(argv = []) {
  let action = null;
  const options = {
    manifest: DEFAULT_MANIFEST,
    protocol: DEFAULT_PROTOCOL,
    ledger: DEFAULT_LEDGER,
    rerunReport: DEFAULT_RERUN_REPORT,
    closeoutReport: null,
    json: false,
  };
  const paths = new Map([
    ['--manifest', 'manifest'],
    ['--protocol', 'protocol'],
    ['--ledger', 'ledger'],
    ['--rerun-report', 'rerunReport'],
    ['--closeout-report', 'closeoutReport'],
  ]);

  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === '-h' || token === '--help') {
      if (action && action !== 'help') throw new UsageError('--help cannot be combined with an action');
      action = 'help';
      continue;
    }
    if (token === 'check' || token === 'score') {
      if (action && action !== token) throw new UsageError('only one action may be selected');
      action = 'check';
      continue;
    }
    if (token === '--json') {
      options.json = true;
      continue;
    }
    const key = paths.get(token);
    if (key) {
      const value = argv[++index];
      if (!value || value.startsWith('--')) throw new UsageError(`${token} needs a path`);
      options[key] = resolveInput(value);
      continue;
    }
    throw new UsageError(`unknown option: ${token}`);
  }

  if (!action) action = 'check';
  return { action, options };
}

export { parseArgs };

export function usage() {
  return `Usage: node scripts/agent/dogfood-campaign.mjs [check|score] [options]\n\nRead-only verifier for the earned-preference campaign. It never starts agents,\nexecutes Jet or Rust programs, edits receipts, or writes Tower state. A campaign\npasses only when every required receipt, score, preference, ledger row, rerun\nverdict, and hostile closeout verdict is present and satisfies the fixed bar.\n\nOptions:\n  --manifest PATH          Sealed receipt manifest\n  --protocol PATH          Protocol to validate\n  --ledger PATH            Finding disposition ledger\n  --rerun-report PATH      Dated rerun report (default: r1 report)\n  --closeout-report PATH   Hostile closeout report (required for PASS)\n  --json                   Emit deterministic JSON\n  -h, --help               Show this help\n\nExit status:\n  0  PASS\n  1  valid evidence but the campaign gate is not met\n  2  usage or setup error\n`;
}

function protocolCheck(file) {
  const text = readText(file);
  const required = [
    ['fixed pass bar', /fixed pass bar/iu],
    ['preregistered protocol', /preregistered protocol/iu],
    ['agent sourcing', /agent sourcing/iu],
    ['matched task contracts', /matched task contracts/iu],
    ['order and arm blinding', /order and arm blinding/iu],
    ['repair and stop rules', /repair and stop rules/iu],
    ['matched measurements', /matched measurements/iu],
    ['raw scorecard receipt', /raw scorecard receipt/iu],
    ['score rules', /score rules/iu],
  ];
  if (text === null) {
    return { status: 'not_measured', path: displayPath(file), sha256: null, missing: required.map(([name]) => name) };
  }
  const missing = required.filter(([, pattern]) => !pattern.test(text)).map(([name]) => name);
  const rules = {
    missing_data_is_not_pass: /not measured/iu.test(text) && /not.*(?:zero|pass)|missing.*fail/isu.test(text),
    non_jet_preference_fails: /Rust or no preference fails|no-preference.*fails|no preference.*fails/iu.test(text),
    both_arms_required: /both arms/iu.test(text),
  };
  return {
    status: missing.length === 0 && Object.values(rules).every(Boolean) ? 'recorded' : 'invalid',
    path: displayPath(file),
    sha256: sha256(Buffer.from(text, 'utf8')),
    missing,
    rules,
  };
}

function ledgerCheck(file) {
  const text = readText(file);
  if (text === null) {
    return {
      status: 'not_measured',
      path: displayPath(file),
      sha256: null,
      expected: EXPECTED_FINDINGS.length,
      covered: 0,
      missing: [...EXPECTED_FINDINGS],
      duplicates: [],
      empty: [],
      invalid: [],
      unknown: [],
    };
  }
  const rows = new Map();
  const duplicates = [];
  const empty = [];
  const invalid = [];
  const unknown = [];
  const dispositionPattern = /#\d+|D-[A-Z0-9-]+|\bdeclined?\b|\bexisting owner\b|\bstanding\b/iu;
  const pattern = /^\|\s*(F\d{2})\s*\|([^|]*)\|([^|]*)\|/gmu;
  for (const match of text.matchAll(pattern)) {
    const finding = match[1];
    const disposition = match[3].trim();
    if (!EXPECTED_FINDINGS.includes(finding)) unknown.push(finding);
    if (rows.has(finding)) duplicates.push(finding);
    else rows.set(finding, disposition);
    if (!disposition) empty.push(finding);
    else if (!dispositionPattern.test(disposition)) invalid.push(finding);
  }
  const missing = EXPECTED_FINDINGS.filter((finding) => !rows.has(finding));
  const covered = EXPECTED_FINDINGS.filter((finding) => rows.has(finding) && rows.get(finding)).length;
  return {
    status: missing.length === 0 && duplicates.length === 0 && empty.length === 0 && invalid.length === 0 && unknown.length === 0
      ? 'recorded' : 'invalid',
    path: displayPath(file),
    sha256: sha256(Buffer.from(text, 'utf8')),
    expected: EXPECTED_FINDINGS.length,
    covered,
    missing,
    duplicates: [...new Set(duplicates)].sort(utf8Compare),
    empty: [...new Set(empty)].sort(utf8Compare),
    invalid: [...new Set(invalid)].sort(utf8Compare),
    unknown: [...new Set(unknown)].sort(utf8Compare),
  };
}

function validateReceiptIdentity(receipt, entry, runId, receiptSchema, failures) {
  if (!isObject(receipt)) {
    failures.push(`receipt ${entryKey(entry)} is not an object`);
    return false;
  }
  if (receipt.schema_version !== receiptSchema) failures.push(`receipt ${entryKey(entry)} has schema ${String(receipt.schema_version)}`);
  if (receipt.card !== 2393) failures.push(`receipt ${entryKey(entry)} has card ${String(receipt.card)}`);
  if (receipt.run_id !== runId) failures.push(`receipt ${entryKey(entry)} has run ${String(receipt.run_id)}`);
  for (const field of ['participant', 'task', 'arm']) {
    if (receipt[field] !== entry[field]) failures.push(`receipt ${entryKey(entry)} disagrees on ${field}`);
  }
  if (receipt.status !== 'measured') failures.push(`receipt ${entryKey(entry)} is not marked measured`);
  if (!isObject(receipt.rating)) failures.push(`receipt ${entryKey(entry)} has no rating object`);
  else {
    for (const category of CATEGORIES) {
      if (!scoreValue(receipt.rating[category])) failures.push(`receipt ${entryKey(entry)} has invalid ${category} score`);
    }
    if (!nonEmptyString(receipt.rating.reason)) failures.push(`receipt ${entryKey(entry)} has no rating reason`);
  }
  if (!isObject(receipt.task_outcome) || typeof receipt.task_outcome.project_green !== 'boolean') {
    failures.push(`receipt ${entryKey(entry)} has no project-green outcome`);
  } else if (typeof entry.project_green !== 'boolean' || receipt.task_outcome.project_green !== entry.project_green) {
    failures.push(`receipt ${entryKey(entry)} project-green outcome disagrees with manifest`);
  }
  if (entry.complete !== true) failures.push(`measured receipt ${entryKey(entry)} is not marked complete`);
  if (typeof entry.project_green !== 'boolean') failures.push(`measured receipt ${entryKey(entry)} has no manifest project-green flag`);
  return true;
}

function validateIncompleteReceipt(receipt, entry, runId, receiptSchema, failures) {
  if (!isObject(receipt)) {
    failures.push(`receipt ${entryKey(entry)} is not an object`);
    return;
  }
  if (receipt.schema_version !== receiptSchema) failures.push(`receipt ${entryKey(entry)} has schema ${String(receipt.schema_version)}`);
  if (receipt.card !== 2393) failures.push(`receipt ${entryKey(entry)} has card ${String(receipt.card)}`);
  if (receipt.run_id !== runId) failures.push(`receipt ${entryKey(entry)} has run ${String(receipt.run_id)}`);
  for (const field of ['participant', 'task', 'arm']) {
    if (receipt[field] !== entry[field]) failures.push(`receipt ${entryKey(entry)} disagrees on ${field}`);
  }
  if (receipt.status !== entry.status) failures.push(`receipt ${entryKey(entry)} status disagrees with the manifest`);
  if (entry.complete !== false) failures.push(`incomplete receipt ${entryKey(entry)} is marked complete`);
  if (typeof entry.project_green !== 'boolean') failures.push(`incomplete receipt ${entryKey(entry)} has no manifest project-green flag`);
}


function loadManifest(file) {
  const { value, bytes } = readJson(file, 'manifest');
  if (!isObject(value)) throw new SetupError('manifest is not an object');
  const manifestSchema = String(value.schema_version ?? '');
  const familyMatch = manifestSchema.match(/^(2393-r\d+)-manifest-v\d+$/u);
  if (!MANIFEST_SCHEMA.test(manifestSchema) || !familyMatch) throw new SetupError('manifest schema is not a 2393-rN schema');
  if (value.run_id !== familyMatch[1]) throw new SetupError(`manifest run_id ${String(value.run_id)} disagrees with schema ${manifestSchema}`);
  if (value.card !== 2393) throw new SetupError(`manifest card is ${String(value.card)}, expected 2393`);
  if (!nonEmptyString(value.run_id)) throw new SetupError('manifest run_id is missing');
  if (!Array.isArray(value.entries)) throw new SetupError('manifest entries are missing');
  if (value.expected_receipt_count !== 80) throw new SetupError(`manifest expected_receipt_count is ${String(value.expected_receipt_count)}, expected 80`);
  if (value.entries.length !== value.expected_receipt_count) throw new SetupError('manifest entry count does not match expected_receipt_count');

  const expected = new Set(expectedKeys());
  const entries = [];
  const seen = new Set();
  for (const entry of value.entries) {
    const key = entryKey(entry);
    if (!PARTICIPANTS.includes(entry?.participant) || !TASKS.includes(entry?.task) || !ARMS.includes(entry?.arm)) {
      throw new SetupError(`manifest has an unknown receipt key ${key}`);
    }
    if (seen.has(key)) throw new SetupError(`manifest repeats receipt key ${key}`);
    seen.add(key);
    if (!nonEmptyString(entry.path)) throw new SetupError(`manifest receipt ${key} has no path`);
    if (!nonEmptyString(entry.status)) throw new SetupError(`manifest receipt ${key} has no status`);
    if (!/^[0-9a-f]{64}$/u.test(String(entry.sha256 ?? ''))) {
      throw new SetupError(`manifest receipt ${key} has no SHA-256 digest`);
    }
    entries.push(entry);
  }
  const missing = [...expected].filter((key) => !seen.has(key));
  if (missing.length) throw new SetupError(`manifest is missing receipt keys: ${missing.join(', ')}`);

  return {
    value,
    bytes,
    entries: entries.sort(compareEntries),
    path: file,
    receiptSchema: `${familyMatch[1]}-receipt-v1`,
    comparisonSchema: `${familyMatch[1]}-comparison-v1`,
  };
}

function loadReceiptEntries(manifest, failures) {
  const trusted = [];
  const missing = [];
  for (const entry of manifest.entries) {
    const file = pathFromManifest(entry.path);
    const bytes = file ? readBytes(file) : null;
    if (entry.status !== 'measured') {
      missing.push({ key: entryKey(entry), status: entry.status });
      if (bytes === null) continue;
      if (!/^[0-9a-f]{64}$/u.test(String(entry.sha256 ?? '')) || sha256(bytes) !== entry.sha256) {
        failures.push(`receipt digest mismatch for ${entryKey(entry)}`);
        continue;
      }
      let receipt;
      try {
        receipt = JSON.parse(bytes.toString('utf8'));
      } catch (error) {
        failures.push(`receipt ${entryKey(entry)} is invalid JSON: ${error.message}`);
        continue;
      }
      validateIncompleteReceipt(receipt, entry, manifest.value.run_id, manifest.receiptSchema, failures);
      continue;
    }
    if (bytes === null) {
      failures.push(`measured receipt ${entryKey(entry)} is missing`);
      missing.push({ key: entryKey(entry), status: 'missing' });
      continue;
    }
    const digest = sha256(bytes);
    if (digest !== entry.sha256) {
      failures.push(`receipt digest mismatch for ${entryKey(entry)}`);
      missing.push({ key: entryKey(entry), status: 'digest_mismatch' });
      continue;
    }
    let receipt;
    try {
      receipt = JSON.parse(bytes.toString('utf8'));
    } catch (error) {
      failures.push(`receipt ${entryKey(entry)} is invalid JSON: ${error.message}`);
      missing.push({ key: entryKey(entry), status: 'invalid_json' });
      continue;
    }
    const before = failures.length;
    validateReceiptIdentity(receipt, entry, manifest.value.run_id, manifest.receiptSchema, failures);
    if (failures.length !== before) {
      missing.push({ key: entryKey(entry), status: 'invalid' });
      continue;
    }
    trusted.push({ entry, receipt });
  }
  return { trusted, missing: missing.sort((left, right) => utf8Compare(left.key, right.key)) };
}

function validateComparison(value, entry, manifest, failures) {
  if (!isObject(value)) {
    failures.push(`comparison ${entry.participant} is not an object`);
    return null;
  }
  if (value.schema_version !== manifest.comparisonSchema) failures.push(`comparison ${entry.participant} has an unknown schema`);
  if (value.card !== 2393 || value.run_id !== manifest.value.run_id || value.participant !== entry.participant) {
    failures.push(`comparison ${entry.participant} identity does not match the manifest`);
  }
  if (value.status !== entry.status) failures.push(`comparison ${entry.participant} status disagrees with the manifest`);
  if (value.status !== 'measured') return null;
  if (!isObject(value.mapping) || !nonEmptyString(value.mapping.jet_label) || !nonEmptyString(value.mapping.rust_label)
    || value.mapping.jet_label === value.mapping.rust_label) {
    failures.push(`comparison ${entry.participant} has no distinct blind arm mapping`);
    return null;
  }
  const choices = ['no preference', value.mapping.jet_label, value.mapping.rust_label];
  if (!choices.includes(value.preference)) failures.push(`comparison ${entry.participant} has an invalid preference answer`);
  const expected = value.preference === value.mapping.jet_label
    ? 'Jet'
    : value.preference === value.mapping.rust_label ? 'Rust' : 'no preference';
  if (value.mapped_choice !== expected) failures.push(`comparison ${entry.participant} has an incorrect language mapping`);
  if (!nonEmptyString(value.reason)) failures.push(`comparison ${entry.participant} has no preference reason`);
  return {
    participant: entry.participant,
    answer: expected,
    reason: value.reason,
  };
}

function loadComparisons(manifest, trustedByParticipant, failures) {
  const comparisonEntries = Array.isArray(manifest.value.comparison_receipts)
    ? [...manifest.value.comparison_receipts].sort((left, right) => utf8Compare(String(left?.participant), String(right?.participant)))
    : [];
  if (comparisonEntries.length !== PARTICIPANTS.length) failures.push(`manifest has ${comparisonEntries.length} comparison receipts, expected ${PARTICIPANTS.length}`);
  const seen = new Set();
  const rows = [];
  const missing = [];
  for (const entry of comparisonEntries) {
    const participant = entry?.participant;
    if (!PARTICIPANTS.includes(participant)) {
      failures.push(`comparison manifest has unknown participant ${String(participant)}`);
      continue;
    }
    if (seen.has(participant)) {
      failures.push(`comparison manifest repeats ${participant}`);
      continue;
    }
    seen.add(participant);
    const file = pathFromManifest(entry.path);
    const bytes = file ? readBytes(file) : null;
    if (bytes === null) {
      failures.push(`comparison ${participant} is missing`);
      missing.push({ participant, status: 'missing' });
      continue;
    }
    if (!/^[0-9a-f]{64}$/u.test(String(entry.sha256 ?? '')) || sha256(bytes) !== entry.sha256) {
      failures.push(`comparison digest mismatch for ${participant}`);
      missing.push({ participant, status: 'digest_mismatch' });
      continue;
    }
    let value;
    try {
      value = JSON.parse(bytes.toString('utf8'));
    } catch (error) {
      failures.push(`comparison ${participant} is invalid JSON: ${error.message}`);
      missing.push({ participant, status: 'invalid_json' });
      continue;
    }
    const before = failures.length;
    const row = validateComparison(value, entry, manifest, failures);
    if (failures.length !== before) {
      missing.push({ participant, status: 'invalid' });
      continue;
    }
    if (!row) {
      missing.push({ participant, status: value.status || entry.status || 'not_measured' });
      continue;
    }
    const participantReceipts = trustedByParticipant.get(participant) || [];
    if (participantReceipts.length !== TASKS.length * ARMS.length) {
      failures.push(`comparison ${participant} is present without all eight measured arm-task receipts`);
      missing.push({ participant, status: 'arms_incomplete' });
      continue;
    }
    rows.push(row);
  }
  for (const participant of PARTICIPANTS) {
    if (!seen.has(participant)) missing.push({ participant, status: 'not_listed' });
  }
  const counts = { Jet: 0, Rust: 0, 'no preference': 0 };
  for (const row of rows) counts[row.answer] += 1;
  return {
    expected: PARTICIPANTS.length,
    measured: rows.length,
    rows: rows.sort((left, right) => utf8Compare(left.participant, right.participant)),
    missing: missing.sort((left, right) => utf8Compare(left.participant, right.participant)),
    counts,
  };
}

function scoreSummaries(trusted) {
  const byParticipantArm = new Map();
  for (const participant of PARTICIPANTS) {
    for (const arm of ARMS) byParticipantArm.set(`${participant}/${arm}`, []);
  }
  for (const { entry, receipt } of trusted) byParticipantArm.get(`${entry.participant}/${entry.arm}`).push({ entry, receipt });

  const arms = {};
  for (const arm of ARMS) {
    const participants = [];
    for (const participant of PARTICIPANTS) {
      const rows = (byParticipantArm.get(`${participant}/${arm}`) || []).sort((left, right) => utf8Compare(left.entry.task, right.entry.task));
      const medians = Object.fromEntries(CATEGORIES.map((category) => {
        const values = rows.map(({ receipt }) => receipt.rating?.[category]).filter(scoreValue);
        return [category, values.length === TASKS.length ? median(values) : null];
      }));
      const rawScores = rows.flatMap(({ receipt }) => CATEGORIES.map((category) => receipt.rating?.[category]).filter(scoreValue));
      const projectGreenTasks = rows.filter(({ receipt }) => receipt.task_outcome?.project_green === true).length;
      participants.push({
        participant,
        task_count: rows.length,
        project_green_tasks: projectGreenTasks,
        expected_tasks: TASKS.length,
        raw_rating_min: rawScores.length ? Math.min(...rawScores) : null,
        medians,
      });
    }
    const campaignMedians = Object.fromEntries(CATEGORIES.map((category) => {
      const values = participants.map((participant) => participant.medians[category]).filter((value) => value !== null);
      return [category, values.length === PARTICIPANTS.length ? median(values) : null];
    }));
    arms[arm] = {
      participants,
      campaign_medians: campaignMedians,
      measured_participants: participants.filter((participant) => participant.task_count === TASKS.length).length,
      project_green_tasks: participants.reduce((total, participant) => total + participant.project_green_tasks, 0),
      expected_project_green_tasks: PARTICIPANTS.length * TASKS.length,
    };
  }
  return { categories: [...CATEGORIES], arms };
}

function scoreFailures(scores, comparisons, receiptMissing, protocol, ledger, artifacts) {
  const failures = [];
  const jet = scores.arms.Jet;
  const rust = scores.arms.Rust;
  if (receiptMissing.length) failures.push(`missing or untrusted arm-task receipts: ${receiptMissing.length}`);
  for (const arm of ARMS) {
    const summary = scores.arms[arm];
    if (summary.project_green_tasks !== summary.expected_project_green_tasks) {
      failures.push(`${arm} project-green tasks ${summary.project_green_tasks}/${summary.expected_project_green_tasks}`);
    }
  }
  for (const participant of jet.participants) {
    if (participant.task_count !== TASKS.length) failures.push(`${participant.participant} Jet has ${participant.task_count}/${TASKS.length} measured tasks`);
    if (participant.raw_rating_min !== null && participant.raw_rating_min < 4) failures.push(`${participant.participant} Jet raw rating below 4`);
    for (const category of CATEGORIES) {
      if (participant.medians[category] !== 5) failures.push(`${participant.participant} Jet ${category} median is ${participant.medians[category] ?? 'not measured'}`);
    }
  }
  for (const category of CATEGORIES) {
    if (jet.campaign_medians[category] !== 5) failures.push(`Jet campaign ${category} median is ${jet.campaign_medians[category] ?? 'not measured'}`);
  }
  if (comparisons.measured !== comparisons.expected) failures.push(`blind preference measured ${comparisons.measured}/${comparisons.expected}`);
  if (comparisons.counts.Jet !== comparisons.expected) failures.push(`blind preference chose Jet ${comparisons.counts.Jet}/${comparisons.expected}`);
  if (protocol.status !== 'recorded') failures.push(`protocol is ${protocol.status}`);
  if (ledger.status !== 'recorded') failures.push(`ledger is ${ledger.status} (${ledger.covered}/${ledger.expected} findings covered)`);
  if (artifacts.rerun.status !== 'passed') failures.push(`rerun verdict is ${artifacts.rerun.status}`);
  if (artifacts.closeout.status !== 'passed') failures.push(`hostile closeout verdict is ${artifacts.closeout.status}`);
  if (rust.measured_participants !== PARTICIPANTS.length) failures.push(`Rust has ${rust.measured_participants}/${PARTICIPANTS.length} complete participants`);
  return [...new Set(failures)].sort(utf8Compare);
}

function artifactCheck(file, kind) {
  if (!file) return { status: 'not_measured', path: null, sha256: null };
  const text = readText(file);
  if (text === null) return { status: 'not_measured', path: displayPath(file), sha256: null };
  const digest = sha256(Buffer.from(text, 'utf8'));
  const verdict = /^\s*(?:\*{0,2})?(?:status|verdict|campaign verdict|closeout verdict)(?:\*{0,2})?\s*:\s*(?:\*{0,2})?\s*pass(?:ed)?\b/imu.test(text);
  const failed = /fixed 5\/5 bar is not met|gate not met|\bFAIL(?:ED)?\b/iu.test(text);
  if (kind === 'rerun') {
    return { status: verdict && !failed ? 'passed' : 'failed', path: displayPath(file), sha256: digest };
  }
  const zeroUncarded = /(?:zero|0)\s+uncarded findings|uncarded findings\s*:\s*0/iu.test(text);
  return { status: verdict && zeroUncarded && !failed ? 'passed' : 'failed', path: displayPath(file), sha256: digest };
}

export function inspectCampaign(input = {}) {
  const manifestPath = input.manifest ? resolve(input.manifest) : DEFAULT_MANIFEST;
  const protocolPath = input.protocol ? resolve(input.protocol) : DEFAULT_PROTOCOL;
  const ledgerPath = input.ledger ? resolve(input.ledger) : DEFAULT_LEDGER;
  const rerunReport = input.rerunReport === null ? null : (input.rerunReport ? resolve(input.rerunReport) : DEFAULT_RERUN_REPORT);
  const closeoutReport = input.closeoutReport ? resolve(input.closeoutReport) : null;
  const manifest = loadManifest(manifestPath);
  const failures = [];
  const protocol = protocolCheck(protocolPath);
  const ledger = ledgerCheck(ledgerPath);
  const loaded = loadReceiptEntries(manifest, failures);
  const trustedByParticipant = new Map(PARTICIPANTS.map((participant) => [participant, []]));
  for (const row of loaded.trusted) trustedByParticipant.get(row.entry.participant).push(row);
  const comparisons = loadComparisons(manifest, trustedByParticipant, failures);
  const scores = scoreSummaries(loaded.trusted);
  const artifacts = {
    rerun: artifactCheck(rerunReport, 'rerun'),
    closeout: artifactCheck(closeoutReport, 'closeout'),
  };
  failures.push(...scoreFailures(scores, comparisons, loaded.missing, protocol, ledger, artifacts));
  const uniqueFailures = [...new Set(failures)].sort(utf8Compare);
  return {
    schema: CAMPAIGN_SCHEMA,
    status: uniqueFailures.length === 0 ? 'PASS' : 'FAIL',
    run_id: manifest.value.run_id,
    protocol,
    manifest: {
      path: displayPath(manifestPath),
      sha256: sha256(manifest.bytes),
      expected_receipts: manifest.value.expected_receipt_count,
      measured_receipts: loaded.trusted.length,
      explicit_not_measured_receipts: loaded.missing.length,
    },
    ledger,
    receipts: {
      missing_or_untrusted: loaded.missing,
    },
    scores,
    preference: comparisons,
    artifacts,
    failures: uniqueFailures,
  };
}

function humanReport(report) {
  const lines = [
    `DOGFOOD CAMPAIGN ${report.status}`,
    `run: ${report.run_id}`,
    `protocol: ${report.protocol.status} ${report.protocol.path || 'not supplied'}`,
    `receipts: ${report.manifest.measured_receipts}/${report.manifest.expected_receipts} measured; ${report.manifest.explicit_not_measured_receipts} explicit not_measured`,
    `ledger: ${report.ledger.status} ${report.ledger.covered}/${report.ledger.expected} findings covered`,
  ];
  for (const arm of ARMS) {
    const summary = report.scores.arms[arm];
    const medians = CATEGORIES.map((category) => `${category}=${summary.campaign_medians[category] ?? 'not measured'}`).join(' ');
    lines.push(`${arm}: project_green=${summary.project_green_tasks}/${summary.expected_project_green_tasks}; ${medians}`);
  }
  lines.push(`preference: ${report.preference.measured}/${report.preference.expected} measured; Jet=${report.preference.counts.Jet} Rust=${report.preference.counts.Rust} no_preference=${report.preference.counts['no preference']}`);
  lines.push(`rerun: ${report.artifacts.rerun.status}; closeout: ${report.artifacts.closeout.status}`);
  if (report.failures.length) {
    lines.push('failures:');
    for (const failure of report.failures) lines.push(`- ${failure}`);
  }
  return `${lines.join('\n')}\n`;
}

export async function main(argv = process.argv.slice(2)) {
  try {
    const { action, options } = parseArgs(argv);
    if (action === 'help') {
      process.stdout.write(usage());
      return 0;
    }
    const report = inspectCampaign(options);
    process.stdout.write(options.json ? prettyJson(report) : humanReport(report));
    return report.status === 'PASS' ? 0 : 1;
  } catch (error) {
    const label = error instanceof UsageError ? 'USAGE' : 'SETUP';
    process.stderr.write(`${label}: ${error.message}\n`);
    if (error instanceof UsageError) process.stderr.write(usage());
    return error instanceof UsageError ? 2 : 2;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) process.exitCode = await main();
