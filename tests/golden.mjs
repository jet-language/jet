#!/usr/bin/env node
// Native replacement for the Cargo golden target: the canonical Jet testkit
// owns discovery, fixture staging, tiers, streams and verdicts.
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ran = spawnSync(process.execPath, [join(repo, 'Tools/testkit/run.mjs'), 'golden', ...process.argv.slice(2)], { cwd: repo, env: process.env, stdio: 'inherit' });
if (ran.error || ran.signal) { console.error(ran.error ?? ran.signal); process.exit(2); }
process.exitCode = ran.status ?? 2;
