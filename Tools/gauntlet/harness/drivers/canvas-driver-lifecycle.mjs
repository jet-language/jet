// The gauntlet driver of Tools/canvas-test/driver-lifecycle.mjs (#5072 c3),
// the isolated Chromium lifecycle check, with the command line of its twin,
// Tools/canvas-test/Run.jet; Tools/gauntlet/entries/canvas-driver-lifecycle/
// driver/run.jet is the twin's driver.
//
//   node canvas-driver-lifecycle.mjs driver-lifecycle [COUNT]
//
// Chromium is $CHROMIUM when set, else the chromium of the nixpkgs revision
// the repository's flake.lock ($FLAKE_LOCK when set) pins, so the check runs
// the same on any machine with the repository's nix inputs. A lock that pins
// nixpkgs other than from GitHub is refused (exit 1).
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '../../../..');
const [mode, ...rest] = process.argv.slice(2);
if (mode !== 'driver-lifecycle') {
  console.error('usage: node canvas-driver-lifecycle.mjs driver-lifecycle [COUNT]');
  process.exit(2);
}

// The nixpkgs revision that $FLAKE_LOCK, else the repository's flake.lock,
// pins, as a flake reference.
function pinnedNixpkgs() {
  const lock = JSON.parse(readFileSync(process.env.FLAKE_LOCK ?? join(repo, 'flake.lock'), 'utf8'));
  const locked = lock.nodes[lock.nodes.root.inputs.nixpkgs].locked;
  if (locked.type !== 'github') throw new Error('flake.lock pins nixpkgs other than from GitHub');
  return `github:${locked.owner}/${locked.repo}/${locked.rev}`;
}

if (!process.env.CHROMIUM) {
  const built = execFileSync('nix', ['build', '--no-link', '--print-out-paths', `${pinnedNixpkgs()}#chromium`], { encoding: 'utf8' });
  process.env.CHROMIUM = join(built.trim(), 'bin/chromium');
}
// The lifecycle check reads its count as its first argument.
process.argv = [process.argv[0], join(repo, 'Tools/canvas-test/driver-lifecycle.mjs'), ...rest];
await import('../../../canvas-test/driver-lifecycle.mjs');
