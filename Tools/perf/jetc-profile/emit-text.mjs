#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const sourcePath = 'Compiler/JetCodegen/Source/Emit/Functions.jet';
const sha256 = value => crypto.createHash('sha256').update(value).digest('hex');

// Use declaration boundaries, never a brace counter: Rust text and Jet
// interpolation contain braces that are not Jet declaration delimiters.
export function helpers(source) {
  const begin = source.indexOf('\nfn jet_rust_emit_blank_line(');
  const localized = source.indexOf('\nstruct JetRustEmitLocalizedSlots {', begin);
  const resume = source.indexOf('\nfn jet_rust_emit_declared_value_id(', localized);
  const end = source.indexOf('\nfn jet_rust_emit_function_signature(', resume);
  if (begin < 0 || localized <= begin || resume <= localized || end <= resume) throw new Error('emit helper declaration boundaries missing or reordered');
  // The declaration's preceding comment can be retained; it references no MIR.
  return source.slice(begin, localized) + source.slice(resume, end);
}

export function prepare(plainRef, out) {
  if (!/^[0-9a-f]{7,40}$/.test(plainRef)) throw new Error('plain reference must be an immutable commit');
  if (fs.existsSync(out)) throw new Error(`refusing to overwrite evidence: ${out}`);
  const git = spawnSync('git', ['show', `${plainRef}:${sourcePath}`], { cwd: repo, encoding: 'utf8', maxBuffer: 4 * 1024 * 1024 });
  if (git.status !== 0) throw new Error(git.stderr || 'plain source unavailable');
  const driver = fs.readFileSync(path.join(here, 'emit-text.jet'), 'utf8');
  const sources = { plain: git.stdout, optimized: fs.readFileSync(path.join(repo, sourcePath), 'utf8') };
  const arms = {};
  for (const [arm, source] of Object.entries(sources)) {
    const block = helpers(source);
    // Only the private helper argument representation changed. Keep the
    // regression input bytes and all workload/measurement code identical.
    const adaptedDriver = source.includes('fn jet_rust_emit_fold_identifiers(bytes: [U8])')
      ? driver.replace(/jet_rust_emit_fold_identifiers\(("(?:[^"\\]|\\.)*")\)/g, 'jet_rust_emit_fold_identifiers($1.bytes())')
      : driver;
    const program = adaptedDriver + '\n' + block;
    const dir = path.join(out, arm);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'main.jet'), program);
    arms[arm] = { source_sha256: sha256(source), program_sha256: sha256(program) };
  }
  const receipt = { schema: 'jet.emit-text-pair.v1', plain_commit: plainRef, driver_sha256: sha256(driver), arms };
  fs.writeFileSync(path.join(out, 'pair.json'), JSON.stringify(receipt, null, 2) + '\n');
  return receipt;
}

export function compare(plainOutputs, optimizedOutputs) {
  if (plainOutputs.length < 3 || plainOutputs.length !== optimizedOutputs.length) throw new Error('require at least three matched trials');
  const parse = output => {
    const times = {}, oracle = [];
    for (const line of output.split('\n')) {
      if (!line.startsWith('@@time ')) { oracle.push(line); continue; }
      const match = /^@@time (fold|unwrap|compact) ([0-9]+)$/.exec(line);
      if (!match || match[1] in times || BigInt(match[2]) <= 0n) throw new Error('invalid, duplicate, or non-positive timing');
      times[match[1]] = BigInt(match[2]);
    }
    if (Object.keys(times).length !== 3 || oracle[0] !== 'emit-text regressions ok') throw new Error('missing timing or regression oracle');
    return { oracle: oracle.join('\n'), times };
  };
  const plain = plainOutputs.map(parse), optimized = optimizedOutputs.map(parse);
  const oracle = plain[0].oracle;
  if ([...plain, ...optimized].some(row => row.oracle !== oracle)) throw new Error('paired emitted text differs');
  const median = values => [...values].sort((a,b) => a < b ? -1 : a > b ? 1 : 0)[Math.floor(values.length / 2)];
  const metrics = {};
  for (const name of ['fold', 'unwrap', 'compact']) {
    const base = median(plain.map(row => row.times[name]));
    const candidate = median(optimized.map(row => row.times[name]));
    metrics[name] = { plain_ns: String(base), optimized_ns: String(candidate), verdict: candidate < base ? 'win' : 'loss' };
  }
  return { schema: 'jet.emit-text-pair.v1', trials: plain.length, output_sha256: sha256(oracle), metrics, verdict: Object.values(metrics).every(row => row.verdict === 'win') ? 'win' : 'loss' };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, input, out] = process.argv.slice(2);
  if (!input || !out) throw new Error('usage: emit-text.mjs prepare PLAIN_COMMIT NEW_OUTPUT_DIRECTORY | compare PAIR_DIRECTORY NEW_RECEIPT_FILE');
  if (command === 'prepare') {
    console.log(JSON.stringify(prepare(input, path.resolve(out)), null, 2));
  } else if (command === 'compare') {
    const pairDir = path.resolve(input);
    const pair = JSON.parse(fs.readFileSync(path.join(pairDir, 'pair.json'), 'utf8'));
    if (pair.schema !== 'jet.emit-text-pair.v1') throw new Error('unsupported pair schema');
    const trialNames = arm => fs.readdirSync(path.join(pairDir, arm)).filter(name => /^trial-[0-9]+\\.out$/.test(name)).sort();
    const names = trialNames('plain');
    if (JSON.stringify(names) !== JSON.stringify(trialNames('optimized'))) throw new Error('unmatched trial files');
    for (const arm of ['plain', 'optimized']) {
      if (sha256(fs.readFileSync(path.join(pairDir, arm, 'main.jet'))) !== pair.arms[arm].program_sha256) throw new Error('program changed after pair preparation');
    }
    const outputs = arm => names.map(name => fs.readFileSync(path.join(pairDir, arm, name), 'utf8'));
    const receipt = { ...compare(outputs('plain'), outputs('optimized')), pair, trial_files: names };
    fs.writeFileSync(out, JSON.stringify(receipt, null, 2) + '\\n', { flag: 'wx' });
    console.log(JSON.stringify(receipt, null, 2));
    if (receipt.verdict !== 'win') process.exitCode = 1;
  } else {
    throw new Error(`unknown emit-text command: ${command}`);
  }
}
