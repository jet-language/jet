#!/usr/bin/env node
// Build the checked Jet runtime as IR into one object, then resolve permitted
// C platform dependencies with Jet's own linker. Never invoke a Rust compiler.
import { nativeCCompiler, nativeCEnvironment, nativeCFlags } from './native-c-compiler.mjs';
import { createHash } from 'node:crypto';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { availableParallelism, tmpdir } from 'node:os';
import { nativePackRecipe, readNativeRuntimePack } from './runtime-pack-cache.mjs';

let repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const [compiler, destination, ...extra] = process.argv.slice(2);
const usage = 'prebuild-runtime.mjs <compiler> [runtime-dir] [--repo <exact-source-export>]';
if (!compiler) throw Error(usage);
for (let i = 0; i < extra.length; i += 2) {
  if (!extra[i + 1] || extra[i + 1].startsWith('--')) throw Error(usage);
  if (extra[i] === '--repo') repo = resolve(extra[i + 1]);
  else throw Error(usage);
}
const jet = resolve(compiler);
const out = resolve(destination ?? join(dirname(jet), 'jet-runtime'));
const musl = process.env.JET_PACK_MUSL_LIB;
if (!musl) throw Error('JET_PACK_MUSL_LIB must name the static musl libc.a directory');
const cc = nativeCCompiler;
const platform = [resolve(musl, 'libc.a'), ...['libgcc_eh.a', 'libgcc.a'].map(name => {
  const path = execFileSync(cc, [`-print-file-name=${name}`], { encoding: 'utf8' }).trim();
  if (!existsSync(path)) throw Error(`${cc} did not locate ${name}: ${path}`);
  return resolve(path);
})];
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
function inventory(files) {
  const hash = createHash('sha256');
  const rows = [...new Set(files)].sort().map(path => {
    const bytes = readFileSync(join(repo, path));
    hash.update(`${Buffer.byteLength(path)}:${path}:${bytes.length}:`).update(bytes);
    return { path, sha256: sha256(bytes) };
  });
  return { sha256: hash.digest('hex'), files: rows };
}
function jetFiles(directory) {
  return readdirSync(join(repo, directory), { withFileTypes: true }).flatMap(entry => {
    const path = `${directory}/${entry.name}`;
    return entry.isDirectory() ? jetFiles(path) : entry.name.endsWith('.jet') ? [path] : [];
  });
}
const manifest = 'Compiler/JetRuntime/sources.list';
const sources = readFileSync(join(repo, manifest), 'utf8').split('\n').map(row => row.trim()).filter(row => row && !row.startsWith('#'));
const source = inventory([manifest, 'Compiler/JetRuntime/package.jet', ...sources]);
// The runtime object's producer: the IR builder/optimizer/selector and the
// x86-64 encoder, object writer and linker it uses.
const backend = inventory([...jetFiles('Compiler/JetIR/Source'), ...jetFiles('Compiler/JetBackend/Source')]);
const lifecycle = inventory(['Compiler/JetRuntime/lifecycle.list']);
const platformInputs = platform.map(path => ({ path, sha256: sha256(readFileSync(path)), language: 'C', role: 'platform' }));
const compilerSha256 = sha256(readFileSync(jet));
// User foreign closures link the Jet-only runtime object with these exact
// platform archives once (Compiler/JetCli ForeignLink.jet); ship them beside it.
// Core's vendored C (Core/Vendor/*) backs Core modules' `c.sqlite3` and
// `c.mbedtls` imports. It is compiled here, once per runtime key, under the one
// native C policy, and shipped as <key>/foreign/<lib>/lib<lib>.a beside the
// pack, so neither jet's self-compile nor a core.db or core.net.tls program
// needs a C compiler; only C a package declares itself is compiled at link time.
const vendorBaseFlags = ['-std=c11', '-O2', '-fPIC', '-fno-stack-protector', '-fno-asynchronous-unwind-tables', '-fno-unwind-tables', '-D_GNU_SOURCE'];
const vendored = [
  // libsqlite3-sys 0.28.0 build.rs120-141,245-249 on Linux.
  { name: 'sqlite3', source: 'Core/Vendor/Sqlite3', flags: [
    '-DSQLITE_CORE', '-DSQLITE_DEFAULT_FOREIGN_KEYS=1',
    '-DSQLITE_ENABLE_API_ARMOR', '-DSQLITE_ENABLE_COLUMN_METADATA',
    '-DSQLITE_ENABLE_DBSTAT_VTAB', '-DSQLITE_ENABLE_FTS3',
    '-DSQLITE_ENABLE_FTS3_PARENTHESIS', '-DSQLITE_ENABLE_FTS5',
    '-DSQLITE_ENABLE_JSON1', '-DSQLITE_ENABLE_LOAD_EXTENSION=1',
    '-DSQLITE_ENABLE_MEMORY_MANAGEMENT', '-DSQLITE_ENABLE_RTREE',
    '-DSQLITE_ENABLE_STAT2', '-DSQLITE_ENABLE_STAT4', '-DSQLITE_SOUNDEX',
    '-DSQLITE_THREADSAFE=1', '-DSQLITE_USE_URI', '-DHAVE_USLEEP=1',
    '-D_POSIX_THREAD_SAFE_FUNCTIONS', '-DHAVE_ISNAN', '-DHAVE_LOCALTIME_R', '-w',
  ] },
  // The tree keeps its relative layout: mbedtls/ and psa/ headers sit under -I.
  { name: 'mbedtls', source: 'Core/Vendor/Mbedtls', flags: [] },
];
function vendorFiles(root, relative = '') {
  const rows = [];
  for (const entry of readdirSync(join(root, relative), { withFileTypes: true })) {
    const path = join(relative, entry.name);
    if (entry.isDirectory()) rows.push(...vendorFiles(root, path));
    else if (/\.[ch]$/u.test(entry.name) || entry.name === 'LICENSE' || entry.name === 'VERSION') rows.push(path);
  }
  return rows.sort();
}
// The C compiler and archiver the policy wrapper (native-c-compiler.mjs) runs.
const vendorCompiler = process.env.CC ?? 'cc';
const vendorArchiver = process.env.AR ?? 'ar';
const vendorEnvironment = nativeCEnvironment();
// Everything the archives are a function of: source bytes, flags, policy, the
// compiler and the wrapper environment a Nix cc adds to every compile.
const vendorRecipeSha256 = sha256(JSON.stringify({
  format: 'jet-vendored-c/v1',
  inputs: inventory(vendored.flatMap(({ source }) => vendorFiles(join(repo, source)).map(path => `${source}/${path}`))).sha256,
  base: vendorBaseFlags, policy: nativeCFlags, libraries: vendored.map(({ name, flags }) => ({ name, flags })),
  compiler: [execFileSync(vendorCompiler, ['--version'], { encoding: 'utf8', env: vendorEnvironment }),
    execFileSync(vendorCompiler, ['-print-prog-name=cc1'], { encoding: 'utf8', env: vendorEnvironment }).trim()],
  environment: ['NIX_CFLAGS_COMPILE', 'NIX_CFLAGS_COMPILE_BEFORE', 'NIX_HARDENING_ENABLE'].map(name => vendorEnvironment[name] ?? ''),
}));
const recipeSha256 = nativePackRecipe(source.sha256, backend.sha256, lifecycle.sha256,
  platformInputs.map(row => row.sha256), compilerSha256, vendorRecipeSha256);
function finished(child, what) {
  return new Promise((done, fail) => {
    let errors = '';
    child.stderr?.on('data', chunk => { if (errors.length < 1 << 16) errors += chunk; });
    child.on('error', fail);
    child.on('close', (status, signal) => done(status === 0 ? null : `${what} failed (${signal ?? status})${errors ? `:\n${errors}` : ''}`));
  });
}
// Compile every vendored unit (at most six compilers at once; the first ones,
// sqlite3's single large unit among them, overlap the runtime build) and
// archive each library deterministically in unit order. Every failing unit is
// reported.
async function buildVendored(work) {
  const jobs = [];
  const libraries = vendored.map(({ name, source, flags }) => {
    const root = join(repo, source);
    const objects = join(work, 'vendored', name);
    mkdirSync(objects, { recursive: true });
    const units = vendorFiles(root).filter(path => path.endsWith('.c')).map(unit => {
      const object = join(objects, unit.replaceAll('/', '_').replace(/\.c$/u, '.o'));
      jobs.push({ unit: `${source}/${unit}`, argv: [...vendorBaseFlags, ...flags, `-ffile-prefix-map=${root}=/jet/c`,
        `-ffile-prefix-map=${objects}=.`, '-I', root, '-c', join(root, unit), '-o', object, ...nativeCFlags] });
      return object;
    });
    return { name, source: root, objects, units, archive: join(objects, `lib${name}.a`) };
  });
  const failures = [];
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const job = jobs[next++];
      const child = spawn(vendorCompiler, job.argv, { env: vendorEnvironment, stdio: ['ignore', 'ignore', 'pipe'] });
      const failure = await finished(child, `C compiler on ${job.unit}`);
      if (failure) failures.push(failure);
    }
  };
  await Promise.all(Array.from({ length: Math.min(6, availableParallelism()) }, worker));
  if (failures.length) throw Error(`vendored C build failed:\n${failures.join('\n')}`);
  for (const library of libraries) {
    execFileSync(vendorArchiver, ['rcsD', library.archive, ...library.units]);
    noRust(library.archive);
  }
  return libraries;
}
function publishPlatform() {
  // Platform archives may come from a read-only store (mode 0444): replace the
  // published copy instead of writing through it, so a second publish succeeds.
  const platformDirectory = join(out, 'foreign', 'platform');
  mkdirSync(platformDirectory, { recursive: true });
  for (const path of platform) {
    const target = join(platformDirectory, basename(path));
    rmSync(target, { force: true });
    copyFileSync(path, target);
  }
}
if (existsSync(join(out, 'native.key'))) {
  try {
    const cached = readNativeRuntimePack(out, recipeSha256);
    noRust(join(out, cached.key, 'jet_runtime_jet.o'));
    noRust(join(out, cached.key, 'jet_runtime.pack.o'));
    noRust(join(out, cached.key, 'jet_runtime.image'));
    publishPlatform();
    console.log(join(out, cached.key, 'jet_runtime.pack.o'));
    process.exit(0);
  } catch (error) {
    console.error(`native runtime cache miss: ${error.message}`);
  }
}
function noRust(path) {
  if (/\.(?:rlib|rs)$/u.test(path)) throw Error(`Rust runtime input forbidden: ${path}`);
  const symbols = execFileSync('nm', ['--format=posix', path], { encoding: 'utf8', maxBuffer: 1 << 28 });
  const forbidden = symbols.split('\n').filter(row => /(?:^|\s)(?:_R[A-Z]|_ZN\d|__rust_|__rdl_)/u.test(row));
  if (forbidden.length) throw Error(`Rust symbols in ${path}:\n${forbidden.join('\n')}`);
}
for (const path of platform) noRust(path);
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), 'jet-runtime-pack-'));
const vendoredBuild = buildVendored(work);
try {
  const library = join(work, 'library');
  execFileSync(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), library, '--assemble-only'], { stdio: 'inherit' });
  const prefix = join(work, 'jet_runtime_jet');
  const pack = join(work, 'jet_runtime.pack.o');
  // The pack prelinked at the run base, which `jet run` binds programs to.
  const image = join(work, 'jet_runtime.image');
  const policy = {
    JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix,
    JET_FRONT_LIBRARY_LIFECYCLE: join(repo, 'Compiler/JetRuntime/lifecycle.list'),
    JET_FRONT_RUNTIME_PACK_OUTPUT: pack,
    JET_FRONT_RUNTIME_IMAGE_OUTPUT: image,
    JET_FRONT_RUNTIME_PLATFORM_INPUTS: platform.join('\n'),
  };
  // The runtime-library seam in the hosted source loader (Request.jet
  // jet_cli_compile_prepared) under `jet doc` (the checked program stopped
  // before lowering, then built as IR; under the seam `jet doc` builds no
  // index or pages). `doc --check` exits 1 on undocumented public items.
  const run = spawnSync(jet, ['doc', join(library, 'unit.jet')], {
    cwd: library, stdio: 'inherit',
    env: { ...process.env, JET_TOOLCHAIN_ROOT: repo, ...policy },
  });
  if (run.error) throw run.error;
  if (run.status !== 0) throw Error(`Jet runtime production failed (${run.status})`);
  const built = await vendoredBuild;
  for (const path of [prefix + '.o', pack, image]) noRust(path);
  const abiBytes = readFileSync(prefix + '.abi.json');
  const abi = JSON.parse(abiBytes);
  if (!Array.isArray(abi) || !abi.length) throw Error('runtime ABI inventory is empty');
  const exportNames = readFileSync(prefix + '.exports', 'utf8').trim().split('\n').sort();
  if (JSON.stringify(abi.map(row => row.symbol).sort()) !== JSON.stringify(exportNames)) throw Error('runtime ABI and checked exports disagree');
  const abiSha256 = sha256(abiBytes);
  const vendoredArchives = built.map(({ name, archive }) => ({ name, path: `foreign/${name}/lib${name}.a`, sha256: sha256(readFileSync(archive)) }));
  const identity = { format: 'jet-native-runtime/v1', source: source.sha256, abi: abiSha256, backend: backend.sha256, lifecycle: lifecycle.sha256, platform: platformInputs.map(row => row.sha256), vendored: vendoredArchives.map(row => row.sha256) };
  const key = sha256(JSON.stringify(identity));
  const directory = join(out, key);
  mkdirSync(directory, { recursive: true });
  copyFileSync(prefix + '.o', join(directory, 'jet_runtime_jet.o'));
  copyFileSync(prefix + '.abi.json', join(directory, 'runtime.abi.json'));
  copyFileSync(prefix + '.exports', join(directory, 'runtime.exports'));
  copyFileSync(pack, join(directory, 'jet_runtime.pack.o'));
  copyFileSync(image, join(directory, 'jet_runtime.image'));
  for (const library of built) {
    const target = join(directory, 'foreign', library.name);
    mkdirSync(target, { recursive: true });
    copyFileSync(library.archive, join(target, `lib${library.name}.a`));
    for (const notice of ['LICENSE', 'VERSION']) if (existsSync(join(library.source, notice))) copyFileSync(join(library.source, notice), join(target, notice));
  }
  const provenance = { backend: 'jet-native', runtimeKey: key, objectSha256: sha256(readFileSync(prefix + '.o')), packSha256: sha256(readFileSync(pack)), imageSha256: sha256(readFileSync(image)), rustObjects: 0, llvmObjects: 0, abiSha256, source, backendInputs: backend, lifecycle, cPlatformArchives: platformInputs, vendoredC: { recipeSha256: vendorRecipeSha256, archives: vendoredArchives }, compilerSha256, recipeSha256 };
  writeFileSync(join(directory, 'provenance.json'), JSON.stringify(provenance, null, 2) + '\n');
  writeFileSync(join(out, 'provenance.json'), JSON.stringify(provenance, null, 2) + '\n');
  publishPlatform();
  writeFileSync(join(out, 'native.key.new'), key + '\n');
  renameSync(join(out, 'native.key.new'), join(out, 'native.key'));
  console.log(join(directory, 'jet_runtime.pack.o'));
} catch (error) {
  // The vendored compilers write under `work`: let them finish before it goes,
  // and report their failures with the runtime build's.
  const vendoredProblem = await vendoredBuild.then(() => null, problem => problem);
  for (const suffix of ['.issues', '.pack-issues']) {
    const path = join(work, 'jet_runtime_jet' + suffix);
    if (existsSync(path)) writeFileSync(join(out, 'runtime' + suffix), readFileSync(path));
  }
  if (vendoredProblem && vendoredProblem !== error) throw Error(`${error.message}\n${vendoredProblem.message}`);
  throw error;
} finally {
  rmSync(work, { recursive: true, force: true });
}
