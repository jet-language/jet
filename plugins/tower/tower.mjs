#!/usr/bin/env node
// Tower entry point. `node tower.mjs help` for the full surface.
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const towerRoot = dirname(fileURLToPath(import.meta.url));
const checkoutRoot = resolve(towerRoot, '..', '..');

// A linked worktree can carry an older vendored app/paths.mjs. Load Tower's
// app from the main checkout first, so stale path code cannot redirect normal
// board discovery to a worktree board. Standalone copies keep local entrypoint.
function canonicalCli() {
  try {
    const gitRoot = resolve(checkoutRoot, execFileSync(
      'git', ['-C', checkoutRoot, 'rev-parse', '--show-toplevel'],
      { encoding: 'utf8' },
    ).trim());
    const commonDir = resolve(checkoutRoot, execFileSync(
      'git', ['-C', checkoutRoot, 'rev-parse', '--path-format=absolute', '--git-common-dir'],
      { encoding: 'utf8' },
    ).trim());
    const mainRoot = dirname(commonDir);
    const mainTowerRoot = join(mainRoot, relative(gitRoot, towerRoot));
    return pathToFileURL(join(mainTowerRoot, 'app', 'cli.mjs')).href;
  } catch {
    return new URL('./app/cli.mjs', import.meta.url).href;
  }
}

const { run } = await import(canonicalCli());
await run(process.argv.slice(2));
