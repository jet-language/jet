// The dev tool roots for node tools. The table is Tools/Agent/roots.env, the
// one every repository tool reads; this module asks roots.sh for it, so node
// tools see exactly what shell and Jet tools see, the machine's
// ${XDG_CONFIG_HOME:-~/.config}/jet/env included. Repository tools find their
// state, scratch, queues, published compilers and seeds through these and
// nothing else (roots.env lists them). No committed file names one machine's
// copy (LaneCheck.jet --personal-info).
//
// Run as a program (`node Tools/Agent/roots.mjs NAME...`), it prints
// `NAME=value` for each root named, and exits 64 at any other name. Its Jet
// twin is Tools/Agent/Roots.jet (gauntlet tool entry RootsMjs).
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const names = ['JET_STATE_ROOT', 'JET_SCRATCH_ROOT', 'JET_RUN_ROOT', 'JET_BASE_ROOT',
  'JET_TARGET_ROOT', 'JET_SEEDS', 'JET_SEED', 'JET_BIN', 'JET_DEV_WORKTREE'];
const inputs = [...names, 'HOME', 'XDG_CACHE_HOME', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'];

// Every root, as roots.sh prints them for this process's environment; asked
// again only when one of the variables they derive from changes.
let cached = { key: null, roots: null };
const roots = () => {
  const key = JSON.stringify(inputs.map((name) => process.env[name] ?? null));
  if (cached.key !== key) {
    const out = execFileSync('bash', [join(here, 'roots.sh'), ...names], { encoding: 'utf8' });
    const values = {};
    for (const line of out.split('\n')) {
      const at = line.indexOf('=');
      if (at > 0) values[line.slice(0, at)] = line.slice(at + 1);
    }
    cached = { key, roots: values };
  }
  return cached.roots;
};

export const stateRoot = () => roots().JET_STATE_ROOT;
export const scratchRoot = () => roots().JET_SCRATCH_ROOT;
export const runRoot = () => roots().JET_RUN_ROOT;
export const baseRoot = () => roots().JET_BASE_ROOT;
export const targetRoot = () => roots().JET_TARGET_ROOT;
export const seedsRoot = () => roots().JET_SEEDS;
export const seedDir = () => roots().JET_SEED;
export const toolJet = () => roots().JET_BIN;
export const devWorktree = () => roots().JET_DEV_WORKTREE;

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const values = roots();
  for (const name of process.argv.slice(2)) {
    if (!Object.hasOwn(values, name)) {
      console.error(`roots: unknown root ${name}`);
      process.exit(64);
    }
    console.log(`${name}=${values[name]}`);
  }
}
