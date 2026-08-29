#!/usr/bin/env node

// Capture immutable Tower inputs from known commits, then ask the current
// Node implementation for the corresponding full projections. This script has
// no live-store fallback: every oracle call receives the staged fixture path.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, relative, resolve, sep, isAbsolute } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PARITY_DIR = dirname(fileURLToPath(import.meta.url));
const FIXTURES_ROOT = resolve(PARITY_DIR, 'fixtures');
const REPO_ROOT = resolve(PARITY_DIR, '../../../..');
const LIVE_ROOT = resolve(REPO_ROOT, 'plugins/tower/.tower');
const ORACLE_PATH = resolve(REPO_ROOT, 'plugins/tower/app/cli.mjs');
const MANIFEST_PATH = resolve(PARITY_DIR, 'snapshots.json');

const SOURCE_FILES = Object.freeze({
  'tower.json': 'plugins/tower/.tower/tower.json',
  'history.json': 'plugins/tower/.tower/history.json',
  'config.json': 'plugins/tower/.tower/config.json',
});

const OUTPUT_FILES = Object.freeze([
  'tower.json',
  'history.json',
  'config.json',
  'state.json',
  'lint.json',
  'capture-time.txt',
]);

const SNAPSHOTS = Object.freeze([
  Object.freeze({
    date: '2026-08-26',
    commit: '12564eb3c0c4e4cd715281c46e6d577db544411f',
  }),
  Object.freeze({
    date: '2026-08-27',
    commit: '7428b1e36fdcb64a1f522d4b062ceabcd2925106',
  }),
  Object.freeze({
    date: '2026-08-28',
    commit: '8b027acc19d9a9f3446bcd885d9392d4c635f06b',
  }),
]);

function fail(message) {
  throw new Error(message);
}

function assertExactKeys(value, expected, label) {
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== expected.length ||
    expected.some((key) => !Object.hasOwn(value, key))
  )
    fail(`${label} has unexpected keys`);
}

function isValidIsoInstant(value) {
  const match = typeof value === 'string' && /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(Z|[+-]\d{2}:\d{2})$/.exec(value);
  if (!match) return false;
  const [, yearText, monthText, dayText, hourText, minuteText, secondText, zone] = match;
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  const hour = Number(hourText);
  const minute = Number(minuteText);
  const second = Number(secondText);
  const daysInMonth = [31, 28 + (year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 1 : 0), 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (month < 1 || month > 12 || day < 1 || day > daysInMonth[month - 1]) return false;
  if (hour > 23 || minute > 59 || second > 59) return false;
  if (zone !== 'Z' && (Number(zone.slice(1, 3)) > 23 || Number(zone.slice(4)) > 59)) return false;
  return !Number.isNaN(Date.parse(value));
}

function assertCaptureMetadata(manifest) {
  if (!isValidIsoInstant(manifest.captured_at)) fail('manifest captured_at is not a valid ISO/RFC3339 instant');
  if (manifest.capture_day !== manifest.captured_at.slice(0, 10))
    fail('manifest capture_day must equal the first ten characters of captured_at');
}

function lstatIfPresent(path) {
  try {
    return lstatSync(path);
  } catch (error) {
    if (error?.code === 'ENOENT') return null;
    throw error;
  }
}

function isWithin(root, candidate) {
  const child = relative(root, candidate);
  return child === '' || (!child.startsWith(`..${sep}`) && child !== '..' && !isAbsolute(child));
}

function assertNoSymlinkChain(path, label) {
  let cursor = resolve(path);
  while (true) {
    const stat = lstatIfPresent(cursor);
    if (stat?.isSymbolicLink()) fail(`${label} is a symlink: ${cursor}`);
    const parent = dirname(cursor);
    if (parent === cursor) return;
    cursor = parent;
  }
}

function assertNoSymlinksTree(root, label) {
  const stat = lstatIfPresent(root);
  if (!stat) return;
  if (stat.isSymbolicLink()) fail(`${label} contains a symlink: ${root}`);
  if (!stat.isDirectory()) fail(`${label} is not a directory: ${root}`);
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const child = join(root, entry.name);
    const childStat = lstatIfPresent(child);
    if (!childStat) fail(`${label} changed while being inspected: ${child}`);
    if (childStat.isSymbolicLink()) fail(`${label} contains a symlink: ${child}`);
    if (childStat.isDirectory()) assertNoSymlinksTree(child, label);
    else if (childStat.isFile()) {
      if (childStat.nlink !== 1) fail(`${label} contains a hardlinked regular file: ${child}`);
    } else fail(`${label} contains a non-regular entry: ${child}`);
  }
}

function assertNotLivePath(path, label) {
  const candidate = resolve(path);
  if (isWithin(LIVE_ROOT, candidate) || isWithin(candidate, LIVE_ROOT))
    fail(`${label} overlaps the live canonical Tower input: ${candidate}`);
}

function assertFixturePath(path, label, { allowRoot = false } = {}) {
  const candidate = resolve(path);
  if (!isWithin(FIXTURES_ROOT, candidate) || (!allowRoot && candidate === FIXTURES_ROOT))
    fail(`${label} is outside the capture fixtures root: ${candidate}`);
  assertNotLivePath(candidate, label);
  assertNoSymlinkChain(candidate, label);
  return candidate;
}

function assertFixtureRoot() {
  assertFixturePath(FIXTURES_ROOT, 'fixtures root', { allowRoot: true });
  const stat = lstatIfPresent(FIXTURES_ROOT);
  if (!stat) {
    mkdirSync(FIXTURES_ROOT, { recursive: true });
    return;
  }
  if (!stat.isDirectory()) fail(`fixtures root is not a directory: ${FIXTURES_ROOT}`);
  assertNoSymlinksTree(FIXTURES_ROOT, 'fixtures root');
}

function assertManifestPath() {
  assertNotLivePath(MANIFEST_PATH, 'manifest');
  assertNoSymlinkChain(MANIFEST_PATH, 'manifest');
  const stat = lstatIfPresent(MANIFEST_PATH);
  if (stat && (!stat.isFile() || stat.isSymbolicLink())) fail(`manifest is not a regular file: ${MANIFEST_PATH}`);
}

function assertOraclePath() {
  assertNotLivePath(ORACLE_PATH, 'Node Tower oracle');
  assertNoSymlinkChain(ORACLE_PATH, 'Node Tower oracle');
  const stat = lstatIfPresent(ORACLE_PATH);
  if (!stat || !stat.isFile() || stat.isSymbolicLink()) fail(`Node Tower oracle is not a regular file: ${ORACLE_PATH}`);
}

function assertLiveRoot() {
  assertNoSymlinkChain(LIVE_ROOT, 'live canonical Tower input');
  const stat = lstatIfPresent(LIVE_ROOT);
  if (!stat || !stat.isDirectory() || stat.isSymbolicLink())
    fail(`live canonical Tower input is not a regular directory: ${LIVE_ROOT}`);
}

function readManifest() {
  assertManifestPath();
  const raw = readFileSync(MANIFEST_PATH, 'utf8');
  let manifest;
  try {
    manifest = JSON.parse(raw);
  } catch (error) {
    fail(`manifest is not valid JSON: ${error.message}`);
  }
  assertExactKeys(manifest, ['schema', 'snapshots', 'captured_at', 'capture_day'], 'manifest');
  if (manifest.schema !== 'jet.tower.parity.v1' || !Array.isArray(manifest.snapshots))
    fail('manifest has the wrong schema');
  assertCaptureMetadata(manifest);
  if (manifest.snapshots.length !== SNAPSHOTS.length) fail('manifest must contain exactly three snapshots');

  for (let index = 0; index < SNAPSHOTS.length; index += 1) {
    const expected = SNAPSHOTS[index];
    const item = manifest.snapshots[index];
    assertExactKeys(item, ['date', 'commit', 'fixture', 'source', 'files'], `manifest snapshot ${index}`);
    if (!item || item.date !== expected.date || item.commit !== expected.commit)
      fail(`manifest snapshot ${index} does not match the fixed commit/date set`);
    const expectedFixture = `fixtures/${expected.date}`;
    if (item.fixture !== expectedFixture) fail(`${expected.date}: manifest fixture path is not fixed`);
    assertExactKeys(item.source, ['tower.json', 'history.json', 'config.json'], `${expected.date}: manifest source map`);
    for (const [file, source] of Object.entries(SOURCE_FILES)) {
      if (item.source[file] !== source) fail(`${expected.date}: manifest source for ${file} is not fixed`);
    }
    assertExactKeys(item.files, OUTPUT_FILES, `${expected.date}: manifest file hashes`);
    for (const file of OUTPUT_FILES) {
      const record = item.files[file];
      assertExactKeys(record, ['sha256'], `${expected.date}/${file}: hash record`);
      if (record.sha256 !== null && (typeof record.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(record.sha256)))
        fail(`${expected.date}/${file}: hash is not a lowercase SHA-256 or null`);
    }
  }
  return manifest;
}

function gitShow(commit, source) {
  try {
    return execFileSync('git', ['show', `${commit}:${source}`], {
      cwd: REPO_ROOT,
      encoding: 'buffer',
      maxBuffer: 128 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (error) {
    fail(`git show failed for ${commit}:${source}: ${error.message}`);
  }
}

async function loadOracle() {
  assertOraclePath();
  const module = await import(pathToFileURL(ORACLE_PATH).href);
  if (typeof module.run !== 'function') fail(`Node Tower oracle has no run() export: ${ORACLE_PATH}`);
  return module.run;
}

async function runOracle(run, command, dataDir) {
  const stdout = [];
  const stderr = [];
  const previousLog = console.log;
  const previousError = console.error;
  const previousExitCode = process.exitCode;
  let exitCode = 0;
  console.log = (...args) => stdout.push(args.map(String).join(' '));
  console.error = (...args) => stderr.push(args.map(String).join(' '));
  process.exitCode = 0;
  try {
    await run([command, '--json', '--data', dataDir]);
    exitCode = process.exitCode ?? 0;
  } finally {
    console.log = previousLog;
    console.error = previousError;
    process.exitCode = previousExitCode;
  }
  if (stderr.length) fail(`Tower ${command} oracle failed for ${dataDir}: ${stderr.join('\n')}`);
  if (command === 'state' && exitCode !== 0) fail(`Tower state oracle exited ${exitCode} for ${dataDir}`);
  if (command === 'lint' && exitCode !== 0 && exitCode !== 1)
    fail(`Tower lint oracle exited ${exitCode} for ${dataDir}`);
  if (stdout.length !== 1 || !stdout[0].trim()) fail(`Tower ${command} oracle did not return one JSON value for ${dataDir}`);
  try {
    JSON.parse(stdout[0]);
  } catch (error) {
    fail(`Tower ${command} oracle returned invalid JSON for ${dataDir}: ${error.message}`);
  }
  return `${stdout[0]}\n`;
}

function writeFresh(path, value, label) {
  assertFixturePath(path, label);
  writeFileSync(path, value, { flag: 'wx' });
}

function stagePath(stage, file, label) {
  const path = join(stage, file);
  assertFixturePath(path, label);
  return path;
}

async function stageSnapshot(snapshot, capturedAt, run) {
  const stagePrefix = join(FIXTURES_ROOT, `.${snapshot.date}.capture-`);
  assertFixturePath(stagePrefix, `${snapshot.date} staging prefix`);
  const stage = mkdtempSync(stagePrefix);
  assertFixturePath(stage, `${snapshot.date} staging directory`);
  try {
    for (const [file, source] of Object.entries(SOURCE_FILES)) {
      writeFresh(stagePath(stage, file, `${snapshot.date}/${file}`), gitShow(snapshot.commit, source), `${snapshot.date}/${file}`);
    }
    writeFresh(stagePath(stage, 'capture-time.txt', `${snapshot.date}/capture-time.txt`), `${capturedAt}\n`, `${snapshot.date}/capture-time.txt`);
    writeFresh(stagePath(stage, 'state.json', `${snapshot.date}/state.json`), await runOracle(run, 'state', stage), `${snapshot.date}/state.json`);
    writeFresh(stagePath(stage, 'lint.json', `${snapshot.date}/lint.json`), await runOracle(run, 'lint', stage), `${snapshot.date}/lint.json`);
    return stage;
  } catch (error) {
    assertFixturePath(stage, `${snapshot.date} staging directory`);
    rmSync(stage, { recursive: true, force: true });
    throw error;
  }
}

function destinationFor(snapshot) {
  const destination = resolve(FIXTURES_ROOT, snapshot.date);
  const expected = join(FIXTURES_ROOT, snapshot.date);
  if (destination !== expected) fail(`${snapshot.date}: destination is not fixed`);
  assertFixturePath(destination, `${snapshot.date} destination`);
  return destination;
}

function replaceFixture(stage, destination, date) {
  assertFixturePath(stage, `${date} staging directory`);
  assertFixturePath(destination, `${date} destination`);
  const old = lstatIfPresent(destination);
  if (old) {
    if (old.isSymbolicLink()) fail(`${date}: refusing to replace symlink destination`);
    if (!old.isDirectory()) fail(`${date}: refusing to replace non-directory destination`);
    assertNoSymlinksTree(destination, `${date} destination`);
    rmSync(destination, { recursive: true, force: true });
  }
  renameSync(stage, destination);
  assertNoSymlinksTree(destination, `${date} destination`);
}

function sha256File(path, label) {
  assertFixturePath(path, label);
  const stat = lstatIfPresent(path);
  if (!stat || stat.isSymbolicLink() || !stat.isFile()) fail(`${label} is not a regular file`);
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function writeManifest(manifest, capturedAt, captureDay) {
  assertManifestPath();
  const updated = {
    ...manifest,
    captured_at: capturedAt,
    capture_day: captureDay,
  };
  const temp = resolve(PARITY_DIR, `.snapshots.json.capture-${process.pid}`);
  assertNotLivePath(temp, 'manifest temporary file');
  assertNoSymlinkChain(temp, 'manifest temporary file');
  if (lstatIfPresent(temp)) fail(`manifest temporary file already exists: ${temp}`);
  let wrote = false;
  try {
    writeFileSync(temp, `${JSON.stringify(updated, null, 2)}\n`, { flag: 'wx' });
    wrote = true;
    assertManifestPath();
    renameSync(temp, MANIFEST_PATH);
  } finally {
    if (wrote && lstatIfPresent(temp)) rmSync(temp, { force: true });
  }
}

async function main() {
  if (process.argv.slice(2).length) fail('capture takes no destination arguments');
  assertLiveRoot();
  assertFixtureRoot();
  const manifest = readManifest();
  const run = await loadOracle();
  const capturedAt = new Date().toISOString();
  const captureDay = capturedAt.slice(0, 10);
  const stages = [];
  try {
    for (const snapshot of SNAPSHOTS) stages.push(await stageSnapshot(snapshot, capturedAt, run));
    for (let index = 0; index < SNAPSHOTS.length; index += 1)
      replaceFixture(stages[index], destinationFor(SNAPSHOTS[index]), SNAPSHOTS[index].date);
  } catch (error) {
    for (const stage of stages) {
      if (!lstatIfPresent(stage)) continue;
      assertFixturePath(stage, 'staging directory cleanup');
      rmSync(stage, { recursive: true, force: true });
    }
    throw error;
  }

  for (let index = 0; index < SNAPSHOTS.length; index += 1) {
    const snapshot = SNAPSHOTS[index];
    const destination = destinationFor(snapshot);
    for (const file of OUTPUT_FILES) {
      const hash = sha256File(join(destination, file), `${snapshot.date}/${file}`);
      manifest.snapshots[index].files[file].sha256 = hash;
    }
  }
  writeManifest(manifest, capturedAt, captureDay);
  console.log(`captured ${SNAPSHOTS.length} Tower parity snapshots on ${captureDay}`);
}

main().catch((error) => {
  console.error(`capture: ${error.message}`);
  process.exitCode = 1;
});
