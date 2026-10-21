#!/usr/bin/env node
// Compose all private Jet crypto KAT exports into ONE candidate native object.
// C files transport fixtures only. Explicit phases follow the runtime witness
// harness convention; no fixture can fall back to a Rust crypto implementation.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-crypto-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), unit = join(out, 'unit');
const fixtures = ['Sha256', 'CryptoX25519', 'CryptoEd25519', 'CryptoChaCha', 'CryptoAead', 'CryptoHkdf', 'CryptoArgon', 'AgeCodec', 'AgeV1', 'CryptoEntropy', 'CryptoHash', 'CryptoPbkdf2', 'CryptoSha3', 'CryptoBlake3', 'CryptoEd25519Signature', 'CryptoZeroize', 'CryptoOsEntropy'];
function command(name, binary, args, options = {}) {
    const result = spawnSync(binary, args, {cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options});
    writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify({status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr}, null, 2) + '\n');
    if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
    return result;
}
if (mode === 'assemble') {
    mkdirSync(join(unit, 'Source'), {recursive: true});
    const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
    paths.push(...fixtures.map(name => `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`));
    const imports = new Set();
    const bodies = paths.map(path => {
        let importing = false;
        const lines = readFileSync(join(root, path), 'utf8').split('\n').filter(line => {
            if (line.startsWith('use ')) {
                if (line.startsWith('use core.')) imports.add(line);
                importing = line.includes('[') && !line.includes(']');
                return false;
            }
            if (importing) { if (line.includes(']')) importing = false; return false; }
            return true;
        });
        return `// [unit source: ${path}]\n${lines.join('\n')}`;
    });
    writeFileSync(join(unit, 'Source/Fixture.jet'), [...imports, '', ...bodies].join('\n'));
    writeFileSync(join(unit, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
    const entries = readFileSync(join(root, 'Compiler/JetRuntime/exports.list'), 'utf8').split('\n').filter(x => x && !x.startsWith('#'));
    for (const name of fixtures) for (const match of readFileSync(join(root, `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`), 'utf8').matchAll(/#Export\(c\)\s+fn\s+(\w+)\(/gu)) entries.push(match[1]);
    writeFileSync(join(unit, 'exports.list'), entries.sort().join('\n') + '\n');
    writeFileSync(join(out, 'sources.receipt.json'), JSON.stringify({paths}, null, 2) + '\n');
} else if (mode === 'lower') {
    const prefix = join(unit, 'fixture');
    const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(unit, 'Source/Fixture.jet'), prefix, exports: join(unit, 'exports.list'), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
    if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
    const object = join(unit, 'fixture.o');
    const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
    const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
    for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/u)[0])) throw Error(`unledgered import ${line}`);
    const definitions = command('definitions', 'nm', ['--defined-only', '--format=posix', object]).stdout;
    const strong = new Set(definitions.trim().split('\n').filter(Boolean).filter(line => line.split(/\s+/u)[1] === 'T').map(line => line.split(/\s+/u)[0]));
    for (const name of fixtures) for (const match of readFileSync(join(root, `Compiler/JetRuntime/Tests/Fragments.fixture/${name}Tests.jet`), 'utf8').matchAll(/#Export\(c\)\s+fn\s+(\w+)\(/gu)) if (!strong.has(match[1])) throw Error(`missing strong Jet fixture export ${match[1]}`);
    command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'link') {
    for (const name of fixtures) command(`link-${name}`, nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', join(root, `Compiler/JetRuntime/Tests/${name}Witness.c`), join(unit, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, name)]);
} else {
    const digest = bytes => createHash('sha256').update(bytes).digest('hex');
    const fixtureFile = join(root, 'Compiler/JetRuntime/Tests/AgeV1Fixtures.json');
    const frozen = JSON.parse(readFileSync(fixtureFile, 'utf8'));
    const material = join(out, 'age-material');
    mkdirSync(material, {recursive: true});
    const captures = join(out, 'jet-seals');
    mkdirSync(captures, {recursive: true});
    for (const name of fixtures.filter(name => name !== 'AgeV1')) {
        const result = command(`run-${name}`, join(out, name), [], {timeout: name === 'CryptoArgon' || name === 'CryptoPbkdf2' || name === 'CryptoEd25519Signature' ? 300000 : 60000});
        if (name === 'CryptoEntropy' || name === 'CryptoHash' || name === 'CryptoPbkdf2' || name === 'CryptoSha3' || name === 'CryptoBlake3' || name === 'CryptoEd25519Signature' || name === 'CryptoZeroize' || name === 'CryptoOsEntropy') {
            const expected = readFileSync(join(root, `Compiler/JetRuntime/Tests/${name}Witness.stdout`), 'utf8');
            const metadata = JSON.parse(readFileSync(join(root, `Compiler/JetRuntime/Tests/${name}Fixtures.json`), 'utf8'));
            if (digest(expected) !== metadata.stdoutSha256) throw Error(`damaged frozen ${name} reference`);
            if (result.stdout !== expected) throw Error(`Jet ${name} contract differs byte-for-byte from the frozen reference`);
        }
        if (name === 'CryptoOsEntropy') {
            const metadata = JSON.parse(readFileSync(join(root, 'Compiler/JetRuntime/Tests/CryptoOsEntropyFixtures.json'), 'utf8'));
            const expected = readFileSync(join(root, 'Compiler/JetRuntime/Tests/CryptoOsEntropyWorld.stderr'), 'utf8');
            if (digest(expected) !== metadata.world.stderrSha256) throw Error('damaged frozen World rejection reference');
            for (const count of ['0', '-1', '1048577', '7']) {
                // This is an expected terminal stop, not command()'s success path.
                const stopped = spawnSync(join(out, name), ['world', count], {cwd: root, encoding: 'utf8', timeout: 60000});
                writeFileSync(join(out, `run-${name}-world-${count}.receipt.json`), JSON.stringify({status: stopped.status, signal: stopped.signal, error: stopped.error?.message ?? null, stdout: stopped.stdout, stderr: stopped.stderr}, null, 2) + '\n');
                if (stopped.error || stopped.status !== metadata.world.exitCode || stopped.signal || stopped.stdout !== '' || stopped.stderr !== expected) throw Error(`World entropy stop differs count=${count}`);
            }
        }
    }
    for (const test of frozen.cases) {
        const cipher = Buffer.from(test.cipher, 'base64');
        if (digest(cipher) !== test.cipherSha256) throw Error(`damaged frozen vector ${test.name}`);
        const path = join(material, `${test.name}.age`);
        writeFileSync(path, cipher);
        command(`run-${test.name}`, join(out, 'AgeV1'), ['open', test.identity, path, test.valid ? '1' : '0', test.plainSha256 ?? '-'], {timeout: 60000});
    }
    for (const test of frozen.seals) {
        const cipher = Buffer.from(test.cipher, 'base64');
        if (digest(cipher) !== test.cipherSha256) throw Error(`damaged frozen seal ${test.name}`);
        const plainPath = join(material, `${test.name}.plain`), cipherPath = join(material, `${test.name}.age`), capturePath = join(captures, `${test.name}.age`);
        writeFileSync(plainPath, Buffer.from(test.plain, 'base64'));
        writeFileSync(cipherPath, cipher);
        command(`run-${test.name}`, join(out, 'AgeV1'), ['seal', test.identity, plainPath, cipherPath, String(test.recipientCount), capturePath], {timeout: 60000});
        if (!readFileSync(capturePath).equals(cipher)) throw Error(`Jet capture differs ${test.name}`);
    }
    console.log(`CRYPTO_NATIVE_KAT_OK primitive_witnesses=${fixtures.length - 1} age_open=${frozen.cases.length} age_seal=${frozen.seals.length} fixtures_sha256=${digest(readFileSync(fixtureFile))}`);
    console.log(`ONE_TIME_RUST_OPENER_OWED captures=${captures}`);
}
