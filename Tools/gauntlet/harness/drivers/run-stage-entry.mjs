// The gauntlet driver of the staging export of
// Tools/gauntlet/harness/run.mjs (#5072 c8): stageEntry.
// Tools/gauntlet/entries/run-stage-entry/driver/run.jet is the twin on the
// kit's Stage module.
//
//   node run-stage-entry.mjs stage DIR RUNS DEV
//
// `stage` stages the comparison entry in DIR (its entry.json) under run/ and
// measures it, RUNS samples to each measurement ("default" for the entry's
// own count) and the dev tier only when DEV is "dev", with ./jet as the
// `jet` it builds and runs with: fake-jet.txt, made executable. Every
// measured command runs through the launcher, built into ./launch from
// Tools/gauntlet/harness/launch.c. It shows the result as JSON indented by
// two spaces, or the error that stopped it. A measurement (a time, a peak, a
// latency, a ratio, a noise or a median) reads `<measured>`, as does what
// follows from one: a comparison's verdict and status and the cache state a
// run tier settles on. Sizes, counts, exit codes and source metrics stay.
// This directory reads `<cwd>` and the checkout `<repo>`, and each `note:`
// line a Jet program (Tools/agent/JetEnv.jet among them) prints on stderr is
// left out, so where the checkout lives changes no output.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { stageEntry } from '../run.mjs';

const usage = 'usage: node run-stage-entry.mjs stage DIR RUNS DEV';
const [command, dir, runs, dev] = process.argv.slice(2);
const cwd = process.cwd();
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..');

// The keys whose numbers are not measurements.
const KEPT = new Set(['artifact_bytes', 'binary_bytes', 'bytes', 'count', 'exit_code', 'exitCode', 'loc', 'resolution_ns', 'source_bytes', 'source_tokens', 'tokens', 'wasm_bytes']);
// The keys whose text follows from a measurement; each entry of a `verdicts`
// map reads as a `verdict`.
const DERIVED = new Set(['cache_state', 'status', 'verdict']);

// `value` with each measurement, and what follows from one, as `<measured>`.
function masked(value, key, measured) {
  if (typeof value === 'number') return KEPT.has(key) ? value : '<measured>';
  if (Array.isArray(value)) return value.map((item) => masked(item, key, measured));
  if (value === null || typeof value !== 'object') return measured && DERIVED.has(key) && typeof value === 'string' ? '<measured>' : value;
  const out = {};
  for (const [name, item] of Object.entries(value)) out[name] = masked(item, key === 'verdicts' ? 'verdict' : name, measured || key === 'comparisons' || name === 'cache_state');
  return out;
}

// `text` without the `note:` lines of the stderr it shows as JSON: each runs
// to its escaped newline or to the end of its string.
function withoutNotes(text) {
  const [first, ...notes] = text.split('note: ');
  return first + notes.map((part) => {
    const line = part.indexOf('\\n');
    const quote = part.indexOf('"');
    if (line >= 0 && (quote < 0 || line < quote)) return part.slice(line + 2);
    return quote < 0 ? `note: ${part}` : part.slice(quote);
  }).join('');
}

if (command === 'stage' && dir && runs && dev) {
  try {
    const jet = path.join(cwd, 'jet');
    fs.copyFileSync('fake-jet.txt', jet);
    fs.chmodSync(jet, 0o755);
    const launcher = path.join(cwd, 'launch');
    const built = spawnSync('cc', ['-O2', '-o', launcher, path.join(repo, 'Tools/gauntlet/harness/launch.c')], { encoding: 'utf8' });
    if (built.status !== 0) throw new Error(`cannot build the launcher: ${built.stderr.trim()}`);
    process.env.JET_GAUNTLET_LAUNCHER = launcher;
    const entryDir = path.resolve(dir);
    const entry = JSON.parse(fs.readFileSync(path.join(entryDir, 'entry.json'), 'utf8'));
    const result = await stageEntry(entryDir, entry, path.join(cwd, 'run'), jet, runs === 'default' ? null : Number(runs), dev === 'dev');
    console.log(withoutNotes(JSON.stringify(masked(result, '', false), null, 2).replaceAll(cwd, '<cwd>').replaceAll(repo, '<repo>')));
  } catch (error) {
    console.log(`error: ${error.message.replaceAll(cwd, '<cwd>').replaceAll(repo, '<repo>')}`);
    process.exitCode = 1;
  }
} else {
  console.error(usage);
  process.exit(64);
}
