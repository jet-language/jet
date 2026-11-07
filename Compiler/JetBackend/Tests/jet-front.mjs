// Native artifacts through the production Jet CLI: Tools/agent/NativeTransport.jet,
// run by the tool jet, drives the candidate and checks for fresh artifacts.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { toolJet } from '../../../Tools/agent/roots.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const toolchains = new Map();

export function candidateJetc0() {
  if (!process.env.JET_FRONT_JETC0) throw new Error('set JET_FRONT_JETC0 to the pinned native CLI candidate');
  return resolve(process.env.JET_FRONT_JETC0);
}

// `jet Tools/agent/NativeTransport.jet ARGS` and its printed result. The
// candidate it runs sees this process's environment plus every entry of `env`
// that differs from it.
function nativeTransport(args, env = process.env, timeout) {
  const settings = Object.entries(env).filter(([name, value]) => process.env[name] !== value).flatMap(([name, value]) => ['--set', `${name}=${value}`]);
  const deadline = timeout === undefined ? [] : ['--timeout', String(timeout)];
  return JSON.parse(execFileSync(toolJet(), ['Tools/agent/NativeTransport.jet', ...args, ...settings, ...deadline], { cwd: repo, encoding: 'utf8', maxBuffer: 256 << 20, stdio: ['ignore', 'pipe', 'inherit'] }));
}

// lifecycle.list for a library object built from exactly the runtime source
// text `source`: Tools/agent/RuntimeGenerators.jet lifecycle, run by the tool jet.
export function runtimeLifecycleList(source) {
  return execFileSync(toolJet(), ['Tools/agent/RuntimeGenerators.jet', 'lifecycle', '-'], { cwd: repo, input: source, encoding: 'utf8', maxBuffer: 128 << 20, stdio: ['pipe', 'pipe', 'inherit'] });
}

// Lower `file` as a native library beside `prefix` (its lifecycle list
// generated when absent); { run, problem } as NativeTransport.jet reports it.
export function runNativeLibrary({ jet, file, prefix, lifecycle = prefix + '.lifecycle.list', exports = null, env = process.env, timeout }) {
  mkdirSync(dirname(lifecycle), { recursive: true });
  if (!existsSync(lifecycle)) writeFileSync(lifecycle, runtimeLifecycleList(readFileSync(file, 'utf8')));
  const exportList = exports ? ['--exports', resolve(exports)] : [];
  return nativeTransport(['library', resolve(jet), resolve(file), resolve(prefix), resolve(lifecycle), ...exportList], env, timeout);
}

function transport(options) {
  const { candidate, prefix, store } = options;
  const pack = options.pack ?? process.env.JET_RUNTIME_PACK;
  if (!pack || !existsSync(pack)) throw new Error('native front requires the pinned JET_RUNTIME_PACK');
  if (!existsSync(candidate)) throw new Error(`candidate native CLI missing: ${candidate}`);
  const key = `${resolve(candidate)}\0${resolve(pack)}`;
  if (!toolchains.has(key)) toolchains.set(key, nativeTransport(['stage', resolve(candidate), resolve(pack), resolve(dirname(prefix), 'native-toolchain')]).jet);
  return { jet: toolchains.get(key), env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_RUNTIME_PACK: resolve(pack), ...(store ? { JET_STORE_DIR: resolve(store) } : {}) } };
}

export function runJetFront(options) {
  const { jet, env } = transport(options);
  const root = options.root ?? dirname(options.file);
  return nativeTransport(['program', jet, resolve(options.file), resolve(root), resolve(options.prefix)], env).problem;
}
