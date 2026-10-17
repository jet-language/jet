import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { elf64, exactExportABI, exactExports, exportRows } from './runtime-shared-seal.mjs';
import { embeddedAdmission, installRelative } from './runtime-admission.mjs';

test('canonical export qualifiers and exact priorities', () => {
  assert.deepEqual(exportRows('# comment\njet_rt_alloc_init init 0\njet_rt_flush fini 65535\njet_at int-index-seam\njet_fn\n'), [
    { name: 'jet_rt_alloc_init', init: 0, fini: null },
    { name: 'jet_rt_flush', init: null, fini: 65535 },
    { name: 'jet_at', init: null, fini: null },
    { name: 'jet_fn', init: null, fini: null },
  ]);
  for (const text of ['', 'jet_fn\njet_fn', 'main', 'jet_fn init', 'jet_fn init 65536', 'jet_fn fini -1', 'jet_fn int-index-seam 2', 'jet_fn init 1 extra']) assert.throws(() => exportRows(text));
});

test('actual LIR ABI rows require exact sorted sealed exports and concrete layouts', () => {
  const word = { size: 8, align: 8, class: 'Integer' };
  const rows = [
    { symbol: 'jet_one', parameters: [], result: null },
    { symbol: 'jet_two', parameters: [word], result: word },
  ];
  const declared = exportRows('jet_two\njet_one\n').map(row => row.name);
  assert.deepEqual(declared, ['jet_two', 'jet_one']);
  assert.equal(exactExportABI(rows, declared), rows);
  for (const bad of [undefined, null, [], [...rows].reverse(), [rows[0], rows[0]], [...rows, rows[1]],
    [{ ...rows[0], extra: true }, rows[1]], [rows[0], { ...rows[1], parameters: null }],
    [rows[0], { ...rows[1], result: undefined }]]) assert.throws(() => exactExportABI(bad, ['jet_one', 'jet_two']));
  for (const bad of [{ ...word, size: 0 }, { ...word, align: 3 }, { ...word, class: 'Unknown' },
    { ...word, size: 1.5 }, { ...word, guessed: true }]) {
    assert.throws(() => exactExportABI([rows[0], { ...rows[1], parameters: [bad] }], ['jet_one', 'jet_two']));
  }
});


test('union seal rejects extra, weak, data and missing definitions', () => {
  const fn = name => ({ name, type: 2, bind: 1, section: 1 });
  exactExports([fn('jet_one'), { name: 'libc_import', type: 2, bind: 1, section: 0 }], ['jet_one']);
  assert.throws(() => exactExports([fn('jet_one'), fn('jet_extra')], ['jet_one']), /unexpected/);
  assert.throws(() => exactExports([{ ...fn('jet_one'), bind: 2 }], ['jet_one']), /missing strong/);
  assert.throws(() => exactExports([{ ...fn('jet_one'), type: 1 }], ['jet_one']), /missing strong/);
  assert.throws(() => exactExports([], ['jet_one']), /missing strong/);
});

test('ELF seal refuses malformed and wrong-machine inputs', () => {
  const dir = mkdtempSync(join(tmpdir(), 'jet-runtime-seal-'));
  try {
    const path = join(dir, 'bad.o');
    for (const bytes of [Buffer.alloc(0), Buffer.from('not ELF'), Buffer.alloc(64)]) {
      writeFileSync(path, bytes);
      assert.throws(() => elf64(path));
    }
    const bytes = Buffer.alloc(64);
    bytes.set([0x7f, 0x45, 0x4c, 0x46, 2, 1]);
    bytes.writeUInt16LE(183, 18);
    writeFileSync(path, bytes);
    assert.throws(() => elf64(path), /x86-64/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('private host constants bind actual receipt bytes, library hash and relative install path', () => {
  const receipt = Buffer.from('sealed receipt bytes\n');
  const libraryHash = 'ab'.repeat(32);
  const result = embeddedAdmission(receipt, libraryHash, 'jet-runtime/jet_runtime.so');
  assert.equal(result.receiptSHA256, createHash('sha256').update(receipt).digest('hex'));
  assert.match(result.source, /LIBRARY_INSTALL_RELATIVE: &str = "jet-runtime\/jet_runtime\.so"/);
  assert.match(result.source, /EXPECTED_LIBRARY_SHA256.*0xab, 0xab/);
  const bytes = result.source.match(/RECEIPT_BYTES: &\[u8\] = &\[\n([\s\d,]+)\n\];/);
  assert.ok(bytes);
  assert.deepEqual(Buffer.from(bytes[1].split(',').map(value => value.trim()).filter(Boolean).map(Number)), receipt);
  for (const path of ['', '/abs/runtime.so', '../runtime.so', 'x/../runtime.so', 'x//runtime.so', 'x\\runtime.so', '.']) assert.throws(() => installRelative(path));
});
