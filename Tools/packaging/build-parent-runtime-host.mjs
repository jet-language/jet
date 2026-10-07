#!/usr/bin/env node
// One private packaging pipeline: Jet runtime -> embedded admission -> host build
// -> install only that host and its matching .so. No runtime env/receipt sidecar.
// Usage: build-parent-runtime-host.mjs INPUTS OUT INSTALL_ROOT HOST_OUTPUT -- COMMAND [ARGS...]
// COMMAND is the existing host build command, run through the source's jet-env.
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { installRelative } from './runtime-admission.mjs';

const [inputs, output, installation, hostOutput, separator, ...build] = process.argv.slice(2);
if (!inputs || !output || !installation || !hostOutput || separator !== '--' || !build.length) throw Error('usage: build-parent-runtime-host.mjs INPUTS OUT INSTALL_ROOT HOST_OUTPUT -- COMMAND [ARGS...]');
const config = JSON.parse(readFileSync(inputs, 'utf8'));
const out = resolve(output), install = resolve(installation);
const relative = installRelative(config.libraryInstallRelative);
if (existsSync(install) || existsSync(install + '.candidate')) throw Error('refusing to overwrite an installation');
function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { cwd: config.source.root, env, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(`${command}: actual exit ${result.status ?? result.signal}`);
}
const producer = join(dirname(fileURLToPath(import.meta.url)), 'prebuild-runtime-shared.mjs');
run(process.execPath, [producer, resolve(inputs), out]);
run(join(config.source.root, 'Tools/agent/jet-env'), build, {
  ...process.env, JET_PARENT_RUNTIME_ADMISSION_FILE: join(out, 'runtime-admission.rs'),
});
const hostBytes = readFileSync(resolve(hostOutput));
const receiptBytes = readFileSync(join(out, 'runtime-shared.json'));
if (!hostBytes.includes(receiptBytes) || !hostBytes.includes(Buffer.from(relative))) throw Error('host output did not retain the actual embedded runtime receipt/path (no installation published)');
const staging = install + '.candidate';
mkdirSync(dirname(install), { recursive: true });
mkdirSync(join(staging, dirname(relative)), { recursive: true });
copyFileSync(resolve(hostOutput), join(staging, basename(hostOutput)));
copyFileSync(join(out, 'jet_runtime.so'), join(staging, relative));
renameSync(staging, install);
console.log(JSON.stringify({ host: join(install, basename(hostOutput)), library: join(install, relative), receipt: join(out, 'runtime-shared.json') }));
