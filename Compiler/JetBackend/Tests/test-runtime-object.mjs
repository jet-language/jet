#!/usr/bin/env node
// Assemble actual current writer witnesses; build/run unit.jet with the admitted
// Jet CLI and pack, then independently inspect its ELF output with --verify.
// usage: JET_RUNTIME_PACK=<pack> node test-runtime-object.mjs <outdir>
//        node test-runtime-object.mjs <outdir> --verify
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || (mode && mode !== '--verify') || process.argv.length > 4) {
  throw new Error('usage: test-runtime-object.mjs <outdir> [--verify]');
}
const out = resolve(output);
const baseNames = ['', '.text', '.data', '.rela.text', '.symtab', '.strtab', '.shstrtab',
  '.note.GNU-stack', '.rela.data', '.eh_frame', '.rela.eh_frame', '.debug_abbrev',
  '.debug_info', '.rela.debug_info', '.debug_line', '.rela.debug_line', '.debug_loclists'];

function readObject(path, lifecycle) {
  const b = readFileSync(path);
  assert.deepEqual([...b.subarray(0, 7)], [0x7f, 69, 76, 70, 2, 1, 1], path);
  assert.equal(b.readUInt16LE(16), 1, 'ET_REL');
  assert.equal(b.readUInt16LE(18), 62, 'EM_X86_64');
  const number = at => {
    const n = b.readBigUInt64LE(at);
    assert(n <= BigInt(Number.MAX_SAFE_INTEGER), 'ELF integer is exact');
    return Number(n);
  };
  const shoff = number(40);
  const count = b.readUInt16LE(60);
  assert.equal(b.readUInt16LE(58), 64, 'section header width');
  assert.equal(count, baseNames.length + (lifecycle ? 8 : 0), 'all section rows emitted');
  assert.equal(shoff + 64 * count, b.length, 'one complete section table, no legacy tail');
  const sections = Array.from({ length: count }, (_, index) => {
    const at = shoff + 64 * index;
    const row = { index, nameAt: b.readUInt32LE(at), kind: b.readUInt32LE(at + 4),
      flags: number(at + 8), offset: number(at + 24), size: number(at + 32),
      link: b.readUInt32LE(at + 40), info: b.readUInt32LE(at + 44),
      align: number(at + 48), stride: number(at + 56) };
    assert(row.offset + row.size <= shoff, `section ${index} content precedes headers`);
    if (row.align) assert.equal(row.offset % row.align, 0, `section ${index} alignment`);
    row.bytes = b.subarray(row.offset, row.offset + row.size);
    return row;
  });
  const stringAt = (table, at) => {
    assert(at >= 0 && at < table.length, 'string offset in bounds');
    const end = table.indexOf(0, at);
    assert(end >= at, 'terminated ELF name');
    return table.subarray(at, end).toString('utf8');
  };
  const names = sections[b.readUInt16LE(62)].bytes;
  for (const row of sections) row.name = stringAt(names, row.nameAt);
  assert.deepEqual(sections.slice(0, baseNames.length).map(row => row.name), baseNames);
  const symtab = sections[4];
  assert.equal(symtab.kind, 2);
  assert.equal(symtab.stride, 24);
  assert.equal(symtab.size % 24, 0);
  assert.equal(symtab.link, 5);
  const symbols = Array.from({ length: symtab.size / 24 }, (_, index) => {
    const at = index * 24;
    return { name: stringAt(sections[5].bytes, symtab.bytes.readUInt32LE(at)),
      bind: symtab.bytes[at + 4] >> 4, section: symtab.bytes.readUInt16LE(at + 6) };
  });
  for (const [index, symbol] of symbols.entries()) {
    assert.equal(symbol.bind, index < symtab.info ? 0 : 1, 'locals precede globals');
    assert(symbol.section < count, 'symbol names an emitted section');
  }
  const relocations = row => {
    assert.equal(row.kind, 4);
    assert.equal(row.link, 4);
    assert.equal(row.stride, 24);
    assert.equal(row.size % 24, 0);
    assert(row.info > 0 && row.info < count, 'relocation target section exists');
    return Array.from({ length: row.size / 24 }, (_, index) => {
      const at = index * 24;
      const symbol = row.bytes.readUInt32LE(at + 12);
      assert(symbol < symbols.length, 'relocation symbol exists');
      return { at: row.bytes.readBigUInt64LE(at), kind: row.bytes.readUInt32LE(at + 8),
        symbol, addend: row.bytes.readBigInt64LE(at + 16) };
    });
  };
  for (const row of sections.filter(row => row.kind === 4)) {
    const target = sections[row.info];
    assert.equal(row.name, `.rela${target.name}`, 'RELA names its actual target');
    for (const reloc of relocations(row)) assert(reloc.at < BigInt(target.size), 'relocation offset in target');
  }
  for (const name of ['.eh_frame', '.debug_abbrev', '.debug_info', '.debug_line']) {
    assert(sections.find(row => row.name === name).size > 0, `${name} is emitted`);
  }
  assert.equal(sections[7].flags, 0, 'non-executable stack');
  if (lifecycle) {
    const globals = symbols.slice(symtab.info, symtab.info + 2).map(symbol => symbol.name);
    assert.deepEqual(globals, ['beta', 'alpha'], 'export declarations stay intact');
    const expected = [['alpha', '.init_array.00002', 14], ['alpha', '.fini_array.00007', 15],
      ['beta', '.init_array.00002', 14], ['beta', '.fini_array.00007', 15]];
    expected.forEach(([symbol, name, kind], index) => {
      const row = sections[baseNames.length + 2 * index];
      assert.equal(row.name, name);
      assert.equal(row.kind, kind);
      assert.equal(row.flags, 3);
      assert.equal(row.stride, 8);
      assert.deepEqual([...row.bytes], Array(8).fill(0));
      const rela = sections[row.index + 1];
      assert.equal(rela.info, row.index);
      const [reloc] = relocations(rela);
      assert.equal(rela.size, 24);
      assert.equal(reloc.at, 0n);
      assert.equal(reloc.kind, 1);
      assert.equal(reloc.addend, 0n);
      assert.equal(symbols[reloc.symbol].name, symbol, 'lexical lifecycle ordering');
    });
    const [dataReloc] = relocations(sections[8]);
    assert.equal(sections[8].size, 24);
    assert.equal(dataReloc.kind, 1);
    assert.equal(dataReloc.addend, 3n);
    assert.equal(symbols[dataReloc.symbol].section, 2);
  }
}

if (mode === '--verify') {
  for (const plain of ['false', 'true']) readObject(join(out, `program-${plain}.o`), false);
  readObject(join(out, 'runtime-lifecycle.o'), true);
  console.log('current program/runtime ELF sections: pass');
} else {
  execFileSync(process.execPath, [join(repo, 'Compiler/JetBackend/Tests/run-lower-fixtures.mjs'), out, '--abi'], { stdio: 'inherit' });
  const unitPath = join(out, 'unit.jet');
  const unit = readFileSync(unitPath, 'utf8');
  const entry = 'fn run() { abi_fixtures() }';
  if (!unit.includes(entry)) throw new Error('ABI fixture entry changed; update the object regression adapter');
  const witness = `fn run() {
    abi_fixtures()
    plains :: [Bool]{false, true}
    loop plain in plains {
        program := abi_alignment_module(true)
        if plain {
            program.data = [LIRData]{}
            program.functions[0].values = [LIRValue]{}
            program.functions[0].blocks[0].insts = [LIRInst]{}
            program.functions[0].blocks[0].lines = [Int]{0}
        }
        current :: x64_relocatable_object(program, LIRFunctionID{value: 0})
        assert(current.errors.len() == 0)
        files.write_bytes("program-{plain}.o", current.bytes) ?? panic("cannot write program object")
    }
    program := abi_alignment_module(true)
    program.data = [LIRData]{LIRData{id: LIRDataID{value: 0}, align: 8,
        bytes: [U8]{U8{0}, U8{0}, U8{0}, U8{0}, U8{0}, U8{0}, U8{0}, U8{0}},
        relocs: [LIRDataReloc]{LIRDataReloc{offset: 0, target: LIRDataID{value: 0}, addend: 3}}}}
    compiled :: x64_compile_module(program)
    assert(compiled.errors.len() == 0)
    exports :: [X64RuntimeExport]{
        X64RuntimeExport{function: LIRFunctionID{value: 0}, symbol: "beta", init_priority: 2, fini_priority: 7},
        X64RuntimeExport{function: LIRFunctionID{value: 0}, symbol: "alpha", init_priority: 2, fini_priority: 7},
    }
    current :: x64_object_functions(program, compiled.functions, compiled.debug, exports)
    assert(current.errors.len() == 0)
    files.write_bytes("runtime-lifecycle.o", current.bytes) ?? panic("cannot write runtime object")
}
`;
  writeFileSync(unitPath, unit.replace(entry, witness));
  console.log(unitPath);
}
