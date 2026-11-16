// The gauntlet driver of the source-map check's live half,
// Tools/gauntlet/entries/source-map-live/original/sourcemap-live.mjs (#5072
// c65), with the command line of its twin; Tools/gauntlet/entries/
// source-map-live/driver/run.jet is the twin's driver.
//
//   node source-map-live.mjs SITE
//
// It serves the working directory on a free loopback port and runs the check
// against SITE, a directory there holding a built page (index.html, app.js
// and its maps). Chromium is $CHROMIUM when set, else the chromium of the
// nixpkgs revision the repository's flake.lock ($FLAKE_LOCK when set) pins,
// so the check runs the same on any machine with the repository's nix inputs.
// A lock that pins nixpkgs other than from GitHub is refused (exit 1).
import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = join(dirname(fileURLToPath(import.meta.url)), '../../../..');
const [site] = process.argv.slice(2);
if (!site) {
  console.error('usage: node source-map-live.mjs SITE');
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

// The working directory's files, a directory's index.html for a path that
// ends in `/`; each response closes its connection, so the server holds
// nothing open once the check is done.
const types = { '.html': 'text/html', '.js': 'text/javascript', '.map': 'application/json', '.wasm': 'application/wasm' };
const root = process.cwd();
const server = createServer(async (request, response) => {
  const path = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname);
  const file = join(root, path.endsWith('/') ? `${path}index.html` : path);
  try {
    const body = await readFile(file);
    response.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream', connection: 'close' });
    response.end(body);
  } catch {
    response.writeHead(404, { connection: 'close' });
    response.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
server.unref();

process.argv = [process.argv[0], join(repo, 'Tools/gauntlet/entries/source-map-live/original/sourcemap-live.mjs'), '--port', String(server.address().port), '--prefix', `/${site}`];
await import('../../entries/source-map-live/original/sourcemap-live.mjs');
