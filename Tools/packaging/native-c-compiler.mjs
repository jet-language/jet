#!/usr/bin/env node
// Native Jet consumers link against musl: glibc's fortified entry points are
// not part of that platform ABI. Scope the policy to this C compiler child.
// The policy rows live once in Compiler/JetCli/Source/Cli/native-c-policy.tsv;
// the Jet CLI embeds the same file while the compiler builds (NativeCPolicy.jet).
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const nativeCCompiler = fileURLToPath(import.meta.url);
export const nativeCPolicyPath = resolve(dirname(nativeCCompiler), '../../Compiler/JetCli/Source/Cli/native-c-policy.tsv');

export function parseNativeCPolicy(text) {
  const flags = [], hardeningRemoved = [];
  for (const [index, line] of text.split('\n').entries()) {
    if (line === '' || line.startsWith('#')) continue;
    const fields = line.split('\t');
    if (fields.length !== 2 || fields[1] === '') throw new Error(`native-c-policy.tsv:${index + 1}: expected "kind<TAB>value"`);
    if (fields[0] === 'flag') flags.push(fields[1]);
    else if (fields[0] === 'hardening-remove') hardeningRemoved.push(fields[1]);
    else throw new Error(`native-c-policy.tsv:${index + 1}: unknown policy kind ${JSON.stringify(fields[0])}`);
  }
  return { flags: Object.freeze(flags), hardeningRemoved: Object.freeze(hardeningRemoved) };
}

const policy = parseNativeCPolicy(readFileSync(nativeCPolicyPath, 'utf8'));
export const nativeCFlags = policy.flags;
export function nativeCEnvironment(environment = process.env) {
  const child = { ...environment };
  // Nix's compiler wrapper adds fortify after caller flags; its setup-hook
  // NIX_HARDENING_DISABLE knob is not consulted by an already-built wrapper.
  if (child.NIX_HARDENING_ENABLE !== undefined) {
    child.NIX_HARDENING_ENABLE = child.NIX_HARDENING_ENABLE.split(/\s+/u)
      .filter(flag => flag && !policy.hardeningRemoved.includes(flag)).join(' ');
  }
  return child;
}

if (process.argv[1] && resolve(process.argv[1]) === nativeCCompiler) {
  // This wrapper must never reach itself again: a CC that names this file (or
  // a `cc` on PATH that resolves back here) would fork without bound.
  if (process.env.JET_NATIVE_C_WRAPPER_ACTIVE === '1') {
    process.stderr.write('native-c-compiler: CC resolves back to this wrapper; set CC to a real C compiler\n');
    process.exit(2);
  }
  const compiler = process.env.CC ?? 'cc';
  const result = spawnSync(compiler, [...process.argv.slice(2), ...nativeCFlags], {
    env: { ...nativeCEnvironment(), JET_NATIVE_C_WRAPPER_ACTIVE: '1' }, stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.signal) process.kill(process.pid, result.signal);
  else process.exitCode = result.status ?? 1;
}
