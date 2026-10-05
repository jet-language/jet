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
    out.push(`Receipt \`${file}\`: ${receipt.startedAt} to ${receipt.finishedAt ?? '(running)'}, compilers ${receipt.compilers.map(c => `${c.label}=${c.path ?? c.template}`).join(', ')}, ${receipt.failures.length} failures.`, '');
  }
  const series = receipts.flatMap(({ receipt }) => receipt.series);
  out.push('## Ladder outcome', '', '| Corpus | Layout | Compiler | Rungs passed (x: N or lines) | Top rung wall s | Peak RSS GB | Stopped |', '|---|---|---|---|---:|---:|---|');
  for (const s of series) {
    const ok = s.points.filter(p => p.ok);
    const top = ok[ok.length - 1];
    out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${ok.map(p => p.x).join(', ') || 'none'} | ${top ? fmt(top.metrics.wall_seconds, 1) : '-'} | ${top ? fmt(top.metrics.peak_rss_kb / 1048576) : '-'} | ${s.truncated ?? ''} |`);
  }
  out.push('', '## Superlinear phases and counters (growth vs N log N over the measured rungs)', '',
    '| Corpus | Layout | Compiler | Metric | x N log N | Exponent | Dominating counter | Responsible functions (profile, phase share) |', '|---|---|---|---|---:|---:|---|---|');
  for (const s of series) {
    for (const f of s.failures ?? []) {
      if (f.kind !== 'growth') continue;
      const hot = s.profile?.phases?.[f.phase]?.inclusive?.slice(0, 3)
        .map(row => `${row.source ?? row.symbol} ${(row.share * 100).toFixed(0)}%`).join('; ') ?? '(not profiled)';
      out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${f.metric} | ${fmt(f.ratio)} | ${fmt(f.exponent)} | ${f.counter ?? '-'} | ${hot} |`);
    }
  }
  out.push('', '## Floor failures at the top rung', '', '| Corpus | Layout | Compiler | Failure |', '|---|---|---|---|');
  for (const s of series) for (const f of s.failures ?? []) if (f.kind === 'floor') out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${f.text} |`);
  out.push('', '## Compile failures, timeouts and incomplete ladders', '',
    '| Corpus | Layout | Compiler | Phase | Failure |', '|---|---|---|---|---|');
  for (const s of series) {
    for (const f of s.failures ?? []) {
      if (!['compile', 'timeout', 'truncated'].includes(f.kind)) continue;
      out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | ${f.phase ?? '-'} | ${f.text} |`);
    }
    if (s.done === false || (!s.done && !s.failures)) out.push(`| ${s.shape} | ${s.layout} | ${s.compiler} | - | incomplete checkpoint |`);
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
  return `${out.join('\n')}\n`;
}

export function readReceipts(args) {
  const files = [];
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--runs-dir') {
      const dir = args[++i];
      if (!dir) throw new Error('--runs-dir requires a directory');
      for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
        const file = path.join(dir, entry.name, 'receipt.json');
        if (entry.isDirectory() && entry.name.startsWith('run-') && fs.existsSync(file)) files.push(file);
      }
    } else files.push(args[i]);
  }
  return [...new Set(files)].map(file => ({ file, receipt: JSON.parse(fs.readFileSync(file, 'utf8')) }));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const receipts = readReceipts(process.argv.slice(2));
  if (!receipts.length) {
    console.error('Usage: node report.mjs RECEIPT.json... | --runs-dir DIR');
    process.exit(2);
  }
  process.stdout.write(report(receipts));
}
