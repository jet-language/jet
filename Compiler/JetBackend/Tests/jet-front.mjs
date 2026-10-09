// Checked Jet MIR and native artifacts through the production Jet CLI.
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildNativeCheckedMirDriver, runNativeCheckedMir, runNativeProgram, stageNativeToolchain } from '../../../Tools/agent/compiler-native-transport.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const toolchains = new Map();
const mirDrivers = new Map();

export function candidateJetc0() {
  if (!process.env.JET_FRONT_JETC0) throw new Error('set JET_FRONT_JETC0 to the pinned native CLI candidate');
  return resolve(process.env.JET_FRONT_JETC0);
}

function transport(options) {
  const { candidate, prefix, store } = options;
  const pack = options.pack ?? process.env.JET_RUNTIME_PACK;
  if (!pack || !existsSync(pack)) throw new Error('native front requires the pinned JET_RUNTIME_PACK');
  if (!existsSync(candidate)) throw new Error(`candidate native CLI missing: ${candidate}`);
  const key = `${resolve(candidate)}\0${resolve(pack)}`;
  if (!toolchains.has(key)) toolchains.set(key, stageNativeToolchain({ jet: candidate, runtimePack: pack, out: join(dirname(prefix), 'native-toolchain') }));
  return { jet: toolchains.get(key), env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_RUNTIME_PACK: resolve(pack), ...(store ? { JET_STORE_DIR: resolve(store) } : {}) }, key };
}

export function runJetFront(options) {
  const { jet, env } = transport(options);
  return runNativeProgram({ jet, env, file: options.file, root: options.root, prefix: options.prefix }).problem;
}

export function runJetMir(options) {
  const { jet, env, key } = transport(options);
  if (!mirDrivers.has(key)) mirDrivers.set(key, buildNativeCheckedMirDriver({ jet, repo, out: join(dirname(options.prefix), 'checked-mir-transport'), env }));
  return runNativeCheckedMir({ driver: mirDrivers.get(key), env, file: options.file, mir: options.mir, prefix: options.prefix, cwd: options.cwd ?? options.root }).problem;
}
