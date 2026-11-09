#!/usr/bin/env node
// The runtime's C census (#6693): which members of the C platform archives
// (musl's libc.a, libgcc.a, libgcc_eh.a, or any other) a static link of the
// Jet runtime object pulls in, and the reference that pulls each one. It
// resolves the way `ld -static` does over one archive group: the objects'
// strong undefined references pull the first archive member that defines
// them, repeated until nothing changes. Weak references pull nothing.
//
//   node Tools/agent/runtime-c-census.mjs [options] <object>... -- <archive>...
//
//   --start          also reference musl's `__libc_start_main`, as a program
//                    the linker starts through libc does (StaticLink.jet
//                    link_startup); without it the census is the owned
//                    startup's (link_owned_startup), which references none.
//   --root=a,b       also reference these names (a pack's extra roots).
//   --provided=a,b   treat these names as defined outside the archives, as a
//                    Jet port of them would be.
//   --expect-none    exit 1 when any archive member is pulled in.
//
// The unresolved names it prints include those the linker itself defines
// (StaticLink.jet link_linker_names, such as `_GLOBAL_OFFSET_TABLE_`).
//
// Example, over a built runtime (JetCli's .jet/build/jet-runtime) and the
// musl sysroot and pack `jet env fetch-toolchain --json` names:
//   node Tools/agent/runtime-c-census.mjs R/jet_runtime_jet.o -- \
//       S/usr/lib/libc.a P/lib/clang/21/lib/x86_64-unknown-linux-musl/libclang_rt.builtins.a
import { readFileSync } from 'node:fs';
import { basename } from 'node:path';

const SHT_SYMTAB = 2;
const SHN_UNDEF = 0;
const STB_LOCAL = 0;
const STB_WEAK = 2;

// The global symbols of one ELF64 relocatable object: what it defines and
// what it references strongly.
function elfSymbols(bytes, label) {
  if (bytes.length < 64 || bytes.readUInt32BE(0) !== 0x7f454c46 || bytes[4] !== 2) throw new Error(`${label}: not an ELF64 object`);
  const shoff = Number(bytes.readBigUInt64LE(0x28));
  const shentsize = bytes.readUInt16LE(0x3a);
  const shnum = bytes.readUInt16LE(0x3c);
  const section = (i) => {
    const at = shoff + i * shentsize;
    return { type: bytes.readUInt32LE(at + 4), offset: Number(bytes.readBigUInt64LE(at + 0x18)), size: Number(bytes.readBigUInt64LE(at + 0x20)), link: bytes.readUInt32LE(at + 0x28), entsize: Number(bytes.readBigUInt64LE(at + 0x38)) };
  };
  const defines = new Set();
  const references = new Set();
  for (let i = 0; i < shnum; i++) {
    const table = section(i);
    if (table.type !== SHT_SYMTAB) continue;
    const strings = section(table.link);
    for (let at = table.offset + table.entsize; at < table.offset + table.size; at += table.entsize) {
      const nameAt = bytes.readUInt32LE(at);
      const bind = bytes[at + 4] >> 4;
      const shndx = bytes.readUInt16LE(at + 6);
      if (nameAt === 0 || bind === STB_LOCAL) continue;
      let end = strings.offset + nameAt;
      while (bytes[end] !== 0) end++;
      const name = bytes.toString('latin1', strings.offset + nameAt, end);
      if (shndx !== SHN_UNDEF) defines.add(name);
      else if (bind !== STB_WEAK) references.add(name);
    }
  }
  return { defines, references };
}

// The ELF members of a System V / GNU ar archive, long names resolved.
function archiveMembers(bytes, label) {
  if (bytes.toString('latin1', 0, 8) !== '!<arch>\n') throw new Error(`${label}: not an ar archive`);
  const members = [];
  let longNames = '';
  let at = 8;
  while (at + 60 <= bytes.length) {
    const rawName = bytes.toString('latin1', at, at + 16).trimEnd();
    const size = Number.parseInt(bytes.toString('latin1', at + 48, at + 58).trim(), 10);
    const body = bytes.subarray(at + 60, at + 60 + size);
    at += 60 + size + (size % 2);
    if (rawName === '/' || rawName === '/SYM64/') continue;
    if (rawName === '//') {
      longNames = body.toString('latin1');
      continue;
    }
    let name = rawName;
    if (/^\/\d+$/.test(rawName)) {
      const start = Number.parseInt(rawName.slice(1), 10);
      name = longNames.slice(start, longNames.indexOf('\n', start));
    }
    if (name.endsWith('/')) name = name.slice(0, -1);
    if (body.length < 4 || body.readUInt32BE(0) !== 0x7f454c46) continue;
    members.push({ name: `${label}(${name})`, ...elfSymbols(body, `${label}(${name})`) });
  }
  return members;
}

function main(argv) {
  const split = argv.indexOf('--');
  if (split < 0) throw new Error('usage: runtime-c-census.mjs [--start] [--root=a,b] [--provided=a,b] [--expect-none] <object>... -- <archive>...');
  const options = argv.slice(0, split).filter((arg) => arg.startsWith('--'));
  const objects = argv.slice(0, split).filter((arg) => !arg.startsWith('--'));
  const archives = argv.slice(split + 1);
  const list = (flag) => options.filter((arg) => arg.startsWith(`${flag}=`)).flatMap((arg) => arg.slice(flag.length + 1).split(',')).filter(Boolean);
  for (const option of options) {
    if (!['--start', '--expect-none'].includes(option) && !option.startsWith('--root=') && !option.startsWith('--provided=')) throw new Error(`unknown option ${option}`);
  }
  if (objects.length === 0) throw new Error('no object to census');

  const defined = new Set(list('--provided'));
  const wanted = new Map();
  const want = (name, from) => {
    if (!defined.has(name) && !wanted.has(name)) wanted.set(name, from);
  };
  const objectReferences = [];
  for (const path of objects) {
    const symbols = elfSymbols(readFileSync(path), basename(path));
    for (const name of symbols.defines) defined.add(name);
    objectReferences.push([basename(path), symbols.references]);
  }
  for (const [label, references] of objectReferences) for (const name of references) want(name, label);
  if (options.includes('--start')) want('__libc_start_main', '<startup>');
  for (const name of list('--root')) want(name, '<root>');
  const outside = [...wanted.keys()].sort();

  const index = new Map();
  for (const path of archives) {
    for (const member of archiveMembers(readFileSync(path), basename(path))) {
      for (const name of member.defines) if (!index.has(name)) index.set(name, member);
    }
  }
  const pulled = [];
  const taken = new Set();
  for (let changed = true; changed; ) {
    changed = false;
    for (const [name, from] of [...wanted]) {
      if (defined.has(name)) {
        wanted.delete(name);
        continue;
      }
      const member = index.get(name);
      if (member === undefined || taken.has(member)) continue;
      taken.add(member);
      pulled.push({ member: member.name, symbol: name, from });
      for (const definition of member.defines) defined.add(definition);
      for (const reference of member.references) want(reference, member.name);
      changed = true;
    }
  }
  const unresolved = [...wanted.keys()].filter((name) => !defined.has(name)).sort();

  console.log(`objects: ${objects.map((path) => basename(path)).join(' ')}`);
  console.log(`references outside the objects (${outside.length}): ${outside.join(' ')}`);
  console.log(`archive members pulled in (${pulled.length}):`);
  for (const row of pulled) console.log(`  ${row.member} <- ${row.from} (${row.symbol})`);
  console.log(`unresolved (${unresolved.length}): ${unresolved.join(' ')}`);
  return options.includes('--expect-none') && pulled.length > 0 ? 1 : 0;
}

process.exitCode = main(process.argv.slice(2));
