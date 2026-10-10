#!/usr/bin/env node
// Native Jet consumers link against musl: glibc's fortified entry points are
// not part of that platform ABI. Scope the policy to this C compiler child.
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const nativeCCompiler = fileURLToPath(import.meta.url);
export const nativeCFlags = Object.freeze(['-U_FORTIFY_SOURCE', '-D_FORTIFY_SOURCE=0']);
export function nativeCEnvironment(environment = process.env) {
  const child = { ...environment };
  // Nix's compiler wrapper adds fortify after caller flags; its setup-hook
  // NIX_HARDENING_DISABLE knob is not consulted by an already-built wrapper.
  if (child.NIX_HARDENING_ENABLE !== undefined) {
    child.NIX_HARDENING_ENABLE = child.NIX_HARDENING_ENABLE.split(/\s+/u)
      .filter(flag => flag && flag !== 'fortify' && flag !== 'fortify3').join(' ');
  }
  return child;
}

if (process.argv[1] && resolve(process.argv[1]) === nativeCCompiler) {
  const compiler = process.env.CC ?? 'cc';
  const result = spawnSync(compiler, [...process.argv.slice(2), ...nativeCFlags], {
    env: nativeCEnvironment(), stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.signal) process.kill(process.pid, result.signal);
  else process.exitCode = result.status ?? 1;
}
