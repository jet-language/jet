#!/usr/bin/env node
// Markdown report of scaling-gate receipts (Tower #4601): per corpus, layout
// and compiler, the ladder outcome, every superlinear phase with its
// dominating counter and the responsible functions (from --profile), the
// lines/s floor failures, and for domain corpora each phase projected to 1M
// lines against the 10 s budget.
//
// node report.mjs RECEIPT.json... > SCALING-REPORT.md

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const fmt = (value, digits = 2) => (typeof value === 'number' && Number.isFinite(value) ? value.toFixed(digits) : value === Infinity ? 'inf' : '?');

export function report(receipts) {
  const out = ['# Scaling gate report (#4601)', ''];
  for (const { file, receipt } of receipts) {
    out.push(`Receipt \`${file}\`: ${receipt.startedAt} to ${receipt.finishedAt ?? '(running)'}, mode ${receipt.mode ?? 'timing'}, compilers ${receipt.compilers.map(c => `${c.label}=${c.path ?? c.template}`).join(', ')}, ${receipt.failures.length} failures.`, '');
  }
  const series = receipts.flatMap(({ receipt }) => receipt.series);
  out.push('Counter-only series gate deterministic work and compile outcomes; their contended wall/RSS observations are not timing samples. Timing floors and 1M projections come only from serial timing-mode series. Unmeasured timing cells remain uncovered, not wins.', '');
  out.push('## Ladder outcome', '', '| Corpus | Layout | Compiler | Mode | Rungs passed (x: N or lines) | Top rung wall s | Peak RSS GB | Stopped |', '|---|---|---|---|---|---:|---:|---|');
  for (const s of series) {
    const ok = s.points.filter(p => p.ok);
    const top = ok[ok.length - 1];
    out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${s.mode ?? 'timing'} | ${ok.map(p => p.x).join(', ') || 'none'} | ${top && s.mode !== 'counters' ? fmt(top.metrics.wall_seconds, 1) : '-'} | ${top && s.mode !== 'counters' ? fmt(top.metrics.peak_rss_kb / 1048576) : '-'} | ${s.truncated ?? ''} |`);
  }
  out.push('', '## Superlinear phases and counters (growth vs N log N over the measured rungs)', '',
    '| Corpus | Layout | Compiler | Mode | Metric | x N log N | Exponent | Dominating counter | Responsible functions (profile, phase share) |', '|---|---|---|---|---|---:|---:|---|---|');
  for (const s of series) {
    for (const f of s.failures ?? []) {
      if (f.kind !== 'growth') continue;
      const hot = s.profile?.phases?.[f.phase]?.inclusive?.slice(0, 3)
        .map(row => `${row.source ?? row.symbol} ${(row.share * 100).toFixed(0)}%`).join('; ') ?? '(not profiled)';
      out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${s.mode ?? 'timing'} | ${f.metric} | ${fmt(f.ratio)} | ${fmt(f.exponent)} | ${f.counter ?? '-'} | ${hot} |`);
    }
  }
  out.push('', '## Floor failures at the top rung', '', '| Corpus | Layout | Compiler | Failure |', '|---|---|---|---|');
  for (const s of series) for (const f of s.failures ?? []) if (f.kind === 'floor') out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${f.text} |`);
  out.push('', '## Compile failures, timeouts and incomplete ladders', '',
    '| Corpus | Layout | Compiler | Mode | Phase | Failure |', '|---|---|---|---|---|---|');
  for (const s of series) {
    for (const f of s.failures ?? []) {
      if (!['compile', 'timeout', 'truncated'].includes(f.kind)) continue;
      out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${s.mode ?? 'timing'} | ${f.phase ?? '-'} | ${f.text} |`);
    }
    if (s.done === false || (!s.done && !s.failures)) out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${s.mode ?? 'timing'} | - | incomplete checkpoint |`);
  }
  out.push('', '## Domain corpora: phases projected to 1M lines (budget 10 s end to end)', '',
    '| Corpus | Layout | Compiler | Measured at lines | Phase | Measured s | Exponent | Projected s at 1M |', '|---|---|---|---:|---|---:|---:|---:|');
  for (const s of series) {
    if (!s.projection) continue;
    const rows = Object.entries(s.projection.phases).sort(([, a], [, b]) => b.projected_s - a.projected_s);
    for (const [key, p] of rows) {
      out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${p.at_lines} | ${key === 'wall_seconds' ? '**total**' : key.slice(6)} | ${fmt(p.measured_s)} | ${fmt(p.exponent)} | ${fmt(p.projected_s, 1)} |`);
    }
  }
  out.push('', '## Timing coverage not established', '', '| Corpus | Layout | Compiler | Reason |', '|---|---|---|---|');
  for (const s of series.filter(s => s.mode === 'counters')) {
    const timed = series.find(t => t.mode !== 'counters' && t.shape === s.shape && t.layout === s.layout && t.compiler === s.compiler);
    if (!timed?.points.some(point => point.ok)) out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${timed ? 'timing compilation failed' : 'not in the timing sample'} |`);
  }
  return `${out.join('\n')}\n`;
}

export function readReceipts(args) {
  const files = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--runs-dir') {
      const dir = args[++i];
      if (!dir) throw new Error('--runs-dir requires a directory');
      if (!fs.existsSync(dir)) continue;
      for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
        const file = path.join(dir, entry.name, 'receipt.json');
        if (entry.isDirectory() && entry.name.startsWith('run-') && fs.existsSync(file)) files.push(file);
      }
    } else files.push(args[i]);
  }
  return [...new Set(files)].map(file => ({ file, receipt: JSON.parse(fs.readFileSync(file, 'utf8')) }));
}

// Wait on completion events, not a polling loop or a second measurement run.
async function waitForJobs(doneDir, plan) {
  const jobs = plan.map(job => `${job.name}.rc`);
  await new Promise((resolve, reject) => {
    const watcher = fs.watch(doneDir, (_event, file) => {
      if (file && jobs.includes(file)) check();
    });
    watcher.on('error', error => { watcher.close(); reject(error); });
    function check() {
      if (jobs.every(job => fs.existsSync(path.join(doneDir, job)))) {
        watcher.close();
        resolve();
      }
    }
    check();
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const inputs = [];
  let output = null;
  let findings = null;
  let doneDir = null;
  let planFile = null;
  for (let i = 2; i < process.argv.length; i++) {
    const arg = process.argv[i];
    if (arg === '--output') output = process.argv[++i];
    else if (arg === '--findings') findings = process.argv[++i];
    else if (arg === '--wait-proofq') doneDir = process.argv[++i];
    else if (arg === '--plan') planFile = process.argv[++i];
    else inputs.push(arg);
  }
  const plan = planFile ? JSON.parse(fs.readFileSync(planFile, 'utf8')) : null;
  if (doneDir) {
    if (!plan) throw new Error('--wait-proofq requires --plan FILE');
    await waitForJobs(doneDir, plan);
  }
  const receipts = readReceipts([...inputs, ...(plan?.flatMap(job => ['--runs-dir', job.config.workDir]) ?? [])]);
  if (!receipts.length && !plan) {
    console.error('Usage: node report.mjs RECEIPT.json... | --runs-dir DIR [--output FILE] [--findings FILE] [--wait-proofq DONE_DIR --plan FILE]');
    process.exit(2);
  }
  let text = report(receipts);
  if (plan) {
    const missing = plan.flatMap(job => job.config.shapes.filter(shape => !receipts.some(({ receipt }) =>
      receipt.finishedAt && receipt.mode === job.config.mode && receipt.series.some(series => series.shape === shape)))
      .map(shape => `${job.config.mode}/${shape}`));
    if (missing.length) {
      text += `\n## Missing or unfinished receipts\n\nThe proofq jobs ended without complete receipts for: ${missing.join(', ')}. These cells are not covered.\n`;
      process.exitCode = 2;
    }
  }
  if (findings) text += `\n${fs.readFileSync(findings, 'utf8')}`;
  if (output) {
    fs.writeFileSync(output, text);
    console.log(`Report: ${output}; ${receipts.length} receipts, ${receipts.reduce((sum, { receipt }) => sum + receipt.series.length, 0)} series, ${receipts.reduce((sum, { receipt }) => sum + receipt.failures.length, 0)} gate failures`);
  } else process.stdout.write(text);
}
