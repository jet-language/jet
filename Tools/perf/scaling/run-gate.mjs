#!/usr/bin/env node
// One resumable scaling gate: one receipt per shape, both layouts, every compiler.
import fs from 'node:fs';
import path from 'node:path';
import { options, run } from './harness.mjs';
import { report } from './report.mjs';

let config;
try {
  config = options(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
if (!config) {
  console.log('Usage: node Tools/perf/scaling/run-gate.mjs --compiler LABEL=KIND:PATH [harness options]\nEach shape has its own resumable receipt and report under --work-dir.');
} else {
  try {
    const receipts = [];
    for (const shape of config.shapes) {
      const workDir = path.join(config.workDir, `run-${shape}`);
      const file = path.join(workDir, 'receipt.json');
      const receipt = await run({ ...config, shapes: [shape], workDir, receipt: file });
      receipts.push({ file, receipt });
      fs.writeFileSync(path.join(workDir, 'report.md'), report([{ file, receipt }]));
    }
    fs.writeFileSync(path.join(config.workDir, 'report.md'), report(receipts));
    console.log(`Report: ${path.join(config.workDir, 'report.md')}`);
    process.exitCode = receipts.every(({ receipt }) => receipt.passed) ? 0 : 1;
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
