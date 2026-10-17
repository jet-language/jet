// Exact ELF/export/constructor checks for the parent-process Jet runtime.
import { readFileSync } from 'node:fs';

export function exportRows(text) {
  const rows = text.split('\n').map(line => line.replace(/#.*/, '').trim()).filter(Boolean).map(line => {
    const [name, marker, priority, ...extra] = line.split(/\s+/);
    const seam = marker === 'int-index-seam' && priority === undefined;
    const lifecycle = (marker === 'init' || marker === 'fini') && /^\d+$/.test(priority ?? '') && Number(priority) <= 65535;
    if (!/^jet_[A-Za-z0-9_]+$/.test(name) || extra.length || (marker !== undefined && !seam && !lifecycle)) {
      throw Error(`invalid runtime export row: ${line}`);
    }
    return { name, init: marker === 'init' ? Number(priority) : null, fini: marker === 'fini' ? Number(priority) : null };
  });
  if (!rows.length || new Set(rows.map(row => row.name)).size !== rows.length) throw Error('empty or duplicate runtime exports');
  return rows;
}
// The library hook writes these rows from the final callable's LIRSignature.
// Require the exact sealed object inventory; never reconstruct ABI from names.
export function exactExportABI(rows, names) {
  if (!Array.isArray(rows) || rows.length !== names.length) throw Error('runtime export ABI inventory mismatch');
  // exportRows admits ASCII C symbols, so default JS and Jet String order agree.
  const expected = [...names].sort();
  const layout = value => {
    if (!value || Object.keys(value).sort().join(',') !== 'align,class,size'
      || !Number.isSafeInteger(value.size) || value.size <= 0
      || !Number.isSafeInteger(value.align) || value.align <= 0
      || (BigInt(value.align) & (BigInt(value.align) - 1n)) !== 0n
      || !['Integer', 'Pointer', 'Float', 'Memory'].includes(value.class)) throw Error('invalid runtime export ABI layout');
  };
  rows.forEach((row, index) => {
    if (!row || Object.keys(row).sort().join(',') !== 'parameters,result,symbol'
      || row.symbol !== expected[index] || !Array.isArray(row.parameters)) throw Error('runtime export ABI inventory mismatch');
    row.parameters.forEach(layout);
    if (row.result !== null) layout(row.result);
  });
  return rows;
}



export function exactExports(actual, expected) {
  const errors = [];
  const names = new Set(expected);
  for (const name of names) if (!actual.some(symbol => symbol.name === name && symbol.type === 2 && symbol.bind === 1 && symbol.section !== 0)) errors.push(`missing strong function export ${name}`);
  for (const symbol of actual) if (symbol.bind !== 0 && symbol.section !== 0 && symbol.name && !names.has(symbol.name)) errors.push(`unexpected export ${symbol.name}`);
  if (errors.length) throw Error(errors.join('\n'));
}

// Read the emitted x86-64 ELF directly: no locale-dependent readelf parsing.
export function elf64(path) {
  const bytes = readFileSync(path);
  const fail = message => { throw Error(`${path}: ${message}`); };
  const span = (offset, size) => {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(size) || offset < 0 || size < 0 || offset > bytes.length - size) fail('ELF field outside file');
    return bytes.subarray(offset, offset + size);
  };
  const u16 = offset => span(offset, 2).readUInt16LE();
  const u32 = offset => span(offset, 4).readUInt32LE();
  const u64 = offset => {
    const value = span(offset, 8).readBigUInt64LE();
    if (value > BigInt(Number.MAX_SAFE_INTEGER)) fail('ELF offset exceeds safe integer range');
    return Number(value);
  };
  const string = (table, offset) => {
    if (offset < 0 || offset >= table.length) fail('string outside ELF table');
    const end = table.indexOf(0, offset);
    if (end < 0) fail('unterminated ELF string');
    return table.toString('utf8', offset, end);
  };
  if (!span(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) || bytes[4] !== 2 || bytes[5] !== 1 || u16(18) !== 62) fail('not little-endian ELF64 x86-64');
  const programs = [];
  const programOffset = u64(32), programSize = u16(54), programCount = u16(56);
  if (programCount) {
    if (programSize !== 56) fail('unsupported program table');
    span(programOffset, programSize * programCount);
    for (let index = 0; index < programCount; index++) {
      const at = programOffset + index * programSize;
      programs.push({ type: u32(at), flags: u32(at + 4) });
    }
  }
  const sectionOffset = u64(40), sectionSize = u16(58), count = u16(60), nameIndex = u16(62);
  if (sectionSize !== 64 || !count || nameIndex >= count) fail('unsupported section table');
  span(sectionOffset, sectionSize * count);
  const sections = Array.from({ length: count }, (_, index) => {
    const at = sectionOffset + index * sectionSize;
    return { index, nameOffset: u32(at), type: u32(at + 4), flags: u64(at + 8), address: u64(at + 16), offset: u64(at + 24), size: u64(at + 32), link: u32(at + 40), info: u32(at + 44), entrySize: u64(at + 56) };
  });
  const sectionBytes = section => span(section.offset, section.size);
  const names = sectionBytes(sections[nameIndex]);
  for (const section of sections) section.name = string(names, section.nameOffset);
  function symbols(section) {
    if (section.entrySize !== 24 || section.size % 24 || section.link >= count) fail('invalid symbol table');
    const names = sectionBytes(sections[section.link]);
    return Array.from({ length: section.size / 24 }, (_, index) => {
      const at = section.offset + index * 24;
      return { name: string(names, u32(at)), type: bytes[at + 4] & 15, bind: bytes[at + 4] >> 4, section: u16(at + 6), value: u64(at + 8) };
    });
  }
  function relocations(section) {
    if (section.entrySize !== 24 || section.size % 24 || section.link >= count) fail('invalid relocation table');
    const table = symbols(sections[section.link]);
    return Array.from({ length: section.size / 24 }, (_, index) => {
      const at = section.offset + index * 24;
      const info = span(at + 8, 8).readBigUInt64LE();
      const symbol = table[Number(info >> 32n)];
      if (!symbol) fail('relocation symbol outside table');
      return { offset: u64(at), kind: Number(info & 0xffffffffn), symbol, addend: span(at + 16, 8).readBigInt64LE() };
    });
  }
  return { type: u16(16), entry: u64(24), programs, sections, symbols, relocations, u64, sectionBytes, string };
}

export function sealObject(path, rows) {
  const elf = elf64(path);
  if (elf.type !== 1 || elf.entry !== 0) throw Error('runtime object is not entryless ET_REL');
  const table = elf.sections.find(section => section.type === 2);
  if (!table) throw Error('runtime object has no symbol table');
  const symbols = elf.symbols(table);
  exactExports(symbols, rows.map(row => row.name));
  const lifecycle = {};
  for (const [kind, type] of [['init', 14], ['fini', 15]]) {
    const actual = [];
    for (const section of elf.sections.filter(section => section.type === type)) {
      const phase = section.name.match(new RegExp(`^\\.${kind}_array\\.(\\d{5})$`));
      if (!phase || section.size !== 8) throw Error(`invalid lifecycle section ${section.name}`);
      const relocation = elf.sections.find(candidate => candidate.type === 4 && candidate.info === section.index);
      const relocs = relocation ? elf.relocations(relocation) : [];
      if (relocs.length !== 1 || relocs[0].offset !== 0 || relocs[0].kind !== 1 || relocs[0].addend !== 0n) throw Error(`invalid lifecycle relocation ${section.name}`);
      actual.push({ name: relocs[0].symbol.name, priority: Number(phase[1]) });
    }
    const expected = rows.filter(row => row[kind] !== null).map(row => ({ name: row.name, priority: row[kind] }));
    const canonical = entries => entries.map(row => `${row.name}:${row.priority}`).sort();
    if (JSON.stringify(canonical(actual)) !== JSON.stringify(canonical(expected))) throw Error(`runtime object LINK_${kind.toUpperCase()} inventory differs from exports.list`);
    // Main's canonical writer sorts equal-priority lifecycle entries by symbol.
    lifecycle[kind] = actual.sort((a, b) => a.priority - b.priority || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  }
  return { lifecycle, imports: symbols.filter(symbol => symbol.bind !== 0 && symbol.section === 0 && symbol.name).map(symbol => symbol.name) };
}

export function sealShared(path, rows, lifecycle) {
  const elf = elf64(path);
  if (elf.type !== 3 || elf.entry !== 0) throw Error('runtime library is not entryless ET_DYN');
  if (!elf.programs.some(program => program.type === 1) || !elf.programs.some(program => program.type === 0x6474e552)) throw Error('runtime library lacks load segments or GNU_RELRO');
  for (const program of elf.programs) {
    if (program.type === 1 && (program.flags & 3) === 3) throw Error('writable executable runtime segment');
    if (program.type === 0x6474e551 && (program.flags & 1)) throw Error('executable runtime stack');
  }
  const table = elf.sections.find(section => section.type === 11);
  if (!table) throw Error('runtime library has no dynamic symbol table');
  const symbols = elf.symbols(table);
  exactExports(symbols, rows.map(row => row.name));
  const imports = symbols.filter(symbol => symbol.bind !== 0 && symbol.section === 0 && symbol.name).map(symbol => symbol.name).sort();
  const neededOffsets = [];
  let dynamicStrings = null;
  for (const section of elf.sections) {
    if ((section.flags & 5) === 5) throw Error(`writable executable section ${section.name}`);
    if (section.type === 6) {
      const dynamic = elf.sectionBytes(section);
      if (dynamic.length % 16) throw Error('invalid runtime dynamic table');
      for (let at = 0; at < dynamic.length; at += 16) {
        const tag = dynamic.readBigInt64LE(at), value = dynamic.readBigUInt64LE(at + 8);
        if (tag === 22n || (tag === 30n && (value & 4n))) throw Error('runtime library contains text relocations');
        if (tag === 1n) neededOffsets.push(Number(value));
        if (tag === 5n) dynamicStrings = Number(value);
      }
    }
  }
  const stringTable = elf.sections.find(section => section.type === 3 && section.address === dynamicStrings);
  if (neededOffsets.length && !stringTable) throw Error('runtime DT_NEEDED lacks DT_STRTAB');
  const needed = neededOffsets.map(offset => elf.string(elf.sectionBytes(stringTable), offset));
  const relocs = elf.sections.filter(section => section.type === 4).flatMap(section => elf.relocations(section));
  for (const [kind, type] of [['init', 14], ['fini', 15]]) {
    const entries = lifecycle[kind];
    const arrays = elf.sections.filter(section => section.type === type);
    if (!entries.length && !arrays.length) continue;
    if (arrays.length !== 1 || arrays[0].size !== entries.length * 8) throw Error(`shared library lost or added LINK_${kind.toUpperCase()} entries`);
    for (const [index, entry] of entries.entries()) {
      const address = arrays[0].address + index * 8;
      const reloc = relocs.find(row => row.offset === address);
      const target = reloc?.kind === 8 ? Number(reloc.addend) : reloc?.kind === 1 ? reloc.symbol.value + Number(reloc.addend) : elf.u64(arrays[0].offset + index * 8);
      if (target !== symbols.find(symbol => symbol.name === entry.name)?.value) throw Error(`shared ${kind} entry ${index} is not ${entry.name}`);
    }
  }
  return { exports: rows.map(row => row.name).sort(), imports, needed, lifecycle };
}
