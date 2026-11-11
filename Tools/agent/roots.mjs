// The dev tool roots. Repository tools find their state, scratch, queues,
// published compilers and seeds through these and nothing else; Tools/agent/
// roots.sh is the same table for shell tools. A machine sets its own values in
// the environment (for example in ~/.config/jet/env); without them the
// defaults derive from $HOME and the XDG directories, so a fresh clone needs
// no setup. No committed file names one machine's copy (LaneCheck.jet --personal-info).
//
//   JET_STATE_ROOT    per-user dev state and caches  ($XDG_CACHE_HOME/jet-dev)
//   JET_SCRATCH_ROOT  large shared scratch           ($JET_STATE_ROOT)
//   JET_RUN_ROOT      land queue, locks, slot ledger ($JET_SCRATCH_ROOT/run)
//   JET_BASE_ROOT     published compilers            ($JET_SCRATCH_ROOT/discovery/base)
//   JET_TARGET_ROOT   build target dirs              ($JET_SCRATCH_ROOT/targets)
//   JET_SEEDS         seed store                     ($XDG_DATA_HOME/jet-seeds)
//   JET_SEED          the admitted seed dir          ($JET_SEEDS/current)
//   JET_BIN           the jet under test, which `jet test` passes its children; node
//                     tools run repository Jet tools with it  ($JET_SEED/jet)
//
// Run as a program (`node Tools/agent/roots.mjs NAME...`), it prints
// `NAME=value` for each root named, and exits 64 at any other name. Its Jet
// twin is Tools/agent/orchestrator/Roots.jet (gauntlet tool entry roots-mjs).
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const env = process.env;
const home = () => env.HOME || homedir();

export const stateRoot = () => env.JET_STATE_ROOT || join(env.XDG_CACHE_HOME || join(home(), '.cache'), 'jet-dev');
export const scratchRoot = () => env.JET_SCRATCH_ROOT || stateRoot();
export const runRoot = () => env.JET_RUN_ROOT || join(scratchRoot(), 'run');
export const baseRoot = () => env.JET_BASE_ROOT || join(scratchRoot(), 'discovery/base');
export const targetRoot = () => env.JET_TARGET_ROOT || join(scratchRoot(), 'targets');
export const seedsRoot = () => env.JET_SEEDS || join(env.XDG_DATA_HOME || join(home(), '.local/share'), 'jet-seeds');
export const seedDir = () => env.JET_SEED || join(seedsRoot(), 'current');
export const toolJet = () => env.JET_BIN || join(seedDir(), 'jet');

const roots = {
  JET_STATE_ROOT: stateRoot,
  JET_SCRATCH_ROOT: scratchRoot,
  JET_RUN_ROOT: runRoot,
  JET_BASE_ROOT: baseRoot,
  JET_TARGET_ROOT: targetRoot,
  JET_SEEDS: seedsRoot,
  JET_SEED: seedDir,
  JET_BIN: toolJet,
};
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  for (const name of process.argv.slice(2)) {
    if (!Object.hasOwn(roots, name)) {
      console.error(`roots: unknown root ${name}`);
      process.exit(64);
    }
    console.log(`${name}=${roots[name]()}`);
  }
}
